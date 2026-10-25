//! Opening a storage root and finding an object in it.
//!
//! Object lookup walks the storage root for `0=ocfl_object_1.1` rather than requiring an
//! `ocfl_layout.json`. That is enough for a `path:` store and for a git checkout, which is every
//! store this tool reads today; a static HTTP store would need a declared layout, and that is
//! deliberately not implemented until there is one to read.

use std::fs;
use std::path::{Path, PathBuf};

use crate::fsx::expand_tilde;
use crate::git::Cache;
use crate::inventory::{discover, Inventory, Object};
use crate::source::{Locator, Source};

pub const DECLARATION: &str = "0=ocfl_1.1";
pub const STORE_DIRECTORY: &str = ".skill-store";

pub struct Store {
    pub root: PathBuf,
    pub rev: Option<String>,
    pub label: String,
}

impl Store {
    pub fn open(source: &Source, offline: bool) -> Result<Self, String> {
        match &source.locator {
            Locator::Path(path) => {
                let root = resolve_root(path, None)?;
                Ok(Store {
                    root,
                    rev: None,
                    label: source.store_label(),
                })
            }
            Locator::Git { url, rev, dir, .. } => {
                // Fetching is the only operation here that touches the network. `list`,
                // `verify` and `export` still read the cache when `--offline` is given.
                let (checkout, resolved) =
                    Cache::discover()?.fetch(url, source_ref(source), rev.as_deref(), offline)?;
                let root = resolve_root(&checkout, dir.as_deref())?;
                Ok(Store {
                    root,
                    rev: Some(resolved),
                    label: source.store_label(),
                })
            }
        }
    }

    pub fn object(&self, skill: &str) -> Result<Object, String> {
        let wanted = format!("urn:skill:{skill}");
        let objects = discover(&self.root)?;
        let matches: Vec<&Object> = objects
            .iter()
            .filter(|object| {
                object.inventory.id == wanted
                    || object
                        .root
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name == skill)
            })
            .collect();
        match matches.as_slice() {
            [object] => Ok(Object {
                root: object.root.clone(),
                inventory: object.inventory.clone(),
            }),
            [] => {
                let available: Vec<String> = objects
                    .iter()
                    .map(|object| format!("{} ({})", object.inventory.id, object.root.display()))
                    .collect();
                Err(if available.is_empty() {
                    format!(
                        "{} is an OCFL storage root but contains no objects",
                        self.root.display()
                    )
                } else {
                    format!(
                        "no object for `{skill}` in {} — available: {}",
                        self.root.display(),
                        available.join(", ")
                    )
                })
            }
            _ => {
                let roots: Vec<String> = matches
                    .iter()
                    .map(|object| object.root.display().to_string())
                    .collect();
                Err(format!(
                    "`{skill}` is ambiguous — matched {}",
                    roots.join(", ")
                ))
            }
        }
    }

    pub fn objects(&self) -> Result<Vec<Object>, String> {
        discover(&self.root)
    }
}

fn source_ref(source: &Source) -> Option<&str> {
    match &source.locator {
        Locator::Git { ref_, .. } => ref_.as_deref(),
        Locator::Path(_) => None,
    }
}

/// The storage root for a checkout or directory.
///
/// An explicit `dir` is used as given. Otherwise the directory itself, then `.skill-store/`,
/// which is the convention that lets a tool repository carry its skill beside its code:
///
/// ```text
/// container-compose/
/// ├── crates/
/// ├── .skill-store/
/// │   ├── 0=ocfl_1.1
/// │   └── container-compose/…
/// ```
/// The storage root at or immediately inside `base`, if there is one.
///
/// The inverse of [`resolve_root`] for the caller that has to decide whether a path *is* a store
/// before trying to open one — a development override points at a plain skill directory instead.
pub fn find_root(base: &Path) -> Option<PathBuf> {
    if is_root(base) {
        return Some(base.to_path_buf());
    }
    let nested = base.join(STORE_DIRECTORY);
    is_root(&nested).then_some(nested)
}

pub fn resolve_root(base: &Path, dir: Option<&str>) -> Result<PathBuf, String> {
    if let Some(dir) = dir {
        let candidate = base.join(dir);
        return validate_root(&candidate);
    }
    match find_root(base) {
        Some(root) => validate_root(&root),
        None => Err(format!(
            "{} is not an OCFL storage root, and neither is {} — expected `{DECLARATION}` in one \
             of them, or pass `?dir=`",
            base.display(),
            base.join(STORE_DIRECTORY).display()
        )),
    }
}

fn is_root(path: &Path) -> bool {
    path.join(DECLARATION).is_file()
}

pub fn validate_root(path: &Path) -> Result<PathBuf, String> {
    let declaration = path.join(DECLARATION);
    let body = fs::read_to_string(&declaration).map_err(|e| {
        format!(
            "{}: not an OCFL storage root ({e}); expected `{DECLARATION}`",
            path.display()
        )
    })?;
    if body.trim() != "ocfl_1.1" {
        return Err(format!(
            "{}: `{DECLARATION}` declares `{}` — this tool writes OCFL 1.1",
            path.display(),
            body.trim()
        ));
    }
    Ok(path.to_path_buf())
}

/// The store root a `path:` source names, with `~` expanded.
pub fn open_path(path: &str, dir: Option<&str>) -> Result<PathBuf, String> {
    resolve_root(&expand_tilde(path), dir)
}

pub fn read_manifest(object_root: &Path) -> Result<Inventory, String> {
    Inventory::read_verified(object_root)
}
