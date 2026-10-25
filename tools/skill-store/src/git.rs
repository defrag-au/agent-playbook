//! Fetching a git-hosted store into a cache.
//!
//! One subprocess, `git`, with read verbs only — the same tier as `at-recall`. The cache is a
//! full clone per URL (a skill store is small, and every OCFL version lives in the working tree),
//! keyed by the URL so two sources from the same repository share one checkout.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::digest::{self, Algorithm};

pub struct Cache {
    root: PathBuf,
}

impl Cache {
    pub fn discover() -> Result<Self, String> {
        let base = if let Some(path) = std::env::var_os("SKILL_STORE_CACHE") {
            PathBuf::from(path)
        } else if let Some(path) = std::env::var_os("XDG_CACHE_HOME") {
            PathBuf::from(path).join("skill-store")
        } else if let Some(home) = std::env::var_os("HOME") {
            PathBuf::from(home).join(".cache").join("skill-store")
        } else {
            return Err(
                "cannot find a cache directory: set HOME, XDG_CACHE_HOME or SKILL_STORE_CACHE"
                    .to_string(),
            );
        };
        Ok(Cache { root: base })
    }

    /// Fetch (or reuse) a checkout, then check out `rev`, `ref_` or the default branch.
    ///
    /// Returns the checkout directory and the commit that was checked out, which is what a
    /// consumer records in its lock.
    pub fn fetch(
        &self,
        url: &str,
        ref_: Option<&str>,
        rev: Option<&str>,
        offline: bool,
    ) -> Result<(PathBuf, String), String> {
        let key = digest::bytes_hex(url.as_bytes(), Algorithm::Sha256);
        let dir = self.root.join(&key[..16]);
        let cloned = dir.join(".git").is_dir();

        if !cloned {
            if offline {
                return Err(format!(
                    "no cached checkout for {url} — run once without `--offline` to fetch it"
                ));
            }
            std::fs::create_dir_all(&self.root)
                .map_err(|e| format!("{}: {e}", self.root.display()))?;
            let status = Command::new("git")
                .args(["clone", "--quiet", url])
                .arg(&dir)
                .env("GIT_TERMINAL_PROMPT", "0")
                .output()
                .map_err(|e| format!("cannot run git: {e}"))?;
            if !status.status.success() {
                let _ = std::fs::remove_dir_all(&dir);
                return Err(format!(
                    "git clone {url}: {}",
                    String::from_utf8_lossy(&status.stderr).trim()
                ));
            }
        } else if !offline {
            git(&[
                "-C",
                path(&dir),
                "fetch",
                "--quiet",
                "--tags",
                "--prune",
                "origin",
            ])?;
        }

        if let Some(rev) = rev {
            if try_checkout(&dir, rev) {
                return Ok((dir.clone(), current_rev(&dir)?));
            }
            if offline {
                return Err(format!(
                    "{url}: `{rev}` is not in the cached checkout, and `--offline` was given"
                ));
            }
            git(&["-C", path(&dir), "fetch", "--quiet", "origin", rev])?;
            checkout(&dir, "FETCH_HEAD")?;
            return Ok((dir.clone(), current_rev(&dir)?));
        }

        if let Some(ref_) = ref_ {
            if try_checkout(&dir, ref_) || try_checkout(&dir, &format!("origin/{ref_}")) {
                return Ok((dir.clone(), current_rev(&dir)?));
            }
            if offline {
                return Err(format!(
                    "{url}: `{ref_}` is not in the cached checkout, and `--offline` was given"
                ));
            }
            git(&["-C", path(&dir), "fetch", "--quiet", "origin", ref_])?;
            checkout(&dir, "FETCH_HEAD")?;
            return Ok((dir.clone(), current_rev(&dir)?));
        }

        // No ref: the default branch. `origin/HEAD` is a symbolic ref set by clone.
        if try_checkout(&dir, "origin/HEAD") {
            return Ok((dir.clone(), current_rev(&dir)?));
        }
        for fallback in ["origin/main", "origin/master"] {
            if try_checkout(&dir, fallback) {
                return Ok((dir.clone(), current_rev(&dir)?));
            }
        }
        Err(format!(
            "{url}: cannot tell which branch is the default — pass `?ref=`"
        ))
    }
}

fn path(dir: &Path) -> &str {
    dir.to_str().unwrap_or("")
}

fn git(args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn try_checkout(dir: &Path, target: &str) -> bool {
    Command::new("git")
        .args([
            "-C",
            path(dir),
            "checkout",
            "--quiet",
            "--force",
            "--detach",
        ])
        .arg(target)
        .env("GIT_TERMINAL_PROMPT", "0")
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn checkout(dir: &Path, target: &str) -> Result<(), String> {
    git(&[
        "-C",
        path(dir),
        "checkout",
        "--quiet",
        "--force",
        "--detach",
        target,
    ])
    .map(|_| ())
}

fn current_rev(dir: &Path) -> Result<String, String> {
    git(&["-C", path(dir), "rev-parse", "HEAD"])
}
