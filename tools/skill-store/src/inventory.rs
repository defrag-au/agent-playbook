//! The OCFL inventory: the subset this store writes and the checks a consumer needs.
//!
//! Full OCFL validation is a larger job than a skill store requires. What is enforced here is
//! the part that makes an export trustworthy: the inventory digest sidecar, the version chain,
//! every `state` digest being in the `manifest`, every manifest content path hashing to its key,
//! and logical and content paths that cannot escape the object root. Fixity blocks and object
//! extensions are read and ignored.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::digest::{self, Algorithm};
use crate::fsx::safe_relative;

pub const TYPE_1_1: &str = "https://ocfl.io/1.1/spec/#inventory";
const TYPE_1_0: &str = "https://ocfl.io/1.0/spec/#inventory";
pub const DECLARATION: &str = "0=ocfl_object_1.1";

pub type Fixity = BTreeMap<String, BTreeMap<String, Vec<String>>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inventory {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(rename = "digestAlgorithm")]
    pub digest_algorithm: String,
    pub head: String,
    #[serde(
        rename = "contentDirectory",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub content_directory: Option<String>,
    pub manifest: BTreeMap<String, Vec<String>>,
    pub versions: BTreeMap<String, Version>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixity: Option<Fixity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Version {
    pub created: String,
    pub state: BTreeMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<User>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
}

impl Inventory {
    pub fn algorithm(&self) -> Result<Algorithm, String> {
        Algorithm::parse(&self.digest_algorithm)
    }

    pub fn content_dir(&self) -> &str {
        self.content_directory.as_deref().unwrap_or("content")
    }

    /// Read and normalise an inventory without verifying its sidecar.
    pub fn read(object_root: &Path) -> Result<Self, String> {
        let path = object_root.join("inventory.json");
        let source = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut inventory: Inventory = serde_json::from_str(&source)
            .map_err(|e| format!("{}: not an OCFL inventory: {e}", path.display()))?;
        inventory.normalise();
        Ok(inventory)
    }

    /// Read an inventory and verify `inventory.json.<algorithm>` against its bytes.
    pub fn read_verified(object_root: &Path) -> Result<Self, String> {
        let inventory = Self::read(object_root)?;
        let algorithm = inventory.algorithm()?;
        verify_sidecar(object_root, algorithm)?;
        Ok(inventory)
    }

    /// OCFL compares digests case-insensitively; normalise so lookups cannot miss on case.
    fn normalise(&mut self) {
        let manifest = std::mem::take(&mut self.manifest);
        self.manifest = manifest
            .into_iter()
            .map(|(digest, paths)| (digest.to_ascii_lowercase(), paths))
            .collect();
        for version in self.versions.values_mut() {
            let state = std::mem::take(&mut version.state);
            version.state = state
                .into_iter()
                .map(|(digest, paths)| (digest.to_ascii_lowercase(), paths))
                .collect();
        }
    }

    /// The logical path → digest map for one version. A logical path twice is ambiguous.
    pub fn state_for(&self, version: &str) -> Result<BTreeMap<String, String>, String> {
        let entry = self.versions.get(version).ok_or_else(|| {
            format!(
                "no version `{version}` — this object has {}",
                self.version_list()
            )
        })?;
        let mut out = BTreeMap::new();
        for (digest, paths) in &entry.state {
            for path in paths {
                if let Some(previous) = out.insert(path.clone(), digest.clone()) {
                    return Err(format!(
                        "version `{version}` maps `{path}` twice ({previous} and {digest})"
                    ));
                }
            }
        }
        Ok(out)
    }

    pub fn version_list(&self) -> String {
        if self.versions.is_empty() {
            return "no versions".to_string();
        }
        self.versions.keys().cloned().collect::<Vec<_>>().join(", ")
    }

