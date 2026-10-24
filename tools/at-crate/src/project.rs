//! The project side of resolution: the nearest `Cargo.lock`, the package entry it holds, and
//! the target directory a build would have written to.
//!
//! The lock is parsed rather than handed to cargo, because the tool's whole promise is that it
//! runs nothing. Only `[[package]]` blocks are read, and only the three fields that identify a
//! source — a full TOML parser would be a dependency and a much larger claim than this needs.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use at_core::contract::Fail;

use crate::store::{normalised, Installed};

#[derive(Clone, Debug)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub source: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Lock {
    pub path: PathBuf,
    pub packages: Vec<Package>,
}

impl Lock {
    /// `Cargo.lock` in this directory, or `None` when there is not one. An existing but
    /// unreadable or empty lock is an error: falling back to installed copies would answer a
    /// different question than the one the caller's project asked.
    pub fn at(root: &Path) -> Result<Option<Lock>, Fail> {
        let path = root.join("Cargo.lock");
        if !path.exists() {
            return Ok(None);
        }
        let text = fs::read_to_string(&path).map_err(|error| {
            Fail::environment(format!("cannot read {}: {error}", path.display()))
        })?;
        let packages = parse(&text);
        if packages.is_empty() {
            return Err(Fail::environment(format!(
                "{} contains no [[package]] entries · not a lockfile this tool can read",
                path.display()
            )));
        }
        Ok(Some(Lock { path, packages }))
    }

    /// The nearest lock above `start`. Cargo workspaces keep one lock at the workspace root, so
    /// walking up is what finds the lock that actually chose the version.
    pub fn discover(start: &Path) -> Result<Option<Lock>, Fail> {
        let mut here = fs::canonicalize(start).map_err(|error| {
            Fail::environment(format!("cannot resolve {}: {error}", start.display()))
        })?;
        if here.is_file() {
            here.pop();
        }
        loop {
            if here.join("Cargo.lock").exists() {
                return Lock::at(&here);
            }
            if !here.pop() {
                return Ok(None);
            }
        }
    }

    /// Exact names first, then the hyphen/underscore spelling. Two spellings are different
    /// packages when both exist, so the fallback is only reached when there is no exact match.
    pub fn matching(&self, query: &str) -> Vec<&Package> {
        let exact: Vec<&Package> = self
            .packages
            .iter()
            .filter(|package| package.name == query)
            .collect();
        if !exact.is_empty() {
            return exact;
        }
        self.packages
            .iter()
            .filter(|package| normalised(&package.name) == normalised(query))
            .collect()
    }
}

/// The source a lock entry names, in the forms this tool can act on.
#[derive(Clone, Debug)]
pub enum SourceKind {
    Registry,
    Git { url: String, rev: Option<String> },
    Path,
    Unknown(String),
}

impl SourceKind {
    pub fn of(source: Option<&str>) -> SourceKind {
        let Some(source) = source else {
            return SourceKind::Path;
        };
        if source.starts_with("registry+") || source.starts_with("sparse+") {
            return SourceKind::Registry;
        }
        if let Some(rest) = source.strip_prefix("git+") {
            let (before_fragment, fragment) = match rest.split_once('#') {
                Some((head, tail)) => (head, Some(tail)),
                None => (rest, None),
            };
            let (url, query) = match before_fragment.split_once('?') {
                Some((head, tail)) => (head, Some(tail)),
                None => (before_fragment, None),
            };
            let rev = fragment.map(str::to_string).or_else(|| {
                query.and_then(|query| {
                    query
                        .split('&')
                        .find_map(|part| part.strip_prefix("rev="))
                        .map(str::to_string)
                })
            });
            return SourceKind::Git {
                url: url.to_string(),
                rev,
            };
        }
        SourceKind::Unknown(source.to_string())
    }
}

pub struct Project {
    pub root: Option<PathBuf>,
    pub lock: Option<Lock>,
    pub explicit: bool,
}

impl Project {
    /// The project the caller named, or the one the working directory is inside. Reads the lock
    /// eagerly, because every caller of this form is asking what version the project chose.
    pub fn discover(explicit: Option<&Path>, cwd: &Path) -> Result<Project, Fail> {
        if let Some(path) = explicit {
            let root = canonical_dir(path, "--project")?;
            let lock = Lock::at(&root)?;
            return Ok(Project {
                root: Some(root),
                lock,
                explicit: true,
            });
        }

        let lock = Lock::discover(cwd)?;
        let root = match &lock {
            Some(lock) => lock.path.parent().map(Path::to_path_buf),
            None => nearest_manifest(cwd),
        };
        Ok(Project {
            root,
            lock,
            explicit: false,
        })
    }

    /// The same discovery without the lock, for `out`: listing a target directory must not fail
    /// because a lock is empty or unreadable.
    pub fn root(explicit: Option<&Path>, cwd: &Path) -> Result<Project, Fail> {
        let (root, explicit) = match explicit {
            Some(path) => (Some(canonical_dir(path, "--project")?), true),
            None => (nearest_manifest(cwd), false),
        };
        Ok(Project {
            root,
            lock: None,
            explicit,
        })
    }

