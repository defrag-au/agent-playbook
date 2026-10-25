//! Comparing a storage root with the directories it is published from.
//!
//! Publishing is a release, so nothing here writes: it answers one question per skill — does the
//! authoring directory still match the object's head? That is the check a repository runs before
//! merging, and the reason it exists is that a version is immutable, so an edit that was never
//! published lives only on the machine that made it.
//!
//! Both directions are reported. A directory with no object is a skill nobody can consume; an
//! object with no directory is a skill nobody can edit.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::fsx::walk_files;
use crate::inventory::{discover, Inventory, Object};

/// What differ between the working copy and the published version.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Difference {
    /// A file that exists in both, with different bytes.
    pub changed: usize,
    /// Only in the working copy — an addition that was never published.
    pub added: usize,
    /// Only in the published version — a deletion that was never published.
    pub removed: usize,
}

impl Difference {
    pub fn is_empty(&self) -> bool {
        self.changed == 0 && self.added == 0 && self.removed == 0
    }

    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if self.changed > 0 {
            parts.push(format!("{} changed", self.changed));
        }
        if self.added > 0 {
            parts.push(format!("{} added", self.added));
        }
        if self.removed > 0 {
            parts.push(format!("{} removed", self.removed));
        }
        if parts.is_empty() {
            "no differences".to_string()
        } else {
            parts.join(", ")
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// The directory is byte-identical to the object's head.
    Current,
    /// The directory has moved on. A publish is owed.
    Ahead(Difference),
    /// A directory with no object: nobody can consume it.
    Missing,
    /// An object with no directory: nobody can edit it.
    StoreOnly,
}

impl State {
    pub fn as_str(&self) -> &'static str {
        match self {
            State::Current => "current",
            State::Ahead(_) => "ahead",
            State::Missing => "missing",
            State::StoreOnly => "store-only",
        }
    }

    /// A state no repository should merge with.
    pub fn is_drift(&self) -> bool {
        !matches!(self, State::Current)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    pub name: String,
    /// The object's head, or `-` when there is no object.
    pub head: String,
    pub state: State,
}

/// Compare every object in `store` with `<worktree>/<name>`, and every directory in `worktree`
/// with the object of the same name.
pub fn compare(store: &Path, worktree: &Path) -> Result<Vec<Reading>, String> {
    let store = crate::store::validate_root(store)?;
    if !worktree.is_dir() {
        return Err(format!(
            "{}: no such directory — this is where a skill's authoring directory lives, one \
             subdirectory per name",
            worktree.display()
        ));
    }

    let objects = discover(&store)?;
    let mut readings: Vec<Reading> = Vec::new();
    let mut named: BTreeSet<String> = BTreeSet::new();

    for object in &objects {
        let Some(name) = object
            .root
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
        else {
            return Err(format!(
                "{}: the object directory has no usable name",
                object.root.display()
            ));
        };
        named.insert(name.clone());
        let directory = worktree.join(&name);
        let state = if directory.is_dir() {
            let difference = diff(object, &directory)?;
            if difference.is_empty() {
                State::Current
            } else {
                State::Ahead(difference)
            }
        } else {
            State::StoreOnly
        };
        readings.push(Reading {
            name,
            head: object.inventory.head.clone(),
            state,
        });
    }

    for directory in subdirectories(worktree)? {
        if named.contains(&directory) {
            continue;
        }
        readings.push(Reading {
            name: directory,
            head: "-".to_string(),
            state: State::Missing,
        });
    }

    readings.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(readings)
}

/// Directories under `worktree`, sorted. A file at the top level (`README.md`) is not a skill.
fn subdirectories(worktree: &Path) -> Result<Vec<String>, String> {
    let entries = fs::read_dir(worktree).map_err(|e| format!("{}: {e}", worktree.display()))?;
    let mut names = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        if !entry.path().is_dir() {
            continue;
        }
        if let Some(name) = entry.file_name().to_str() {
            names.push(name.to_string());
        }
    }
    names.sort();
    Ok(names)
}

/// Byte-compare the object's head against a directory.
fn diff(object: &Object, directory: &Path) -> Result<Difference, String> {
    let inventory = &object.inventory;
    let state = inventory.state_for(&inventory.head)?;
    let mut difference = Difference::default();
    let mut named: BTreeSet<String> = BTreeSet::new();

    for (logical, digest) in &state {
        named.insert(logical.clone());
        let published = content_path(object, inventory, digest)?;
        let authored = directory.join(logical);
        if !authored.is_file() {
            difference.removed += 1;
            continue;
        }
        let published =
            fs::read(&published).map_err(|e| format!("{}: {e}", published.display()))?;
        let authored = fs::read(&authored).map_err(|e| format!("{}: {e}", authored.display()))?;
        if published != authored {
            difference.changed += 1;
        }
    }

    for (relative, _) in walk_files(directory)? {
        if !named.contains(&relative) {
            difference.added += 1;
        }
    }

    Ok(difference)
}

fn content_path(object: &Object, inventory: &Inventory, digest: &str) -> Result<PathBuf, String> {
    let paths = inventory
        .manifest
        .get(digest)
        .ok_or_else(|| format!("{} is not in the manifest of {}", digest, inventory.id))?;
    let first = paths
        .first()
        .ok_or_else(|| format!("{} has no content path in {}", digest, inventory.id))?;
    Ok(object.root.join(first))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_difference_describes_itself() {
        assert_eq!(
            Difference {
                changed: 2,
                added: 1,
                removed: 0
            }
            .describe(),
            "2 changed, 1 added"
        );
        assert_eq!(Difference::default().describe(), "no differences");
    }

    #[test]
    fn only_current_is_not_drift() {
        assert!(!State::Current.is_drift());
        assert!(State::Ahead(Difference::default()).is_drift());
        assert!(State::Missing.is_drift());
        assert!(State::StoreOnly.is_drift());
    }
}