    /// `v1`, `v2`, … in order, after checking the sequence is contiguous.
    pub fn ordered_versions(&self) -> Result<Vec<(u32, String)>, String> {
        let mut numbered = Vec::new();
        for name in self.versions.keys() {
            numbered.push((version_number(name)?, name.clone()));
        }
        numbered.sort_by_key(|(number, _)| *number);
        for (index, (number, name)) in numbered.iter().enumerate() {
            let expected = index as u32 + 1;
            if *number != expected {
                return Err(format!(
                    "version directories are not contiguous: expected v{expected}, found {name}"
                ));
            }
        }
        Ok(numbered)
    }

    pub fn head_number(&self) -> Result<u32, String> {
        let ordered = self.ordered_versions()?;
        let Some((number, name)) = ordered.last() else {
            return Err("inventory has no versions".to_string());
        };
        if name != &self.head {
            return Err(format!(
                "`head` is `{}` but the newest version is `{name}`",
                self.head
            ));
        }
        Ok(*number)
    }

    pub fn next_version(&self) -> Result<String, String> {
        Ok(format!("v{}", self.head_number()? + 1))
    }

    /// Structural checks that do not need to touch file contents.
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.type_ != TYPE_1_1 && self.type_ != TYPE_1_0 {
            errors.push(format!(
                "inventory `type` is `{}` — expected OCFL 1.0 or 1.1",
                self.type_
            ));
        }
        let algorithm = match Algorithm::parse(&self.digest_algorithm) {
            Ok(algorithm) => Some(algorithm),
            Err(error) => {
                errors.push(error);
                None
            }
        };
        if let Err(error) = self.ordered_versions() {
            errors.push(error);
        } else if let Err(error) = self.head_number() {
            errors.push(error);
        }
        if self.id.trim().is_empty() {
            errors.push("inventory `id` is empty".to_string());
        }
        if let Some(dir) = &self.content_directory {
            if dir.is_empty() || dir.contains('/') || dir == "." || dir == ".." {
                errors.push(format!(
                    "`contentDirectory` `{dir}` is not a direct child name"
                ));
            }
        }

        let mut content_paths = Vec::new();
        for (digest_value, paths) in &self.manifest {
            if let Some(algorithm) = algorithm {
                let wanted = match algorithm {
                    Algorithm::Sha256 => 64,
                    Algorithm::Sha512 => 128,
                };
                if digest_value.len() != wanted
                    || !digest_value.chars().all(|c| c.is_ascii_hexdigit())
                {
                    errors.push(format!("manifest key `{digest_value}` is not a hex digest"));
                }
            }
            if paths.is_empty() {
                errors.push(format!("manifest digest `{digest_value}` has no paths"));
            }
            for path in paths {
                if let Err(error) = safe_relative(path) {
                    errors.push(format!("manifest content path: {error}"));
                }
                content_paths.push(path.clone());
            }
        }
        check_paths(&mut errors, &content_paths, "manifest content path");

        for (name, version) in &self.versions {
            let mut logical = Vec::new();
            for (digest_value, paths) in &version.state {
                if !self.manifest.contains_key(digest_value) {
                    errors.push(format!(
                        "{name}: state digest `{digest_value}` is not in the manifest"
                    ));
                }
                for path in paths {
                    if let Err(error) = safe_relative(path) {
                        errors.push(format!("{name}: logical path: {error}"));
                    }
                    logical.push(path.clone());
                }
            }
            check_paths(&mut errors, &logical, &format!("{name} logical path"));
            if version.created.trim().is_empty() {
                errors.push(format!("{name}: `created` is empty"));
            }
        }
        errors
    }
}

fn check_paths(errors: &mut Vec<String>, paths: &[String], label: &str) {
    let unique: BTreeSet<&String> = paths.iter().collect();
    if unique.len() != paths.len() {
        errors.push(format!("a {label} is declared twice"));
    }
    let mut sorted: Vec<&String> = unique.into_iter().collect();
    sorted.sort();
    for pair in sorted.windows(2) {
        let (first, second) = (pair[0], pair[1]);
        if second.starts_with(&format!("{first}/")) {
            errors.push(format!(
                "{label} `{first}` is a directory prefix of `{second}`"
            ));
        }
    }
}

