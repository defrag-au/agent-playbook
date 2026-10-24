//! The cargo home: which crates are extracted, at which versions, from which registry.
//!
//! A read of directory names only. Nothing here opens a `.crate` archive, runs `cargo`, or
//! writes anything — an uninstalled crate is reported as absent rather than fetched. The
//! registry directory name (`index.crates.io-1949cf8c6b5b557f`) is kept because it is the only
//! thing that distinguishes two sources of the same name and version.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use at_core::contract::{plural_of, Fail};

use crate::walk::read_dir_sorted;

/// One extracted crate source directory.
#[derive(Clone, Debug)]
pub struct Installed {
    pub name: String,
    pub version: String,
    pub registry: String,
    pub dir: PathBuf,
}

impl Installed {
    /// The form the headers and rows use.
    pub fn label(&self) -> String {
        format!("{} {}", self.name, self.version)
    }
}

/// Every extracted copy, plus what the scan could not read. A read-only tool that silently
/// skipped a registry directory would make "not installed" a claim it cannot support.
#[derive(Default)]
pub struct Listing {
    pub entries: Vec<Installed>,
    pub unreadable: usize,
    pub symlinks: usize,
}

pub struct Store {
    home: PathBuf,
}

impl Store {
    /// `--cargo-home`, then `CARGO_HOME`, then `~/.cargo`. The result is canonicalised, so the
    /// containment checks that use it compare real paths.
    pub fn resolve(explicit: Option<&Path>) -> Result<Store, Fail> {
        let home = match explicit {
            Some(path) => path.to_path_buf(),
            None => match env::var_os("CARGO_HOME") {
                Some(value) if !value.is_empty() => PathBuf::from(value),
                _ => {
                    let home = home_dir().ok_or_else(|| {
                        Fail::environment(
                            "cannot find the cargo home: --cargo-home, CARGO_HOME and HOME are all \
                             unset",
                        )
                    })?;
                    home.join(".cargo")
                }
            },
        };
        let home = fs::canonicalize(&home).map_err(|e| {
            Fail::environment(format!(
                "cannot read the cargo home {}: {e} · --cargo-home or CARGO_HOME can name one",
                home.display()
            ))
        })?;
        if !home.is_dir() {
            return Err(Fail::environment(format!(
                "the cargo home {} is not a directory",
                home.display()
            )));
        }
        Ok(Store { home })
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    /// Git checkouts cargo has extracted for a source URL at a full revision. The checkout
    /// directory is `<repo>-<url hash>/<short rev>`, so a unique short-rev match is the address;
    /// two matches are reported rather than chosen between.
    pub fn git_checkout_candidates(&self, url: &str, rev: Option<&str>) -> Vec<PathBuf> {
        let Some(rev) = rev.filter(|rev| rev.len() >= 7) else {
            return Vec::new();
        };
        let repo = url
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(url)
            .trim_end_matches(".git");
        let short = &rev[..7];
        let checkouts = self.home.join("git").join("checkouts");
        let mut hits = std::collections::BTreeSet::new();
        for checkout in read_dir_sorted(&checkouts).unwrap_or_default() {
            let Some(name) = checkout
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
            else {
                continue;
            };
            if !name.starts_with(&format!("{repo}-")) {
                continue;
            }
            for revision in read_dir_sorted(&checkout).unwrap_or_default() {
                let Some(name) = revision
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                else {
                    continue;
                };
                if name.starts_with(short) && revision.is_dir() {
                    hits.insert(revision);
                }
            }
        }
        hits.into_iter().collect()
    }

    /// Every `<crate>-<version>` directory under `registry/src/<registry>/`, sorted by name and
    /// then newest version first.
    pub fn installed(&self) -> Result<Listing, Fail> {
        let root = self.home.join("registry").join("src");
        let registries = match read_dir_sorted(&root) {
            Ok(paths) => paths,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Listing::default())
            }
            Err(error) => {
                return Err(Fail::environment(format!(
                    "cannot read {}: {error}",
                    root.display()
                )))
            }
        };

