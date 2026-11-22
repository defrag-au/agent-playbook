//! Starting one harness, keeping its streams, and waiting for it.
//!
//! The streams are written to a staging directory outside the checkout, then moved into the
//! results directory once the child is gone. A harness whose stdout or stderr is redirected into
//! a path the corpus seal denies can die at startup (Node resolves `/dev/fd/2` and reads it), so
//! the child never holds a descriptor pointing into the sealed tree.

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use crate::container::Trial;

const POLL: Duration = Duration::from_millis(50);

/// How a trial is confined: the host under a seatbelt profile, or a container with the snapshot
/// mounted. Isolation is the same for every arm, so it never becomes part of what an arm measures.
#[derive(Debug, Clone)]
pub enum Isolation {
    Host { profile: Option<String> },
    Container(Trial),
}

/// What to run, before the sandbox wraps it.
pub struct Invocation {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
}

pub struct Completed {
    pub status: ExitStatus,
    pub timed_out: bool,
    pub wall: Duration,
    pub stdout: PathBuf,
    pub stderr: PathBuf,
}

/// Run `invocation`, writing stdout and stderr beside the results.
pub fn run(
    invocation: Invocation,
    out: &Path,
    stem: &str,
    timeout: Option<Duration>,
    isolation: &Isolation,
) -> Result<Completed> {
    fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    let stdout_path = out.join(format!("{stem}.jsonl"));
    let stderr_path = out.join(format!("{stem}.err"));

    // Inside the denied results tree, a stream file breaks some harnesses at startup; stage it
    // in the temp directory, which the profile allows, and move it in after the run.
    let staging = staging_dir(stem);
    fs::create_dir_all(&staging).with_context(|| format!("creating {}", staging.display()))?;
    let stdout_staging = staging.join("stdout");
    let stderr_staging = staging.join("stderr");
    let stdout = File::create(&stdout_staging)
        .with_context(|| format!("creating {}", stdout_staging.display()))?;
    let stderr = File::create(&stderr_staging)
        .with_context(|| format!("creating {}", stderr_staging.display()))?;

    let (mut command, container) = match isolation {
        Isolation::Host { profile } => {
            let (program, args) = match profile {
                Some(profile) => {
                    let mut args = vec![
                        "-p".to_owned(),
                        profile.to_owned(),
                        invocation.program.display().to_string(),
                    ];
                    args.extend(invocation.args.iter().cloned());
                    (PathBuf::from("/usr/bin/sandbox-exec"), args)
                }
                None => (invocation.program.clone(), invocation.args.clone()),
            };
            let mut command = Command::new(&program);
            command
                .args(&args)
                .current_dir(&invocation.cwd)
                .env("PWD", &invocation.cwd);
            for (key, value) in &invocation.env {
                command.env(key, value);
            }
            (command, None)
        }
        Isolation::Container(trial) => {
            // Secrets go through a file rather than `--env`: an argument list is visible to every
            // process on the host, and a model key is not.
            let env_file = staging.join("env");
            write_env(&env_file, &invocation.env)?;
            let name = container_name(stem);
            let command = trial.command(&name, &invocation, &env_file)?;
            (command, Some(name))
        }
    };

    let started = Instant::now();
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .with_context(|| format!("starting {}", invocation.program.display()))?;
    let (status, timed_out) = wait(&mut child, timeout, container.as_deref())?;

    fs::rename(&stdout_staging, &stdout_path)
        .with_context(|| format!("moving the stream to {}", stdout_path.display()))?;
    fs::rename(&stderr_staging, &stderr_path)
        .with_context(|| format!("moving the stream to {}", stderr_path.display()))?;
    if container.is_some() {
        // The CLI's own chatter is not the harness's: a one-line error must not read as a
        // SIGWINCH warning or a progress bar.
        filter_container_stderr(&stderr_path)?;
    }
    let _ = fs::remove_dir_all(&staging);

    Ok(Completed {
        status,
        timed_out,
        wall: started.elapsed(),
        stdout: stdout_path,
        stderr: stderr_path,
    })
}

