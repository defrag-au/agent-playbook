//! Verifying an object the way a consumer cares about.
//!
//! This is not a full OCFL validator. It checks the inventory sidecar, the version chain, the
//! state-to-manifest references, every content digest, that no content file is unaccounted for,
//! and that per-version inventory snapshots agree with their directory. That is enough to trust
//! an export; fixity blocks and extensions are ignored.

use std::collections::BTreeSet;
use std::path::Path;

use crate::digest;
use crate::fsx::{safe_relative, walk_files};
use crate::inventory::{self, Inventory};
use crate::store::Store;

pub struct Report {
    pub objects: usize,
    pub files: usize,
    pub errors: Vec<String>,
}

pub fn verify(store: &Store, skill: Option<&str>, version: Option<&str>) -> Result<Report, String> {
    let objects = match skill {
        Some(name) => vec![store.object(name)?],
        None => store.objects()?,
    };
    let mut report = Report {
        objects: objects.len(),
        files: 0,
        errors: Vec::new(),
    };
    for object in &objects {
        verify_object(&object.root, version, &mut report);
    }
    Ok(report)
}

fn verify_object(root: &Path, only: Option<&str>, report: &mut Report) {
    let label = root.display().to_string();
    let inventory = match Inventory::read(root) {
        Ok(inventory) => inventory,
        Err(error) => {
            report.errors.push(format!("{label}: {error}"));
            return;
        }
    };
    let algorithm = match inventory.algorithm() {
        Ok(algorithm) => algorithm,
        Err(error) => {
            report.errors.push(format!("{label}: {error}"));
            return;
        }
    };
    if let Err(error) = inventory::verify_sidecar(root, algorithm) {
        report.errors.push(format!("{label}: {error}"));
    }
    report.errors.extend(
        inventory
            .validate()
            .into_iter()
            .map(|e| format!("{label}: {e}")),
    );

    if let Some(only) = only {
        if !inventory.versions.contains_key(only) {
            report.errors.push(format!(
                "{label}: no version `{only}` (has {})",
                inventory.version_list()
            ));
        }
    }

    let versions: Vec<String> = match only {
        Some(version) => vec![version.to_string()],
        None => inventory.versions.keys().cloned().collect(),
    };
    let mut checked: BTreeSet<String> = BTreeSet::new();

    for version in &versions {
        let state = match inventory.state_for(version) {
            Ok(state) => state,
            Err(error) => {
                report.errors.push(format!("{label}: {error}"));
                continue;
            }
        };
        for (logical, digest_value) in &state {
            if let Err(error) = safe_relative(logical) {
                report.errors.push(format!("{label}: {version}: {error}"));
            }
            let Some(paths) = inventory.manifest.get(digest_value) else {
                report.errors.push(format!(
                    "{label}: {version}: `{logical}` refers to {digest_value}, which is not in the \
                     manifest"
                ));
                continue;
            };
            let Some(content_path) = paths.first() else {
                report.errors.push(format!(
                    "{label}: {version}: manifest digest `{digest_value}` has no content path"
                ));
                continue;
            };
            if checked.insert(content_path.clone()) {
                check_content(root, content_path, digest_value, algorithm, report, &label);
            }
        }
    }

    if only.is_some() {
        return;
    }

    // Content that no current state references is still stored, and still has to hash.
    for (digest_value, paths) in &inventory.manifest {
        for content_path in paths {
            if checked.insert(content_path.clone()) {
                check_content(root, content_path, digest_value, algorithm, report, &label);
            }
        }
    }

    // And every file in a version's content directory has to be in the manifest.
    let known: BTreeSet<&String> = inventory.manifest.values().flatten().collect();
    for (_, version) in inventory.ordered_versions().unwrap_or_default() {
        let directory = root.join(&version).join(inventory.content_dir());
        if !directory.is_dir() {
            continue;
        }
        match walk_files(&directory) {
            Ok(files) => {
                for (relative, _) in files {
                    let content_path =
                        format!("{}/{}/{}", version, inventory.content_dir(), relative);
                    if !known.contains(&content_path) {
                        report.errors.push(format!(
                            "{label}: {content_path} is in the store but not in the manifest"
                        ));
                    }
                }
            }
            Err(error) => report.errors.push(format!("{label}: {error}")),
        }
    }

    // A version directory may carry an inventory snapshot; where it does, it must describe its
    // own version as the head.
    for (_, version) in inventory.ordered_versions().unwrap_or_default() {
        let directory = root.join(&version);
        if !directory.join("inventory.json").is_file() {
            continue;
        }
        if let Err(error) = inventory::verify_sidecar(&directory, algorithm) {
            report.errors.push(format!("{label}: {version}: {error}"));
        }
        match Inventory::read(&directory) {
            Ok(snapshot) if snapshot.head != version => report.errors.push(format!(
                "{label}: {version}/inventory.json has head `{}`",
                snapshot.head
            )),
            Ok(_) => {}
            Err(error) => report.errors.push(format!("{label}: {version}: {error}")),
        }
    }
}

fn check_content(
    root: &Path,
    content_path: &str,
    expected: &str,
    algorithm: digest::Algorithm,
    report: &mut Report,
    label: &str,
) {
    let path = root.join(content_path);
    match digest::file_hex(&path, algorithm) {
        Ok(actual) if actual == expected => report.files += 1,
        Ok(actual) => report.errors.push(format!(
            "{label}: {content_path}: digest is {actual}, inventory says {expected}"
        )),
        Err(error) => report.errors.push(format!("{label}: {error}")),
    }
}