        let mut listing = Listing::default();
        for registry in registries {
            let Some(registry_name) = registry
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
            else {
                continue;
            };
            let Ok(entries) = read_dir_sorted(&registry) else {
                listing.unreadable += 1;
                continue;
            };
            for entry in entries {
                let Ok(meta) = fs::symlink_metadata(&entry) else {
                    listing.unreadable += 1;
                    continue;
                };
                if meta.file_type().is_symlink() {
                    listing.symlinks += 1;
                    continue;
                }
                if !meta.is_dir() {
                    continue;
                }
                let Some(dir_name) = entry
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                else {
                    continue;
                };
                let Some((name, version)) = split_crate_dir(&dir_name) else {
                    continue;
                };
                listing.entries.push(Installed {
                    name,
                    version,
                    registry: registry_name.clone(),
                    dir: entry,
                });
            }
        }

        listing.entries.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then_with(|| version_order(&b.version, &a.version))
                .then_with(|| a.registry.cmp(&b.registry))
        });
        Ok(listing)
    }

    /// The one installed copy a query names, or a refusal that says which copies exist.
    ///
    /// Exact names win over the hyphen/underscore spelling, because `serde_json` and a
    /// hypothetical `serde-json` are different crates. Only when there is no exact match does the
    /// other spelling count, and two candidates under it are an error rather than a choice.
    pub fn find(
        &self,
        listing: &Listing,
        query: &str,
        version: Option<&str>,
        registry: Option<&str>,
    ) -> Result<Installed, Fail> {
        let mut matches: Vec<&Installed> = listing
            .entries
            .iter()
            .filter(|entry| entry.name == query)
            .collect();

        if matches.is_empty() {
            matches = listing
                .entries
                .iter()
                .filter(|entry| normalised(&entry.name) == normalised(query))
                .collect();
        }

        if let Some(version) = version {
            matches.retain(|entry| entry.version == version);
        }
        if let Some(registry) = registry {
            matches.retain(|entry| entry.registry == registry);
        }

        match matches.as_slice() {
            [one] => Ok((*one).clone()),
            [] => {
                let versions = versions_of(listing, query, registry);
                let looked = match version {
                    Some(version) => format!("{query} {version}"),
                    None => query.to_string(),
                };
                let suffix = if versions.is_empty() {
                    format!(
                        "no extracted copy is named {looked} under {} · the crate is not installed, \
                         and this tool never fetches one",
                        self.home.join("registry/src").display()
                    )
                } else {
                    format!(
                        "no extracted copy is named {looked} · installed: {versions} · run a build \
                         (or cargo fetch) so cargo extracts it"
                    )
                };
                Err(Fail::environment(suffix))
            }
            many => {
                let mut lines = Vec::new();
                for entry in many {
                    lines.push(format!(
                        "{} {} · {} · {}",
                        entry.name,
                        entry.version,
                        entry.registry,
                        entry.dir.display()
                    ));
                }
                let hint = if version.is_some() {
                    "--registry <dir>"
                } else {
                    "--version <ver>"
                };
                Err(Fail::usage(format!(
                    "{query} is ambiguous: {} extracted copies match · {hint} picks one\n  {}",
                    many.len(),
                    lines.join("\n  ")
                )))
            }
        }
    }
}

/// The versions of a query the store does hold, for an error that would otherwise say only
/// "no". Registry filtering is applied because an error about the wrong registry is noise.
fn versions_of(listing: &Listing, query: &str, registry: Option<&str>) -> String {
    let mut versions: Vec<String> = listing
        .entries
        .iter()
        .filter(|entry| entry.name == query || normalised(&entry.name) == normalised(query))
        .filter(|entry| registry.is_none_or(|registry| entry.registry == registry))
        .map(|entry| entry.version.clone())
        .collect();
    versions.sort_by(|a, b| version_order(a, b));
    versions.dedup();
    if versions.is_empty() {
        String::new()
    } else {
        versions.join(", ")
    }
}

