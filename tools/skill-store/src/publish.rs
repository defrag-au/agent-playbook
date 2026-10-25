//! Publishing a plain skill directory as a new OCFL version.
//!
//! The source is deliberately plain: `SKILL.md` plus whatever it references. A version is a
//! release, not a keystroke, so the working copy stays diffable and the store stays immutable.
//! Content is deduplicated by digest, so a version that changes one sentence of `SKILL.md`
//! stores one file.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, Utc};

use crate::digest::{self, Algorithm};
use crate::fsx::walk_files;
use crate::inventory::{
    self, Inventory, User, Version, DECLARATION as OBJECT_DECLARATION, TYPE_1_1,
};
use crate::skill;
use crate::store::{validate_root, DECLARATION as STORE_DECLARATION};

#[derive(Default)]
pub struct Options<'a> {
    pub id: Option<&'a str>,
    pub message: Option<&'a str>,
    pub user: Option<&'a str>,
    pub init: bool,
}

#[derive(Debug)]
pub struct Report {
    pub id: String,
    pub version: String,
    pub added: usize,
    pub changed: usize,
    pub removed: usize,
    pub unchanged: usize,
}

pub fn publish(from: &Path, store: &Path, options: &Options<'_>) -> Result<Report, String> {
    let from: PathBuf = fs::canonicalize(from).map_err(|e| format!("{}: {e}", from.display()))?;
    let meta = skill::read(&from)?;
    let id = options
        .id
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("urn:skill:{}", meta.name));
    if id.trim().is_empty() {
        return Err("`--id` cannot be empty".to_string());
    }

    ensure_store(store, options.init)?;

    let existing = inventory::discover(store)?
        .into_iter()
        .find(|object| object.inventory.id == id);
    let (object_root, mut inventory, old_state) = match existing {
        Some(object) => {
            let inventory = Inventory::read_verified(&object.root)?;
            let errors = inventory.validate();
            if !errors.is_empty() {
                return Err(format!(
                    "{}: the existing object is not valid:\n  {}",
                    object.root.display(),
                    errors.join("\n  ")
                ));
            }
            let state = inventory.state_for(&inventory.head)?;
            (object.root, inventory, state)
        }
        None => {
            let root = store.join(&meta.name);
            if root.exists() {
                return Err(format!(
                    "{} exists but is not an OCFL object for `{id}` — move it or publish under \
                     another name",
                    root.display()
                ));
            }
            let inventory = Inventory {
                id,
                type_: TYPE_1_1.to_string(),
                digest_algorithm: Algorithm::Sha512.name().to_string(),
                head: String::new(),
                content_directory: None,
                manifest: BTreeMap::new(),
                versions: BTreeMap::new(),
                fixity: None,
            };
            (root, inventory, BTreeMap::new())
        }
    };

    let mut contents = Vec::new();
    for (relative, path) in walk_files(&from)? {
        let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let digest = digest::bytes_hex(&bytes, Algorithm::Sha512);
        contents.push((relative, digest, bytes));
    }
    if !contents
        .iter()
        .any(|(relative, _, _)| relative == "SKILL.md")
    {
        return Err(format!("{}: a skill needs a SKILL.md", from.display()));
    }

    let mut new_state: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (relative, digest, _) in &contents {
        new_state
            .entry(digest.clone())
            .or_default()
            .push(relative.clone());
    }
    for paths in new_state.values_mut() {
        paths.sort();
    }

    let old_by_digest = invert(&old_state);
    if old_by_digest == new_state {
        return Err(format!(
            "{}: content is identical to {}; there is nothing to publish",
            from.display(),
            if inventory.head.is_empty() {
                "an empty object"
            } else {
                inventory.head.as_str()
            }
        ));
    }

    let mut report = Report {
        id: inventory.id.clone(),
        version: if inventory.head.is_empty() {
            "v1".to_string()
        } else {
            inventory.next_version()?
        },
        added: 0,
        changed: 0,
        removed: 0,
        unchanged: 0,
    };
    for (relative, digest) in state_pairs(&new_state) {
        match old_state.get(&relative) {
            None => report.added += 1,
            Some(previous) if previous == &digest => report.unchanged += 1,
            Some(_) => report.changed += 1,
        }
    }
    report.removed = {
        let new_paths: BTreeSet<&String> = new_state.values().flatten().collect();
        old_state
            .keys()
            .filter(|logical| !new_paths.contains(*logical))
            .count()
    };

    let version_dir = object_root.join(&report.version);
    if inventory.head.is_empty() {
        fs::create_dir_all(&object_root).map_err(|e| format!("{}: {e}", object_root.display()))?;
        fs::write(object_root.join(OBJECT_DECLARATION), "ocfl_object_1.1\n")
            .map_err(|e| format!("{}: {e}", object_root.display()))?;
    }
    fs::create_dir_all(&version_dir).map_err(|e| format!("{}: {e}", version_dir.display()))?;

    let content_dir = inventory.content_dir().to_string();
    for (relative, digest, bytes) in &contents {
        if inventory.manifest.contains_key(digest) {
            continue;
        }
        let content_path = format!("{}/{content_dir}/{relative}", report.version);
        let path = object_root.join(&content_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        inventory
            .manifest
            .insert(digest.clone(), vec![content_path]);
    }

    let user = options
        .user
        .map(ToString::to_string)
        .or_else(|| std::env::var("USER").ok())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "skill-store".to_string());
    inventory.versions.insert(
        report.version.clone(),
        Version {
            created: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            state: new_state,
            message: options.message.map(ToString::to_string),
            user: Some(User {
                name: user,
                address: None,
            }),
        },
    );
    inventory.head = report.version.clone();

    // The version directory gets its snapshot first; the object root's inventory is the last
    // thing written, so a half-finished publish is visible as a missing head rather than as a
    // version that looks complete.
    inventory::write(&version_dir, &inventory)?;
    inventory::write(&object_root, &inventory)?;
    Ok(report)
}

fn ensure_store(store: &Path, init: bool) -> Result<(), String> {
    if store.join(STORE_DECLARATION).is_file() {
        return validate_root(store).map(|_| ());
    }
    if !init {
        return Err(format!(
            "{} is not an OCFL storage root — pass `--init` to create one",
            store.display()
        ));
    }
    if store.exists() {
        let mut entries = fs::read_dir(store).map_err(|e| format!("{}: {e}", store.display()))?;
        if entries.next().is_some() {
            return Err(format!(
                "{} is not empty and has no `{STORE_DECLARATION}` — refusing to adopt it; use a \
                 new or empty directory",
                store.display()
            ));
        }
    }
    fs::create_dir_all(store).map_err(|e| format!("{}: {e}", store.display()))?;
    fs::write(store.join(STORE_DECLARATION), "ocfl_1.1\n")
        .map_err(|e| format!("{}: {e}", store.display()))
}

fn invert(state: &BTreeMap<String, String>) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (logical, digest) in state {
        out.entry(digest.clone()).or_default().push(logical.clone());
    }
    for paths in out.values_mut() {
        paths.sort();
    }
    out
}

fn state_pairs(state: &BTreeMap<String, Vec<String>>) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for (digest, paths) in state {
        for path in paths {
            pairs.push((path.clone(), digest.clone()));
        }
    }
    pairs.sort();
    pairs
}