    /// `--target-dir`, then `CARGO_TARGET_DIR`, then `<project>/target`. The last form is
    /// returned whether or not it exists, so a verb can say "nothing has been built" rather than
    /// fail as though the project were unreadable.
    pub fn target_dir(&self, explicit: Option<&Path>, cwd: &Path) -> Result<Option<PathBuf>, Fail> {
        if let Some(path) = explicit {
            return canonical_dir(path, "--target-dir").map(Some);
        }
        if let Some(value) = env::var_os("CARGO_TARGET_DIR").filter(|value| !value.is_empty()) {
            let path = PathBuf::from(value);
            return Ok(Some(if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            }));
        }
        Ok(self.root.as_ref().map(|root| root.join("target")))
    }
}

/// The resolved crate, before a verb decides whether it needs a directory or just a version.
#[derive(Clone, Debug)]
pub struct Resolved {
    pub name: String,
    pub version: String,
    pub kind: SourceKind,
    /// Where the source is, when this tool can name it without guessing.
    pub dir: Option<PathBuf>,
    /// True when the lock named the version; false when the store's single copy did.
    pub from_lock: bool,
    /// Set when the query's spelling differs from the package's real name.
    pub spelling: Option<String>,
}

impl Resolved {
    pub fn label(&self) -> String {
        format!("{} {}", self.name, self.version)
    }

    pub fn from_installed(installed: &Installed, query: &str) -> Resolved {
        Resolved {
            name: installed.name.clone(),
            version: installed.version.clone(),
            kind: SourceKind::Registry,
            dir: Some(installed.dir.clone()),
            from_lock: false,
            spelling: spelling(query, &installed.name),
        }
    }
}

fn spelling(query: &str, name: &str) -> Option<String> {
    (query != name).then(|| name.to_string())
}

/// Parse `[[package]]` blocks. Multiline arrays and checksums are ignored; the fields read here
/// are single-line strings in every lock format cargo has written.
pub fn parse(text: &str) -> Vec<Package> {
    let mut packages = Vec::new();
    let mut current: Option<Package> = None;
    let mut in_package = false;

    for line in text.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            if let Some(package) = current.take() {
                packages.push(package);
            }
            current = Some(Package {
                name: String::new(),
                version: String::new(),
                source: None,
            });
            in_package = true;
            continue;
        }
        if line.starts_with("[[") {
            if let Some(package) = current.take() {
                packages.push(package);
            }
            in_package = false;
            continue;
        }
        if !in_package {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .unwrap_or(value);
        let Some(package) = current.as_mut() else {
            continue;
        };
        match key.trim() {
            "name" => package.name = value.to_string(),
            "version" => package.version = value.to_string(),
            "source" => package.source = Some(value.to_string()),
            _ => {}
        }
    }
    if let Some(package) = current {
        packages.push(package);
    }
    packages.retain(|package| !package.name.is_empty() && !package.version.is_empty());
    packages
}

fn canonical_dir(path: &Path, flag: &str) -> Result<PathBuf, Fail> {
    let resolved = fs::canonicalize(path).map_err(|error| {
        Fail::environment(format!("cannot read {flag} {}: {error}", path.display()))
    })?;
    if !resolved.is_dir() {
        return Err(Fail::environment(format!(
            "{flag} {} is not a directory",
            path.display()
        )));
    }
    Ok(resolved)
}

fn nearest_manifest(start: &Path) -> Option<PathBuf> {
    let mut here = fs::canonicalize(start).ok()?;
    if here.is_file() {
        here.pop();
    }
    loop {
        if here.join("Cargo.toml").exists() || here.join("Cargo.lock").exists() {
            return Some(here);
        }
        if !here.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCK: &str = r#"
version = 3

[[package]]
name = "headless_chrome"
version = "1.0.22"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "abc"

[[package]]
name = "workspace-member"
version = "0.1.0"
dependencies = [
 "headless_chrome",
]

[[patch.unused]]
name = "not-a-package"
version = "9.9.9"
"#;

    #[test]
    fn packages_are_parsed_and_patch_sections_are_not() {
        let packages = parse(LOCK);
        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0].name, "headless_chrome");
        assert_eq!(packages[0].version, "1.0.22");
        assert!(matches!(
            SourceKind::of(packages[0].source.as_deref()),
            SourceKind::Registry
        ));
        assert_eq!(packages[1].name, "workspace-member");
        assert!(matches!(
            SourceKind::of(packages[1].source.as_deref()),
            SourceKind::Path
        ));
    }

    #[test]
    fn a_git_source_keeps_the_url_and_full_rev() {
        let kind = SourceKind::of(Some(
            "git+https://github.com/defrag/shared-crates?rev=abcdef#abcdef0123456789",
        ));
        match kind {
            SourceKind::Git { url, rev } => {
                assert_eq!(url, "https://github.com/defrag/shared-crates");
                assert_eq!(rev.as_deref(), Some("abcdef0123456789"));
            }
            other => panic!("expected a git source, got {other:?}"),
        }
    }

    #[test]
    fn matching_prefers_an_exact_name() {
        let lock = Lock {
            path: PathBuf::from("Cargo.lock"),
            packages: vec![
                Package {
                    name: "serde_json".into(),
                    version: "1.0.0".into(),
                    source: None,
                },
                Package {
                    name: "serde-json".into(),
                    version: "1.0.0".into(),
                    source: None,
                },
            ],
        };
        assert_eq!(lock.matching("serde_json")[0].name, "serde_json");
    }
}
