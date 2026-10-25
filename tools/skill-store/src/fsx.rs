//! Small filesystem and flat-config helpers shared by the store operations.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Every regular file under `root`, as `(relative path with '/', absolute path)`, sorted.
///
/// Symlinks are refused rather than followed: OCFL forbids them in a storage hierarchy, and a
/// skill tree that contains one is not portable. VCS metadata is skipped so a skill published
/// from a working copy does not capture `.git/`.
pub fn walk_files(root: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let mut out = Vec::new();
    walk(root, root, &mut out, 0)?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn walk(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(String, PathBuf)>,
    depth: usize,
) -> Result<(), String> {
    if depth > 32 {
        return Err(format!(
            "{}: directory nesting is deeper than 32 levels",
            dir.display()
        ));
    }
    let entries = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    paths.sort();
    for path in paths {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("{}: file name is not valid UTF-8", path.display()))?;
        let metadata =
            fs::symlink_metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "{}: symlinks are not portable in a skill tree — replace it with a file or a directory",
                path.display()
            ));
        }
        if metadata.is_dir() {
            if name == ".git" {
                continue;
            }
            walk(root, &path, out, depth + 1)?;
        } else if metadata.is_file() {
            if name == ".DS_Store" {
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|e| format!("{}: {e}", path.display()))?
                .to_string_lossy()
                .replace('\\', "/");
            out.push((relative, path));
        }
    }
    Ok(())
}

/// A logical path is relative, uses `/`, and has no `.`/`..`/empty segment.
pub fn safe_relative(path: &str) -> Result<(), String> {
    if path.is_empty() || path.starts_with('/') {
        return Err(format!("`{path}` is not a relative path"));
    }
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(format!("`{path}` is not a safe relative path"));
        }
    }
    Ok(())
}

/// `~/x` to `$HOME/x`.
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

/// Read a flat `key: value` file. Comments and blank lines are ignored; a duplicate key is an
/// error rather than a silent last-wins, because a lock that says two things is not a lock.
pub fn read_flat(path: &Path) -> Result<BTreeMap<String, String>, String> {
    let source = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = BTreeMap::new();
    for (number, line) in source.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            return Err(format!(
                "{}:{}: expected `key: value`, got `{line}`",
                path.display(),
                number + 1
            ));
        };
        let key = key.trim().to_string();
        let value = value.trim().to_string();
        if key.is_empty() {
            return Err(format!("{}:{}: empty key", path.display(), number + 1));
        }
        if out.insert(key.clone(), value).is_some() {
            return Err(format!(
                "{}:{}: duplicate key `{key}`",
                path.display(),
                number + 1
            ));
        }
    }
    Ok(out)
}

pub fn write_flat(path: &Path, pairs: &[(&str, String)]) -> Result<(), String> {
    let mut body = String::new();
    for (key, value) in pairs {
        body.push_str(key);
        body.push_str(": ");
        body.push_str(value);
        body.push('\n');
    }
    fs::write(path, body).map_err(|e| format!("{}: {e}", path.display()))
}
