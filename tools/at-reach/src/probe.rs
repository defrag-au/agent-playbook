//! The read-only probe, rendered in the toolkit's output contract.
//!
//! The probe is one fixed POSIX shell script. It is run with `sh` when the profile names the
//! local host, and with `ssh` otherwise. Nothing in the profile becomes an arbitrary shell
//! fragment: the script is a constant, and the only variable parts are repo paths passed as
//! arguments and quoted for the remote shell.

use std::path::PathBuf;
use std::process::Command;

use at_core::contract::{Fail, Outcome, Report};

use crate::fleet::{Fleet, Profile};
use crate::TOOL;

/// The script every probe runs. It is read-only: no sudo, no rebuild, no writes of its own.
/// `git status` may refresh the index's stat cache, which is git's own write.
const PROBE_SCRIPT: &str = r#"
printf 'hostname=%s\n' "$(hostname 2>/dev/null || printf unknown)"
printf 'arch=%s\n' "$(uname -m 2>/dev/null || printf unknown)"
if command -v sw_vers >/dev/null 2>&1; then
  printf 'os=macos %s\n' "$(sw_vers -productVersion 2>/dev/null || printf unknown)"
elif [ -r /etc/os-release ]; then
  . /etc/os-release
  printf 'os=%s\n' "${PRETTY_NAME:-linux}"
else
  printf 'os=%s\n' "$(uname -s 2>/dev/null || printf unknown)"
fi
if command -v nix >/dev/null 2>&1; then
  printf 'nix=%s\n' "$(nix --version 2>/dev/null | head -n1)"
else
  printf 'nix=missing\n'
fi
if command -v home-manager >/dev/null 2>&1; then
  printf 'home-manager=%s\n' "$(home-manager generations 2>/dev/null | head -n1)"
else
  printf 'home-manager=missing\n'
fi
if command -v tailscale >/dev/null 2>&1; then
  printf 'tailscale_ip=%s\n' "$(tailscale ip -4 2>/dev/null | head -n1)"
  printf 'tailscale=%s\n' "$(tailscale status 2>/dev/null | head -n1)"
else
  printf 'tailscale=missing\n'
fi
if command -v df >/dev/null 2>&1; then
  printf 'disk_free_kb=%s\n' "$(df -Pk / 2>/dev/null | awk 'NR==2 {print $4}')"
fi
for path in "$@"; do
  if git -C "$path" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    sha=$(git -C "$path" rev-parse --short HEAD 2>/dev/null || printf -- -)
    changes=$(git -C "$path" status --porcelain 2>/dev/null | wc -l | tr -d ' ')
    printf 'repo=%s head=%s changes=%s\n' "$path" "$sha" "$changes"
  else
    printf 'repo=%s missing\n' "$path"
  fi
done
"#;

pub fn root(fleet: &Fleet) -> Outcome {
    let mut report = Report::new();
    report.header(
        TOOL,
        "root",
        &fleet.root().display().to_string(),
        fleet.source(),
    );
    report.content(format!("root={}", fleet.root().display()));
    report.content(format!("source={}", fleet.source()));
    report.content(format!("profiles={}", fleet.profiles().len()));
    Outcome::from_report(report)
}

pub fn hosts(fleet: &Fleet) -> Outcome {
    let mut report = Report::new();
    report.header(
        TOOL,
        "hosts",
        &fleet.root().display().to_string(),
        &format!("{} profile(s)", fleet.profiles().len()),
    );
    for profile in fleet.profiles() {
        let os_arch = format!("{}/{}", profile.os, profile.arch);
        report.content(format!(
            "{:<14} {:<8} {:<16} tailscale={:<18} ssh={:<14} writes={}",
            profile.id,
            profile.role,
            os_arch,
            profile.tailscale.as_deref().unwrap_or("-"),
            profile.ssh,
            profile.writes,
        ));
    }
    report.bound(format!("{} host(s)", fleet.profiles().len()));
    Outcome::from_report(report)
}

pub fn probe(fleet: &Fleet, id: &str) -> Result<Outcome, Fail> {
    let profile = fleet.find(id).ok_or_else(|| {
        Fail::usage(format!(
            "unknown host `{id}` · known: {}",
            if fleet.profiles().is_empty() {
                "none".to_string()
            } else {
                fleet.ids()
            }
        ))
    })?;

    let paths = profile.repo_paths()?;
    let local = is_local(profile);
    let (stdout, stderr, status) = if local {
        run_local(&paths)?
    } else {
        run_ssh(profile, &paths)?
    };

    if !status.success() {
        return Err(Fail::environment(format!(
            "probe of {} failed ({status}) · {}",
            profile.id,
            first_line(&stderr)
        )));
    }

    let mut report = Report::new();
    let target = if local {
        format!("{} · local", profile.id)
    } else {
        format!("{} · ssh {}", profile.id, profile.ssh)
    };
    report.header(TOOL, "probe", &fleet.root().display().to_string(), &target);
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        report.content(line);
    }
    if !stderr.trim().is_empty() {
        report.bound(format!("stderr: {}", first_line(&stderr)));
    }
    Ok(Outcome::from_report(report))
}