/// `<crate>-<version>`, where the version is a semver, not merely "the part after the last
/// hyphen": a pre-release version contains hyphens of its own (`foo-1.0.0-alpha`), and a crate
/// name may end in a digit (`sha2-0.10.8`). The first hyphen whose remainder is version-shaped
/// is the split.
pub fn split_crate_dir(dir: &str) -> Option<(String, String)> {
    for (index, _) in dir.match_indices('-') {
        let name = &dir[..index];
        let version = &dir[index + 1..];
        if !name.is_empty() && version_shaped(version) {
            return Some((name.to_string(), version.to_string()));
        }
    }
    None
}

fn version_shaped(text: &str) -> bool {
    let core = text.split(['-', '+']).next().unwrap_or(text);
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

/// Semver's numeric core, newest last. A pre-release sorts before its release, which is the one
/// ordering rule a reader would notice if it were wrong.
fn version_order(a: &str, b: &str) -> std::cmp::Ordering {
    fn key(version: &str) -> (u64, u64, u64, u8) {
        let core = version.split(['-', '+']).next().unwrap_or(version);
        let mut parts = core.split('.').map(|part| part.parse::<u64>().unwrap_or(0));
        (
            parts.next().unwrap_or(0),
            parts.next().unwrap_or(0),
            parts.next().unwrap_or(0),
            if version.contains('-') { 0 } else { 1 },
        )
    }
    key(a).cmp(&key(b))
}

pub fn normalised(name: &str) -> String {
    name.replace('_', "-")
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .or_else(|| env::var_os("USERPROFILE").filter(|value| !value.is_empty()))
        .map(PathBuf::from)
}

/// The one line an `installed` row is: name, version, registry and path.
pub fn installed_row(entry: &Installed) -> String {
    format!(
        "{} {} · {} · {}",
        entry.name,
        entry.version,
        entry.registry,
        entry.dir.display()
    )
}

/// A count with the right plural, for the listing bounds.
pub fn listing_bound(listing: &Listing, total: usize, shown: usize, limit: usize) -> String {
    let mut text = format!(
        "{} of {}",
        shown,
        plural_of(total, "extracted copy", "extracted copies")
    );
    if shown < total {
        text.push_str(&format!(" · --limit {limit}"));
    }
    if listing.unreadable > 0 {
        text.push_str(&format!(
            " · {} not counted",
            plural_of(
                listing.unreadable,
                "unreadable registry directory",
                "unreadable registry directories"
            )
        ));
    }
    if listing.symlinks > 0 {
        text.push_str(&format!(
            " · {} symbolic link(s) not followed",
            listing.symlinks
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_crate_and_version_split() {
        assert_eq!(
            split_crate_dir("headless_chrome-1.0.22"),
            Some(("headless_chrome".into(), "1.0.22".into()))
        );
    }

    #[test]
    fn a_hyphenated_name_splits_at_the_version() {
        assert_eq!(
            split_crate_dir("wasm-bindgen-0.2.100"),
            Some(("wasm-bindgen".into(), "0.2.100".into()))
        );
    }

    #[test]
    fn a_prerelease_version_keeps_its_hyphen() {
        assert_eq!(
            split_crate_dir("foo-1.0.0-alpha.1"),
            Some(("foo".into(), "1.0.0-alpha.1".into()))
        );
    }

    #[test]
    fn a_numeric_name_is_not_mistaken_for_a_version() {
        assert_eq!(
            split_crate_dir("foo-2-1.0.0"),
            Some(("foo-2".into(), "1.0.0".into()))
        );
    }

    #[test]
    fn a_directory_without_a_semver_is_not_a_crate() {
        assert_eq!(split_crate_dir("src"), None);
        assert_eq!(split_crate_dir("foo-bar"), None);
    }

    #[test]
    fn versions_order_numerically() {
        assert_eq!(version_order("0.9.0", "0.10.0"), std::cmp::Ordering::Less);
        assert_eq!(
            version_order("1.0.0-alpha", "1.0.0"),
            std::cmp::Ordering::Less
        );
    }
}
