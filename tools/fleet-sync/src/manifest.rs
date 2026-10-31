//! The payload manifest: flat `key: value` text, the same shape as a fleet profile.
//!
//! No JSON and no parser dependency. It carries only what `apply` needs to verify a bundle and
//! reset a checkout: the host it was built for, the repository, the ref, the commit, the bundle
//! file and its size and digest.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::Path;

use at_core::contract::Fail;
use sha2::{Digest, Sha256};

pub const FILE: &str = "manifest";
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The repository entry as the profile spells it, relative to `repo_root` unless absolute.
    pub repo: String,
    /// `refs/heads/main`, or `HEAD` for a detached checkout.
    pub reference: String,
    pub commit: String,
    pub bundle: String,
    pub bytes: u64,
    pub sha256: String,
    /// The source plus `.git` without artifacts: what a clone of this repository needs.
    pub worktree_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub host: String,
    pub entries: Vec<Entry>,
}

impl Manifest {
    pub fn render(&self) -> String {
        let mut out = format!("version: {VERSION}\nhost: {}\n", self.host);
        for entry in &self.entries {
            out.push_str(&format!(
                "\nrepo: {}\nref: {}\ncommit: {}\nbundle: {}\nbytes: {}\nsha256: {}\nworktree_bytes: {}\n",
                entry.repo,
                entry.reference,
                entry.commit,
                entry.bundle,
                entry.bytes,
                entry.sha256,
                entry.worktree_bytes,
            ));
        }
        out
    }

    pub fn write(&self, dir: &Path) -> Result<(), Fail> {
        let path = dir.join(FILE);
        fs::write(&path, self.render())
            .map_err(|error| Fail::environment(format!("cannot write {}: {error}", path.display())))
    }

    pub fn read(dir: &Path) -> Result<Manifest, Fail> {
        let path = dir.join(FILE);
        let raw = fs::read_to_string(&path).map_err(|error| {
            Fail::environment(format!(
                "cannot read the manifest {}: {error}",
                path.display()
            ))
        })?;
        Manifest::parse(&path, &raw)
    }

    fn parse(path: &Path, raw: &str) -> Result<Manifest, Fail> {
        let mut blocks: Vec<Vec<(usize, String, String)>> = Vec::new();
        let mut current = Vec::new();

        for (index, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                if !current.is_empty() {
                    blocks.push(std::mem::take(&mut current));
                }
                continue;
            }
            if line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                return Err(Fail::environment(format!(
                    "{}:{}: expected `key: value`",
                    path.display(),
                    index + 1
                )));
            };
            let key = key.trim();
            if key.is_empty() {
                return Err(Fail::environment(format!(
                    "{}:{}: empty key",
                    path.display(),
                    index + 1
                )));
            }
            current.push((index + 1, key.to_string(), value.trim().to_string()));
        }
        if !current.is_empty() {
            blocks.push(current);
        }

        let mut blocks = blocks.into_iter();
        let header = blocks
            .next()
            .ok_or_else(|| Fail::environment(format!("{}: empty manifest", path.display())))?;
        let header = block_map(path, "manifest", header)?;

        let version = header
            .get("version")
            .ok_or_else(|| missing(path, "version"))?;
        let version = version.parse::<u32>().map_err(|_| {
            Fail::environment(format!(
                "{}: version `{version}` is not a number",
                path.display()
            ))
        })?;
        if version != VERSION {
            return Err(Fail::environment(format!(
                "{}: manifest version {version}, expected {VERSION}",
                path.display()
            )));
        }

        let host = header
            .get("host")
            .ok_or_else(|| missing(path, "host"))?
            .clone();

        let mut entries = Vec::new();
        for (number, block) in blocks.enumerate() {
            let map = block_map(path, &format!("entry {}", number + 1), block)?;
            let required = |key: &str| -> Result<String, Fail> {
                map.get(key).cloned().ok_or_else(|| missing(path, key))
            };
            let number = |key: &str| -> Result<u64, Fail> {
                let value = required(key)?;
                value.parse::<u64>().map_err(|_| {
                    Fail::environment(format!("{}: `{key}` is not a byte count", path.display()))
                })
            };

            entries.push(Entry {
                repo: required("repo")?,
                reference: required("ref")?,
                commit: required("commit")?,
                bundle: required("bundle")?,
                bytes: number("bytes")?,
                sha256: required("sha256")?,
                worktree_bytes: number("worktree_bytes")?,
            });
        }

        if entries.is_empty() {
            return Err(Fail::environment(format!(
                "{}: no repositories in the manifest",
                path.display()
            )));
        }

        Ok(Manifest { host, entries })
    }
}

fn block_map(
    path: &Path,
    label: &str,
    block: Vec<(usize, String, String)>,
) -> Result<BTreeMap<String, String>, Fail> {
    let mut map = BTreeMap::new();
    for (line, key, value) in block {
        if map.insert(key.clone(), value).is_some() {
            return Err(Fail::environment(format!(
                "{}:{line}: duplicate key `{key}` in {label}",
                path.display()
            )));
        }
    }
    Ok(map)
}

fn missing(path: &Path, key: &str) -> Fail {
    Fail::environment(format!("{}: missing required key `{key}`", path.display()))
}

/// Lowercase SHA-256 of a file, read in chunks so a large bundle is never held in memory.
pub fn sha256_file(path: &Path) -> Result<String, Fail> {
    let mut file = fs::File::open(path)
        .map_err(|error| Fail::environment(format!("cannot read {}: {error}", path.display())))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            Fail::environment(format!("cannot read {}: {error}", path.display()))
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn example() -> Manifest {
        Manifest {
            host: "defrag-air".to_string(),
            entries: vec![Entry {
                repo: "defrag/shared-crates".to_string(),
                reference: "refs/heads/main".to_string(),
                commit: "0123456789012345678901234567890123456789".to_string(),
                bundle: "shared-crates.bundle".to_string(),
                bytes: 37_748_736,
                sha256: "a".repeat(64),
                worktree_bytes: 52_428_800,
            }],
        }
    }

    fn temp_root(label: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "fleet-sync-manifest-{label}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("temp root");
        root
    }

    #[test]
    fn a_manifest_round_trips() {
        let root = temp_root("round-trip");
        let manifest = example();
        manifest.write(&root).expect("write");
        assert_eq!(Manifest::read(&root).expect("read"), manifest);
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_duplicate_key_is_an_error() {
        let root = temp_root("duplicate");
        let raw = "version: 1\nhost: air\nhost: other\n";
        let error = Manifest::parse(&root.join(FILE), raw).expect_err("duplicate");
        assert!(error.message.contains("duplicate key `host`"));
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_missing_host_is_an_error() {
        let root = temp_root("missing");
        let error = Manifest::parse(&root.join(FILE), "version: 1\n").expect_err("missing host");
        assert!(error.message.contains("missing required key `host`"));
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_future_version_is_an_error() {
        let root = temp_root("version");
        let error =
            Manifest::parse(&root.join(FILE), "version: 2\nhost: air\n").expect_err("version");
        assert!(error.message.contains("manifest version 2"));
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn an_empty_manifest_is_an_error() {
        let root = temp_root("empty");
        let error =
            Manifest::parse(&root.join(FILE), "version: 1\nhost: air\n").expect_err("no repos");
        assert!(error.message.contains("no repositories"));
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_known_digest_matches() {
        let root = temp_root("sha");
        let path = root.join("abc.txt");
        fs::write(&path, "abc").expect("write");
        assert_eq!(
            sha256_file(&path).expect("hash"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        fs::remove_dir_all(&root).expect("cleanup");
    }
}