/// `KEY=VALUE` lines, mode 600, read by `container run --env-file`.
fn write_env(path: &Path, env: &[(String, String)]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    for (key, value) in env {
        if key.contains('=') || key.contains('\n') || value.contains('\n') {
            bail!("cannot pass `{key}` to a container: a key or value contains a newline or `=`");
        }
        writeln!(file, "{key}={value}").with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

/// A name unique to the job and to this runner: the timeout path kills by it, and two runs of the
/// same scenario must not collide.
fn container_name(stem: &str) -> String {
    let safe: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let safe = safe.trim_matches('-');
    let safe = if safe.is_empty() { "job" } else { safe };
    let capped = &safe[..safe.len().min(48)];
    format!("rig-eval-{}-{capped}", std::process::id())
}

/// Stop the container a timed-out run left behind, then the CLI that started it. Killing only the
/// CLI can leave the harness running with no reader.
fn kill_container(name: &str) {
    let _ = Command::new("container").args(["kill", name]).output();
    let _ = Command::new("container").args(["delete", name]).output();
}

/// Drop the container CLI's own lines, keeping the harness's stderr.
fn filter_container_stderr(path: &Path) -> Result<()> {
    let text = fs::read_to_string(path).unwrap_or_default();
    let kept: Vec<&str> = text
        .lines()
        .filter(|line| !line.starts_with("failed to send signal") && !is_cli_progress(line))
        .collect();
    let mut filtered = kept.join("\n");
    if !filtered.is_empty() {
        filtered.push('\n');
    }
    fs::write(path, filtered).with_context(|| format!("rewriting {}", path.display()))
}

/// `[3/6] Unpacking image [0s]`: the CLI's build/run progress, not the harness's output.
fn is_cli_progress(line: &str) -> bool {
    line.strip_prefix('[')
        .and_then(|rest| rest.split(']').next())
        .is_some_and(|counter| {
            counter.contains('/') && counter.chars().all(|c| c.is_ascii_digit() || c == '/')
        })
}

/// A per-run staging directory outside the checkout, named so a leftover is traceable to a run.
fn staging_dir(stem: &str) -> PathBuf {
    let safe: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    std::env::temp_dir().join(format!("rig-eval-{}-{safe}", std::process::id()))
}

/// The last non-empty line of a harness's stderr, capped for a one-line report.
pub fn tail(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let line = text.lines().rev().find(|line| !line.trim().is_empty())?;
    Some(line.chars().take(300).collect())
}

/// Wait for the child, killing it if the run passes its limit. One owner keeps the poll simple.
fn wait(
    child: &mut Child,
    limit: Option<Duration>,
    container: Option<&str>,
) -> Result<(ExitStatus, bool)> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().context("waiting for the harness")? {
            return Ok((status, false));
        }
        if limit.is_some_and(|limit| started.elapsed() >= limit) {
            if let Some(name) = container {
                kill_container(name);
            }
            let _ = child.kill();
            return Ok((child.wait().context("waiting for the harness")?, true));
        }
        thread::sleep(POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_container_name_is_safe_and_short() {
        let name = container_name("recover-investigation:rig:gated:r1");
        assert!(name.starts_with(&format!("rig-eval-{}-", std::process::id())));
        assert!(!name.contains(':'));
        assert!(name.len() <= 60);
    }

    #[test]
    fn the_container_clis_progress_is_not_the_harnesses() {
        assert!(is_cli_progress("[0/6] [0s]"));
        assert!(is_cli_progress("[6/6] Starting container [0s]"));
        assert!(!is_cli_progress("[warn] the harness said this"));
        assert!(!is_cli_progress("error: refused"));
    }

    #[test]
    fn filtering_drops_the_cli_and_keeps_the_harness() {
        let path = std::env::temp_dir().join(format!("rig-eval-stderr-{}", std::process::id()));
        fs::write(
            &path,
            "failed to send signal: [\"signal\": 28]\n[3/6] Unpacking image [0s]\nrig: refused\n",
        )
        .expect("a fixture");
        filter_container_stderr(&path).expect("a filtered file");
        let text = fs::read_to_string(&path).expect("the filtered file");
        assert_eq!(text, "rig: refused\n");
        let _ = fs::remove_file(&path);
    }
}