fn is_local(profile: &Profile) -> bool {
    if profile.ssh.eq_ignore_ascii_case("local") {
        return true;
    }
    matches_id(&local_names(), &profile.id)
}

/// The names a machine answers to. On macOS `hostname` often returns `Mac.lan`, while the
/// inventory names the machine `defrag-air`; `scutil` supplies the names a reader would
/// recognise. All are collected, and one match is enough.
fn local_names() -> Vec<String> {
    let mut names = Vec::new();
    if let Some(name) = output_name(Command::new("hostname")) {
        names.push(name);
    }
    let mut short = Command::new("hostname");
    short.arg("-s");
    if let Some(name) = output_name(short) {
        names.push(name);
    }
    for key in ["ComputerName", "LocalHostName"] {
        let mut scutil = Command::new("scutil");
        scutil.args(["--get", key]);
        if let Some(name) = output_name(scutil) {
            names.push(name);
        }
    }
    if let Ok(value) = std::env::var("HOSTNAME") {
        let value = value.trim().to_string();
        if !value.is_empty() {
            names.push(value);
        }
    }
    names.sort();
    names.dedup();
    names
}

fn output_name(mut command: Command) -> Option<String> {
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!name.is_empty()).then_some(name)
}

fn matches_id(names: &[String], id: &str) -> bool {
    names.iter().any(|name| name.eq_ignore_ascii_case(id))
}

fn run_local(paths: &[PathBuf]) -> Result<(String, String, std::process::ExitStatus), Fail> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(PROBE_SCRIPT)
        .arg("--")
        .args(paths)
        .output()
        .map_err(|e| Fail::environment(format!("cannot run the local probe: {e}")))?;
    Ok((
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status,
    ))
}

fn run_ssh(
    profile: &Profile,
    paths: &[PathBuf],
) -> Result<(String, String, std::process::ExitStatus), Fail> {
    if profile.ssh.trim().is_empty() {
        return Err(Fail::environment(format!("{}: `ssh` is empty", profile.id)));
    }
    let remote = remote_command(paths);
    let output = Command::new("ssh")
        .args([
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=5",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=2",
        ])
        .arg(&profile.ssh)
        .arg("--")
        .arg(&remote)
        .output()
        .map_err(|e| Fail::environment(format!("cannot run ssh for {}: {e}", profile.id)))?;
    Ok((
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status,
    ))
}

fn remote_command(paths: &[PathBuf]) -> String {
    let mut command = format!("sh -c {} --", shell_quote(PROBE_SCRIPT));
    for path in paths {
        command.push(' ');
        command.push_str(&shell_quote(&path.to_string_lossy()));
    }
    command
}

/// POSIX single-quote a string. The script and the paths are the only things that reach a
/// remote shell, and this is what keeps a path with a quote or a space from becoming syntax.
fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    let mut quoted = String::from("'");
    for ch in value.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

fn first_line(text: &str) -> String {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .unwrap_or_else(|| "no stderr".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_path_is_single_quoted() {
        assert_eq!(shell_quote("/home/user/code"), "'/home/user/code'");
    }

    #[test]
    fn a_quote_in_a_path_is_escaped() {
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn the_remote_command_passes_paths_as_arguments() {
        let command = remote_command(&[PathBuf::from("/a b"), PathBuf::from("/c'd")]);
        assert!(command.starts_with("sh -c '"));
        assert!(command.contains("'/a b'"));
        assert!(command.contains("'/c'\\''d'"));
    }

    #[test]
    fn the_probe_script_names_the_facts_it_reports() {
        assert!(PROBE_SCRIPT.contains("hostname="));
        assert!(PROBE_SCRIPT.contains("tailscale_ip="));
        assert!(PROBE_SCRIPT.contains("repo="));
        assert!(!PROBE_SCRIPT.contains("sudo"));
    }

    #[test]
    fn a_mac_name_and_an_inventory_name_can_differ() {
        let names = vec!["Mac.lan".to_string(), "defrag-air".to_string()];
        assert!(matches_id(&names, "defrag-air"));
        assert!(!matches_id(&names, "defrag-mini"));
    }
}
