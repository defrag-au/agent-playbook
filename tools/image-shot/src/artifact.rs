//! Derived-view naming and artifact containment.
//!
//! The tool owns one directory. `--out` can name a path inside it and nothing else; an omitted
//! `--out` gets a name derived from the source bytes and the canonical operation, so the same
//! request is the same file and a changed source is a new one.

use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};

/// FNV-1a, for a scratch filename. This is not a security hash; it is a deterministic name.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn hash_hex(bytes: &[u8]) -> String {
    format!("{:012x}", fnv1a(bytes) & 0x0000_ffff_ffff_ffff)
}

/// A filename-safe form of the source's stem.
fn slug(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("image");
    let cleaned: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .take(48)
        .collect();
    if cleaned.is_empty() {
        "image".to_owned()
    } else {
        cleaned
    }
}

/// `<stem>-<hash>.<ext>`, where the hash covers the source bytes, the canonical op and the tool
/// version. That makes the derived view idempotent and makes a changed input a different file.
pub fn derived_name(src: &Path, op: &str, ext: &str, source_bytes: &[u8]) -> String {
    let mut input = Vec::with_capacity(source_bytes.len() + op.len() + 16);
    input.extend_from_slice(source_bytes);
    input.extend_from_slice(op.as_bytes());
    input.extend_from_slice(env!("CARGO_PKG_VERSION").as_bytes());
    format!("{}-{}.{}", slug(src), hash_hex(&input), ext)
}

/// The one directory this tool may write into.
pub struct Artifacts {
    root: PathBuf,
    cwd: PathBuf,
}

impl Artifacts {
    pub fn new(dir: Option<&str>, cwd: &Path) -> Result<Self> {
        let given = dir.unwrap_or(".tmp/image-shot");
        let root = if Path::new(given).is_absolute() {
            PathBuf::from(given)
        } else {
            cwd.join(given)
        };
        std::fs::create_dir_all(&root)
            .with_context(|| format!("create the artifacts directory {}", root.display()))?;
        let root = std::fs::canonicalize(&root)
            .with_context(|| format!("resolve the artifacts directory {}", root.display()))?;
        if !root.is_dir() {
            bail!("artifacts {} is not a directory", root.display());
        }
        Ok(Self {
            root,
            cwd: cwd.to_path_buf(),
        })
    }

    /// Resolve `--out`, or the derived name when it is absent, and refuse anything that escapes
    /// the root. The parent is created, then canonicalised, so a symlinked subdirectory cannot
    /// point the write outside.
    pub fn resolve(&self, out: Option<&str>, derived: &str) -> Result<PathBuf> {
        let given = out.unwrap_or(derived);
        if given.is_empty() {
            bail!("--out is empty");
        }
        let path = Path::new(given);
        if path.is_absolute() {
            bail!(
                "--out must be relative to the artifacts directory ({})",
                self.root.display()
            );
        }
        if path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
        {
            bail!("--out must not contain `..`");
        }
        let Some(name) = path.file_name() else {
            bail!("--out must name a file");
        };

        let destination = self.root.join(path);
        let parent = destination.parent().unwrap_or(&self.root);
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        let parent = std::fs::canonicalize(parent)
            .with_context(|| format!("resolve {}", parent.display()))?;
        if !parent.starts_with(&self.root) {
            bail!(
                "--out {} resolves outside the artifacts directory ({})",
                given,
                self.root.display()
            );
        }
        Ok(parent.join(name))
    }

    /// The path as a reader wants it: relative to the working directory when it is below it.
    pub fn display(&self, path: &Path) -> String {
        display(path, &self.cwd)
    }
}

/// A path relative to the working directory when possible, forward slashes.
pub fn display(path: &Path, cwd: &Path) -> String {
    path.strip_prefix(cwd)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_is_stable() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn derived_name_is_stable_and_input_sensitive() {
        let path = Path::new("shots/home view.png");
        let a = derived_name(path, "full/max/0/default.png", "png", b"one");
        let b = derived_name(path, "full/max/0/default.png", "png", b"one");
        let c = derived_name(path, "full/max/0/default.png", "png", b"two");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.starts_with("home-view-"));
        assert!(a.ends_with(".png"));
    }

    #[test]
    fn out_must_stay_inside_the_root() {
        let cwd = std::env::temp_dir().join(format!("image-shot-artifacts-{}", std::process::id()));
        std::fs::create_dir_all(&cwd).unwrap();
        let artifacts = Artifacts::new(Some(".tmp/image-shot"), &cwd).unwrap();
        assert!(artifacts.resolve(Some("../escape.png"), "x.png").is_err());
        assert!(artifacts.resolve(Some("/tmp/escape.png"), "x.png").is_err());
        assert!(artifacts.resolve(Some("crops/top.png"), "x.png").is_ok());
        std::fs::remove_dir_all(&cwd).ok();
    }
}
