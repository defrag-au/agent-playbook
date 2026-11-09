//! Starting one harness, keeping its streams, and waiting for it.
//!
//! The streams are written to a staging directory outside the checkout, then moved into the
//! results directory once the child is gone. A harness whose stdout or stderr is redirected into
//! a path the corpus seal denies can die at startup (Node resolves `/dev/fd/2` and reads it), so
//! the child never holds a descriptor pointing into the sealed tree.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

const POLL: Duration = Duration::from_millis(50);

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
    sandbox: Option<&str>,
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

    let (program, args) = match sandbox {
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
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    for (key, value) in &invocation.env {
        command.env(key, value);
    }

    let started = Instant::now();
    let mut child = command
        .spawn()
        .with_context(|| format!("starting {}", program.display()))?;
    let (status, timed_out) = wait(&mut child, timeout)?;

    fs::rename(&stdout_staging, &stdout_path)
        .with_context(|| format!("moving the stream to {}", stdout_path.display()))?;
    fs::rename(&stderr_staging, &stderr_path)
        .with_context(|| format!("moving the stream to {}", stderr_path.display()))?;
    let _ = fs::remove_dir(&staging);

    Ok(Completed {
        status,
        timed_out,
        wall: started.elapsed(),
        stdout: stdout_path,
        stderr: stderr_path,
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
fn wait(child: &mut Child, limit: Option<Duration>) -> Result<(ExitStatus, bool)> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().context("waiting for the harness")? {
            return Ok((status, false));
        }
        if limit.is_some_and(|limit| started.elapsed() >= limit) {
            let _ = child.kill();
            return Ok((child.wait().context("waiting for the harness")?, true));
        }
        thread::sleep(POLL);
    }
}
