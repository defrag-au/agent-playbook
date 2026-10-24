//! Bounded, sorted filesystem walks.
//!
//! The same rules `at-peek` applies to a worktree apply here: one directory at a time in sorted
//! order, symlinks not followed, unreadable entries counted rather than fatal, and a cap that is
//! reported. A `target` directory is skipped by name — a build artefact, not source — and a
//! build-output walk starts at `out`, where that name is unlikely to occur.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use at_core::contract::{secret_shaped, Fail};

const SOURCE_SKIPS: &[&str] = &[
    ".cargo",
    ".direnv",
    ".git",
    ".tmp",
    "dist",
    "node_modules",
    "result",
    "target",
];

pub struct File {
    pub path: PathBuf,
    pub rel: String,
    pub len: u64,
    pub modified: Option<SystemTime>,
}

pub struct Walk {
    pub files: Vec<File>,
    pub considered: usize,
    pub capped: bool,
    pub symlinks: usize,
    pub unreadable: usize,
    pub secret_skipped: usize,
    pub skipped_dirs: BTreeSet<&'static str>,
}

/// Every file under `root`, in path order, up to `max_files`. `root` must already be canonical.
pub fn files(root: &Path, max_files: usize, include_secret: bool) -> Result<Walk, Fail> {
    let mut walk = Walk {
        files: Vec::new(),
        considered: 0,
        capped: false,
        symlinks: 0,
        unreadable: 0,
        secret_skipped: 0,
        skipped_dirs: BTreeSet::new(),
    };

    let meta = fs::metadata(root)
        .map_err(|error| Fail::environment(format!("cannot read {}: {error}", root.display())))?;
    if meta.is_file() {
        push_file(root, root, include_secret, &mut walk)?;
        return Ok(walk);
    }
    if !meta.is_dir() {
        return Err(Fail::environment(format!(
            "{} is neither a file nor a directory",
            root.display()
        )));
    }

    descend(root, root, max_files, include_secret, &mut walk)?;
    Ok(walk)
}

fn descend(
    root: &Path,
    dir: &Path,
    max_files: usize,
    include_secret: bool,
    walk: &mut Walk,
) -> Result<(), Fail> {
    let entries = match read_dir_sorted(dir) {
        Ok(entries) => entries,
        Err(_) => {
            walk.unreadable += 1;
            return Ok(());
        }
    };

    for path in entries {
        if walk.considered >= max_files {
            walk.capped = true;
            return Ok(());
        }
        let Ok(meta) = fs::symlink_metadata(&path) else {
            walk.unreadable += 1;
            continue;
        };
        if meta.file_type().is_symlink() {
            walk.symlinks += 1;
            continue;
        }
        if meta.is_dir() {
            let name = file_name(&path);
            match SOURCE_SKIPS.iter().find(|skip| **skip == name) {
                Some(skip) => {
                    walk.skipped_dirs.insert(skip);
                }
                None => descend(root, &path, max_files, include_secret, walk)?,
            }
        } else if meta.is_file() {
            push_file(root, &path, include_secret, walk)?;
        }
    }
    Ok(())
}

fn push_file(root: &Path, path: &Path, include_secret: bool, walk: &mut Walk) -> Result<(), Fail> {
    walk.considered += 1;
    let name = file_name(path);
    if !include_secret && secret_shaped(&name).is_some() {
        walk.secret_skipped += 1;
        return Ok(());
    }
    let Ok(meta) = fs::metadata(path) else {
        walk.unreadable += 1;
        return Ok(());
    };
    walk.files.push(File {
        path: path.to_path_buf(),
        rel: relative(root, path),
        len: meta.len(),
        modified: meta.modified().ok(),
    });
    Ok(())
}

pub struct TreeEntry {
    pub rel: String,
    pub is_dir: bool,
    pub len: u64,
}

pub struct Tree {
    pub entries: Vec<TreeEntry>,
    pub truncated: bool,
    pub symlinks: usize,
    pub unreadable: usize,
    pub secret_skipped: usize,
    pub not_descended: usize,
    pub skipped_dirs: BTreeSet<&'static str>,
}

/// Entries under `root` down to `depth` levels, in sorted order. A directory at the boundary is
/// listed and counted as not descended, so the output says what the depth cut rather than only
/// where it stopped.
pub fn tree(root: &Path, depth: usize, limit: usize, include_secret: bool) -> Result<Tree, Fail> {
    let mut tree = Tree {
        entries: Vec::new(),
        truncated: false,
        symlinks: 0,
        unreadable: 0,
        secret_skipped: 0,
        not_descended: 0,
        skipped_dirs: BTreeSet::new(),
    };

    let meta = fs::metadata(root)
        .map_err(|error| Fail::environment(format!("cannot read {}: {error}", root.display())))?;
    if meta.is_file() {
        tree.entries.push(TreeEntry {
            rel: file_name(root),
            is_dir: false,
            len: meta.len(),
        });
        return Ok(tree);
    }
    if !meta.is_dir() {
        return Err(Fail::environment(format!(
            "{} is neither a file nor a directory",
            root.display()
        )));
    }

    tree_level(root, root, depth, limit, include_secret, &mut tree)?;
    Ok(tree)
}