pub fn version_number(name: &str) -> Result<u32, String> {
    let Some(number) = name.strip_prefix('v') else {
        return Err(format!(
            "version directory `{name}` does not start with `v`"
        ));
    };
    if number.is_empty() || number.starts_with('0') {
        return Err(format!(
            "version directory `{name}` is not `v` followed by a positive number"
        ));
    }
    number
        .parse::<u32>()
        .map_err(|_| format!("version directory `{name}` is not a number"))
}

/// The digest sidecar for the inventory in `dir`, checked against the inventory's own bytes.
pub fn verify_sidecar(dir: &Path, algorithm: Algorithm) -> Result<(), String> {
    let inventory = dir.join("inventory.json");
    let actual = digest::file_hex(&inventory, algorithm)?;
    let sidecar = dir.join(format!("inventory.json.{}", algorithm.name()));
    let source = fs::read_to_string(&sidecar).map_err(|e| {
        format!(
            "{}: missing inventory digest sidecar ({e})",
            sidecar.display()
        )
    })?;
    let Some((declared, name)) = source.split_once(char::is_whitespace) else {
        return Err(format!(
            "{}: expected `<digest> inventory.json`, got `{}`",
            sidecar.display(),
            source.trim()
        ));
    };
    if name.trim() != "inventory.json" {
        return Err(format!(
            "{}: sidecar names `{}` rather than inventory.json",
            sidecar.display(),
            name.trim()
        ));
    }
    if !declared.trim().eq_ignore_ascii_case(&actual) {
        return Err(format!(
            "{}: inventory digest does not match the inventory — the inventory was changed after \
             it was written",
            sidecar.display()
        ));
    }
    Ok(())
}

/// Write `inventory.json` and its sidecar, computing the sidecar last.
pub fn write(dir: &Path, inventory: &Inventory) -> Result<(), String> {
    let algorithm = inventory.algorithm()?;
    let body = serde_json::to_string_pretty(inventory)
        .map_err(|e| format!("cannot serialise the inventory: {e}"))?;
    let body = format!("{body}\n");
    let path = dir.join("inventory.json");
    fs::write(&path, &body).map_err(|e| format!("{}: {e}", path.display()))?;
    let digest_value = digest::bytes_hex(body.as_bytes(), algorithm);
    let sidecar = dir.join(format!("inventory.json.{}", algorithm.name()));
    fs::write(&sidecar, format!("{digest_value} inventory.json\n"))
        .map_err(|e| format!("{}: {e}", sidecar.display()))
}

/// The object roots below a storage root, with the inventory each declares.
pub struct Object {
    pub root: PathBuf,
    pub inventory: Inventory,
}

pub fn discover(root: &Path) -> Result<Vec<Object>, String> {
    let mut roots = Vec::new();
    collect_objects(root, &mut roots, 0)?;
    let mut out = Vec::new();
    for object_root in roots {
        let inventory = Inventory::read(&object_root)?;
        out.push(Object {
            root: object_root,
            inventory,
        });
    }
    out.sort_by(|a, b| a.inventory.id.cmp(&b.inventory.id));
    Ok(out)
}

fn collect_objects(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) -> Result<(), String> {
    if depth > 6 {
        return Err(format!(
            "{}: object nesting is deeper than 6 levels — is this an OCFL storage root?",
            dir.display()
        ));
    }
    if dir.join(DECLARATION).is_file() {
        out.push(dir.to_path_buf());
        return Ok(());
    }
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(()),
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    paths.sort();
    for path in paths {
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            if path.file_name().and_then(|name| name.to_str()) == Some(".git") {
                continue;
            }
            collect_objects(&path, out, depth + 1)?;
        }
    }
    Ok(())
}
