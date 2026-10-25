//! Exporting one OCFL version as a plain skill directory.
//!
//! The output is an envelope, not the skill itself:
//!
//! ```text
//! out/
//! ├── export.conf     # flat: name, id, version, source, digests
//! └── skill/          # SKILL.md, references/… — exactly what an agent reads
//! ```
//!
//! The envelope is what lets a target-aware consumer (`playbook`) read the facts without a JSON
//! parser or a network call.

use std::fs;
use std::path::Path;

use crate::digest;
use crate::fsx::{self, safe_relative};
use crate::inventory::Inventory;
use crate::skill;
use crate::store::Store;

pub struct Report {
    pub name: String,
    pub id: String,
    pub version: String,
    pub files: usize,
    pub tree_digest: String,
}

pub fn export(
    store: &Store,
    skill_name: &str,
    version: Option<&str>,
    out: &Path,
) -> Result<Report, String> {
    let object = store.object(skill_name)?;
    let inventory = Inventory::read_verified(&object.root)?;
    let errors = inventory.validate();
    if !errors.is_empty() {
        return Err(format!(
            "{}: the object is not valid:\n  {}",
            object.root.display(),
            errors.join("\n  ")
        ));
    }
    let version = version.unwrap_or(&inventory.head).to_string();
    let state = inventory.state_for(&version)?;
    let algorithm = inventory.algorithm()?;

    prepare_out(out)?;
    let tree = out.join("skill");
    fs::create_dir_all(&tree).map_err(|e| format!("{}: {e}", tree.display()))?;

    for (logical, digest_value) in &state {
        safe_relative(logical)?;
        let paths = inventory.manifest.get(digest_value).ok_or_else(|| {
            format!("`{digest_value}` is in the state of {version} but not in the manifest")
        })?;
        let content_path = paths
            .first()
            .ok_or_else(|| format!("manifest digest `{digest_value}` has no content path"))?;
        let source = object.root.join(content_path);
        let metadata =
            fs::symlink_metadata(&source).map_err(|e| format!("{}: {e}", source.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!(
                "{}: manifest content is not a regular file",
                source.display()
            ));
        }
        let bytes = fs::read(&source).map_err(|e| format!("{}: {e}", source.display()))?;
        let actual = digest::bytes_hex(&bytes, algorithm);
        if &actual != digest_value {
            return Err(format!(
                "{}: content digest is {actual}, but the inventory says {digest_value}",
                source.display()
            ));
        }
        let destination = tree.join(logical);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        fs::write(&destination, &bytes).map_err(|e| format!("{}: {e}", destination.display()))?;
    }

    let meta = skill::read_skill_md(&tree.join("SKILL.md"), Some(skill_name))?;
    if meta.name != skill_name {
        return Err(format!(
            "{}: `name: {}` does not match the selected skill `{skill_name}`",
            tree.display(),
            meta.name
        ));
    }

    let tree_digest = digest::tree_hex(&tree)?;
    let inventory_digest = format!(
        "{}:{}",
        algorithm.name(),
        digest::file_hex(&object.root.join("inventory.json"), algorithm)?
    );
    let files = state.len();

    let mut pairs: Vec<(&str, String)> = vec![
        ("name", meta.name.clone()),
        ("id", inventory.id.clone()),
        ("version", version.clone()),
        ("source", store.label.clone()),
    ];
    if let Some(rev) = &store.rev {
        pairs.push(("store_rev", rev.clone()));
    }
    pairs.push(("inventory_digest", inventory_digest));
    pairs.push(("tree_digest", tree_digest.clone()));
    pairs.push(("files", files.to_string()));
    fsx::write_flat(&out.join("export.conf"), &pairs)?;

    Ok(Report {
        name: meta.name,
        id: inventory.id,
        version,
        files,
        tree_digest,
    })
}

fn prepare_out(out: &Path) -> Result<(), String> {
    if out.exists() {
        if !out.is_dir() {
            return Err(format!("{} exists and is not a directory", out.display()));
        }
        let mut entries = fs::read_dir(out).map_err(|e| format!("{}: {e}", out.display()))?;
        if entries.next().is_some() {
            return Err(format!(
                "{} is not empty — choose a new directory for the export",
                out.display()
            ));
        }
    } else {
        fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    Ok(())
}
