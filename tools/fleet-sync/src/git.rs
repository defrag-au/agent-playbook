//! The git calls this tool makes, with the command named in every failure.
//!
//! `bundle` reads (and writes only a bundle file); `apply` fetches, resets and cleans. Neither
//! reaches another machine: there is no `ssh` here, deliberately.

use std::path::Path;
use std::process::Command;

use at_core::contract::Fail;

/// Run git with no repository context. Only `clone` needs this.
pub fn command(args: &[&str]) -> Result<(), Fail> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|error| Fail::environment(format!("cannot run git: {error}")))?;
    if !output.status.success() {
        return Err(Fail::environment(format!(
            "git {} failed · {}",
            args.join(" "),
            first_line(&String::from_utf8_lossy(&output.stderr))
        )));
    }
    Ok(())
}

/// Run git in a repository and discard stdout.
pub fn run(repo: &Path, args: &[&str]) -> Result<(), Fail> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|error| {
            Fail::environment(format!("cannot run git in {}: {error}", repo.display()))
        })?;
    if !output.status.success() {
        return Err(Fail::environment(format!(
            "git {} failed in {} · {}",
            args.join(" "),
            repo.display(),
            first_line(&String::from_utf8_lossy(&output.stderr))
        )));
    }
    Ok(())
}

/// Run git in a repository and return stdout exactly as git wrote it.
pub fn capture(repo: &Path, args: &[&str]) -> Result<String, Fail> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|error| {
            Fail::environment(format!("cannot run git in {}: {error}", repo.display()))
        })?;
    if !output.status.success() {
        return Err(Fail::environment(format!(
            "git {} failed in {} · {}",
            args.join(" "),
            repo.display(),
            first_line(&String::from_utf8_lossy(&output.stderr))
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn capture_trimmed(repo: &Path, args: &[&str]) -> Result<String, Fail> {
    Ok(capture(repo, args)?.trim().to_string())
}

pub fn is_work_tree(repo: &Path) -> bool {
    capture_trimmed(repo, &["rev-parse", "--is-inside-work-tree"])
        .is_ok_and(|value| value == "true")
}

pub fn head(repo: &Path) -> Result<String, Fail> {
    capture_trimmed(repo, &["rev-parse", "HEAD"])
}

/// The checked-out branch, or `None` for a detached `HEAD`.
pub fn branch(repo: &Path) -> Result<Option<String>, Fail> {
    match capture_trimmed(repo, &["symbolic-ref", "--short", "HEAD"]) {
        Ok(name) if !name.is_empty() => Ok(Some(name)),
        Ok(_) => Ok(None),
        Err(_) => Ok(None),
    }
}

/// Tracked changes only: an untracked build directory is not a dirty tree, and a satellite is
/// expected to have one.
pub fn tracked_changes(repo: &Path) -> Result<usize, Fail> {
    let output = capture(repo, &["status", "--porcelain", "--untracked-files=no"])?;
    Ok(output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count())
}

fn first_line(text: &str) -> String {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .unwrap_or_else(|| "no stderr".to_string())
}
