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
use crate::identity;
use crate::inventory::{
    self, Inventory, User, Version, DECLARATION as OBJECT_DECLARATION, TYPE_1_1,
};
use crate::skill;
use crate::status::{self, State};
use crate::store::ensure_root;

#[derive(Default)]
pub struct Options<'a> {
    pub id: Option<&'a str>,
    pub message: Option<&'a str>,
    pub user: Option<&'a str>,
    pub address: Option<&'a str>,
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

    ensure_root(store, options.init)?;

    // The `user` block is resolved here, before anything is written: a publish that cannot record
    // the identity it was asked for must fail having written nothing, rather than leave a version
    // directory with no inventory beside it.
    //
    // OCFL wants a version's `user` block to be present and, when it is, to carry an address
    // (W007b, W008, W009). The identity is read from the repository the store lives in, so nothing
    // is invented — and a store outside a git checkout records no block at all, rather than a name
    // with no address to go with it.
    let discovered = identity::discover(store);
    let address = options
        .address
        .map(ToString::to_string)
        .or_else(|| discovered.as_ref().map(|id| id.address.clone()));
    // A name with no address is exactly W008, so a caller who asked for a user block and gave
    // nothing to put in its address half is told, rather than having the flag quietly dropped.
    if options.user.is_some() && address.is_none() {
        return Err(
            "`--user` records a user block, and OCFL wants one to carry an address — pass \
             `--address`, or publish from a repository with an `origin`"
                .to_string(),
        );
    }
    let user = address.map(|address| User {
        name: options
            .user
            .map(ToString::to_string)
            .or_else(|| discovered.as_ref().map(|id| id.name.clone()))
            .or_else(|| std::env::var("USER").ok())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "skill-store".to_string()),
        address: Some(address),
    });

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

    inventory.versions.insert(
        report.version.clone(),
        Version {
            created: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            state: new_state,
            message: options.message.map(ToString::to_string),
            user,
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

/// The result of publishing everything that has moved on.
#[derive(Debug, Default)]
pub struct Changed {
    /// One report per skill that was published.
    pub published: Vec<Report>,
    /// Skills whose object has no authoring directory. Publishing cannot fix those: there is
    /// nothing to publish from.
    pub blocked: Vec<String>,
}

/// Publish every skill in `worktree` that is `ahead` of its object or has no object yet.
///
/// This is the whole loop as one command: a version is a release, so the authoring directory and
/// the store are compared first and only the skills that differ are published. Running it when
/// nothing has changed publishes nothing and succeeds, which is what makes it safe as a routine
/// step before a commit.
///
/// An object with no directory is not an error here, because it is not something this command can
/// repair — it is reported in [`Changed::blocked`] so the caller can fail on it with the reason.
pub fn publish_changed(
    store: &Path,
    worktree: &Path,
    options: &Options<'_>,
) -> Result<Changed, String> {
    if options.id.is_some() {
        return Err(
            "`publish-changed` publishes each skill under its own name — drop `--id`".to_string(),
        );
    }
    ensure_root(store, options.init)?;

    let per_skill = Options {
        id: None,
        message: options.message,
        user: options.user,
        address: options.address,
        init: false,
    };

    let mut changed = Changed::default();
    for reading in status::compare(store, worktree)? {
        match reading.state {
            State::Current => {}
            State::Ahead(_) | State::Missing => {
                let report = publish(&worktree.join(&reading.name), store, &per_skill)?;
                changed.published.push(report);
            }
            State::StoreOnly => changed.blocked.push(reading.name),
        }
    }
    Ok(changed)
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
