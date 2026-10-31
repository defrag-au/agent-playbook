//! Disk accounting and the one deletion this tool performs.
//!
//! The artifact list is fixed and deliberately short. `target` is the reason this tool exists:
//! on the machine this was written on it was 43 GB against a 36 MB `.git`. Nothing outside a
//! repository's own artifact directories is touched, and a symlink is left alone rather than
//! followed.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use at_core::contract::Fail;

pub const ARTIFACTS: &[&str] = &["target", ".direnv", "node_modules", ".tmp", ".venv"];

#[derive(Debug, Default)]
pub struct Cleaned {
    pub removed: Vec<String>,
    pub skipped: Vec<String>,
    pub bytes: u64,
}

/// `du -sk` for one path. A missing path is zero bytes, not an error.
pub fn du_kb(path: &Path) -> Result<u64, Fail> {
    if !path.exists() {
        return Ok(0);
    }
    let output = Command::new("du")
        .arg("-sk")
        .arg(path)
        .output()
        .map_err(|error| {
            Fail::environment(format!("cannot run du on {}: {error}", path.display()))
        })?;
    if !output.status.success() {
        return Err(Fail::environment(format!(
            "du failed for {} · {}",
            path.display(),
            first_line(&String::from_utf8_lossy(&output.stderr))
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let kb = text
        .split_whitespace()
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| Fail::environment(format!("du printed no size for {}", path.display())))?;
    Ok(kb)
}

pub fn artifact_bytes(repo: &Path) -> Result<u64, Fail> {
    let mut total = 0;
    for name in ARTIFACTS {
        total += du_kb(&repo.join(name))? * 1024;
    }
    Ok(total)
}

/// What a clone of this repository needs beyond its artifacts: the source plus `.git`.
pub fn worktree_bytes(repo: &Path) -> Result<u64, Fail> {
    let total = du_kb(repo)? * 1024;
    Ok(total.saturating_sub(artifact_bytes(repo)?))
}

/// Free bytes on the filesystem holding `path`, walking up to the nearest existing directory so
/// a not-yet-created clone target still answers.
pub fn free_kb(path: &Path) -> Result<u64, Fail> {
    let mut probe = path.to_path_buf();
    while !probe.exists() {
        if !probe.pop() {
            probe = PathBuf::from(".");
            break;
        }
    }

    let output = Command::new("df")
        .arg("-Pk")
        .arg(&probe)
        .output()
        .map_err(|error| {
            Fail::environment(format!("cannot run df on {}: {error}", probe.display()))
        })?;
    if !output.status.success() {
        return Err(Fail::environment(format!(
            "df failed for {} · {}",
            probe.display(),
            first_line(&String::from_utf8_lossy(&output.stderr))
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text
        .lines()
        .nth(1)
        .ok_or_else(|| Fail::environment(format!("df printed no row for {}", probe.display())))?;
    let available = line
        .split_whitespace()
        .nth(3)
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| {
            Fail::environment(format!("df printed no free space for {}", probe.display()))
        })?;
    Ok(available)
}

/// Remove the fixed artifact directories, reporting what was removed and what was left alone.
pub fn clean_artifacts(repo: &Path) -> Result<Cleaned, Fail> {
    let mut cleaned = Cleaned::default();

    for name in ARTIFACTS {
        let path = repo.join(name);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(Fail::environment(format!(
                    "cannot read {}: {error}",
                    path.display()
                )))
            }
        };

        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            cleaned.skipped.push((*name).to_string());
            continue;
        }

        let kb = du_kb(&path)?;
        fs::remove_dir_all(&path).map_err(|error| {
            Fail::environment(format!("cannot remove {}: {error}", path.display()))
        })?;
        cleaned.bytes += kb * 1024;
        cleaned.removed.push((*name).to_string());
    }

    Ok(cleaned)
}

pub fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
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
    fn bytes_are_reported_in_binary_units() {
        assert_eq!(human(512), "512 B");
        assert_eq!(human(1024), "1.0 KiB");
        assert_eq!(human(1024 * 1024), "1.0 MiB");
        assert_eq!(human(43 * 1024 * 1024 * 1024), "43.0 GiB");
    }

    #[test]
    fn a_missing_artifact_is_zero_bytes() {
        let root = std::env::temp_dir().join(format!("fleet-sync-du-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("temp root");
        assert_eq!(du_kb(&root.join("target")).expect("du"), 0);
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn only_artifact_directories_are_removed() {
        let root = std::env::temp_dir().join(format!("fleet-sync-clean-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("target")).expect("target");
        fs::create_dir_all(root.join(".direnv")).expect("direnv");
        fs::write(root.join("target/artifact.bin"), "x").expect("artifact");
        fs::write(root.join(".env"), "keep me").expect("env");

        let cleaned = clean_artifacts(&root).expect("clean");
        assert_eq!(cleaned.removed, ["target", ".direnv"]);
        assert!(!root.join("target").exists());
        assert!(!root.join(".direnv").exists());
        assert!(root.join(".env").exists());
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_symlinked_artifact_is_left_alone() {
        let root = std::env::temp_dir().join(format!("fleet-sync-link-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("root");
        let outside = root.join("outside");
        fs::create_dir_all(&outside).expect("outside");
        std::os::unix::fs::symlink(&outside, root.join("target")).expect("symlink");

        let cleaned = clean_artifacts(&root).expect("clean");
        assert!(cleaned.removed.is_empty());
        assert_eq!(cleaned.skipped, ["target"]);
        assert!(outside.exists());
        fs::remove_dir_all(&root).expect("cleanup");
    }
}