fn tree_level(
    root: &Path,
    dir: &Path,
    depth: usize,
    limit: usize,
    include_secret: bool,
    tree: &mut Tree,
) -> Result<(), Fail> {
    if depth == 0 {
        return Ok(());
    }
    let entries = match read_dir_sorted(dir) {
        Ok(entries) => entries,
        Err(_) => {
            tree.unreadable += 1;
            return Ok(());
        }
    };

    for path in entries {
        if tree.entries.len() >= limit {
            tree.truncated = true;
            return Ok(());
        }
        let Ok(meta) = fs::symlink_metadata(&path) else {
            tree.unreadable += 1;
            continue;
        };
        if meta.file_type().is_symlink() {
            tree.symlinks += 1;
            continue;
        }
        let rel = relative(root, &path);
        if meta.is_dir() {
            let name = file_name(&path);
            if let Some(skip) = SOURCE_SKIPS.iter().find(|skip| **skip == name) {
                tree.skipped_dirs.insert(*skip);
                continue;
            }
            tree.entries.push(TreeEntry {
                rel,
                is_dir: true,
                len: 0,
            });
            if depth == 1 {
                tree.not_descended += 1;
            } else {
                tree_level(root, &path, depth - 1, limit, include_secret, tree)?;
            }
        } else if meta.is_file() {
            let name = file_name(&path);
            if !include_secret && secret_shaped(&name).is_some() {
                tree.secret_skipped += 1;
                continue;
            }
            tree.entries.push(TreeEntry {
                rel,
                is_dir: false,
                len: meta.len(),
            });
        }
    }
    Ok(())
}

pub fn read_dir_sorted(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .collect();
    paths.sort();
    Ok(paths)
}

pub fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A build output directory, named by the crate its build script belonged to.
#[derive(Clone, Debug)]
pub struct BuildOut {
    pub crate_name: String,
    pub hash: String,
    pub dir: PathBuf,
    pub out: PathBuf,
}

impl BuildOut {
    /// Project-relative when the target is inside the project, absolute otherwise.
    pub fn label(&self, project: Option<&Path>) -> String {
        match project {
            Some(project) => relative(project, &self.out),
            None => self.out.display().to_string(),
        }
    }
}

/// Every `target/**/build/<crate>-<hash>/out`. Two levels of target children are checked, which
/// covers `target/debug/build`, `target/release/build`, and `target/<triple>/<profile>/build`
/// without walking the dependency tree.
pub fn build_outs(target: &Path) -> Vec<BuildOut> {
    let mut build_roots = BTreeSet::new();
    if let Ok(children) = read_dir_sorted(target) {
        for child in children {
            let build = child.join("build");
            if build.is_dir() {
                build_roots.insert(build);
            }
            if let Ok(grandchildren) = read_dir_sorted(&child) {
                for grandchild in grandchildren {
                    let build = grandchild.join("build");
                    if build.is_dir() {
                        build_roots.insert(build);
                    }
                }
            }
        }
    }

    let mut outs = Vec::new();
    for build in build_roots {
        let Ok(entries) = read_dir_sorted(&build) else {
            continue;
        };
        for entry in entries {
            let Some(dir_name) = entry
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
            else {
                continue;
            };
            let Some((crate_name, hash)) = split_build_dir(&dir_name) else {
                continue;
            };
            let out = entry.join("out");
            if out.is_dir() {
                outs.push(BuildOut {
                    crate_name,
                    hash,
                    dir: entry,
                    out,
                });
            }
        }
    }
    outs.sort_by(|a, b| a.out.cmp(&b.out));
    outs
}

/// `<crate>-<hash>`, where the hash is cargo's sixteen hex characters. The whole name is kept
/// when there is no such suffix, so a crate whose name ends in hex is not truncated.
pub fn split_build_dir(dir: &str) -> Option<(String, String)> {
    let (name, hash) = dir.rsplit_once('-')?;
    if name.is_empty() || hash.len() < 8 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some((name.to_string(), hash.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_build_dir_splits_into_crate_and_hash() {
        assert_eq!(
            split_build_dir("headless_chrome-19b2e98dafeccb3e"),
            Some(("headless_chrome".into(), "19b2e98dafeccb3e".into()))
        );
    }

    #[test]
    fn a_directory_without_a_hash_is_not_a_build_dir() {
        assert_eq!(split_build_dir("build"), None);
        assert_eq!(split_build_dir("foo-xyz"), None);
    }
}
