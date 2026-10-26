//! Resolve a target name to a build artefact this workspace produced.
//!
//! The grammar accepts a *name*, never a path and never a command line. Resolution walks up to the
//! nearest `target/<profile>`, canonicalises the hit, refuses a symlink, and then checks that some
//! `Cargo.toml` under that root actually declares the target. Copying a binary into `target/` is
//! not enough: it has to be a build output the workspace describes.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use sha2::{Digest, Sha256};

use crate::report::ProgramInfo;
use crate::scenario::{is_bin_name, Profile};

#[derive(Debug)]
pub struct Resolved {
    pub path: PathBuf,
    pub info: ProgramInfo,
}

pub fn resolve(bin: &str, profile: Profile, cwd: &Path) -> Result<Resolved> {
    if !is_bin_name(bin) {
        bail!(
            "`{bin}` is not a binary target name; use letters, digits, `_` or `-` (no path, no \
             arguments)"
        );
    }
    if !cwd.is_dir() {
        bail!("working directory {} is not a directory", cwd.display());
    }
    let cwd = fs::canonicalize(cwd)
        .with_context(|| format!("resolve working directory {}", cwd.display()))?;

    let mut directory = cwd.as_path();
    loop {
        let target = directory.join("target");
        for candidate in [
            target.join(profile.directory()).join(bin),
            target.join(profile.directory()).join("examples").join(bin),
        ] {
            if !candidate.is_file() {
                continue;
            }
            if fs::symlink_metadata(&candidate)
                .map(|metadata| metadata.file_type().is_symlink())
                .unwrap_or(false)
            {
                bail!(
                    "{} is a symlink; tui-shot runs only a regular build artefact",
                    candidate.display()
                );
            }
            let resolved = fs::canonicalize(&candidate)
                .with_context(|| format!("resolve {}", candidate.display()))?;
            let target_root = fs::canonicalize(&target)
                .with_context(|| format!("resolve {}", target.display()))?;
            if !resolved.starts_with(&target_root) {
                bail!(
                    "{} resolves outside the target directory {}",
                    candidate.display(),
                    target_root.display()
                );
            }
            if !declares(directory, bin)? {
                bail!(
                    "`{bin}` is built in {}, but no Cargo.toml there declares it as a binary or \
                     example target; tui-shot runs this workspace's targets only",
                    directory.display()
                );
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = fs::metadata(&resolved)?.permissions().mode();
                if mode & 0o111 == 0 {
                    bail!("{} is not executable", resolved.display());
                }
            }
            let info = describe(bin, profile, directory, &resolved)?;
            return Ok(Resolved {
                path: resolved,
                info,
            });
        }
        match directory.parent() {
            Some(parent) => directory = parent,
            None => {
                bail!(
                    "no built binary named `{bin}` under {}/target/{} · build it first, as \
                     `cargo build --bin {bin}`",
                    cwd.display(),
                    profile.directory()
                )
            }
        }
    }
}

fn describe(bin: &str, profile: Profile, workspace: &Path, path: &Path) -> Result<ProgramInfo> {
    let metadata = fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    let modified = metadata
        .modified()
        .ok()
        .map(|time| DateTime::<Utc>::from(time).to_rfc3339_opts(SecondsFormat::Millis, true))
        .unwrap_or_else(|| "unknown".to_owned());

    let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("read {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(ProgramInfo {
        bin: bin.to_owned(),
        path: path
            .strip_prefix(workspace)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/"),
        profile: profile.to_string(),
        bytes: metadata.len(),
        sha256: format!("{:x}", hasher.finalize()),
        modified,
    })
}

/// True when some manifest under `root` declares `bin` as a binary or example target.
fn declares(root: &Path, bin: &str) -> Result<bool> {
    let mut visited = 0usize;
    scan(root, bin, 0, &mut visited)
}

fn scan(directory: &Path, bin: &str, depth: usize, visited: &mut usize) -> Result<bool> {
    if depth > 8 || *visited > 4000 {
        return Ok(false);
    }

    let manifest = directory.join("Cargo.toml");
    if manifest.is_file() {
        *visited += 1;
        if manifest_declares(directory, &manifest, bin)? {
            return Ok(true);
        }
    }

    for entry in fs::read_dir(directory).with_context(|| format!("read {}", directory.display()))? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if matches!(
            name.as_ref(),
            "target" | ".git" | ".tmp" | "node_modules" | "dist" | "result"
        ) || name.starts_with('.')
        {
            continue;
        }
        if scan(&entry.path(), bin, depth + 1, visited)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn manifest_declares(package: &Path, manifest: &Path, bin: &str) -> Result<bool> {
    let text =
        fs::read_to_string(manifest).with_context(|| format!("read {}", manifest.display()))?;

    #[derive(PartialEq)]
    enum Section {
        Other,
        Package,
        Bin,
        Example,
    }
    let mut section = Section::Other;
    let mut package_name = None;

    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("[[bin]]") {
            section = Section::Bin;
            continue;
        }
        if line.starts_with("[[example]]") {
            section = Section::Example;
            continue;
        }
        if line.starts_with('[') {
            section = if line.starts_with("[package]") {
                Section::Package
            } else {
                Section::Other
            };
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "name" {
            continue;
        }
        let value = value.trim().trim_matches(['"', '\'']);
        if matches!(section, Section::Bin | Section::Example) && value == bin {
            return Ok(true);
        }
        if section == Section::Package {
            package_name = Some(value.to_owned());
        }
    }

    if package.join("src/bin").join(format!("{bin}.rs")).is_file() {
        return Ok(true);
    }
    if package.join("src/bin").join(bin).join("main.rs").is_file() {
        return Ok(true);
    }
    if package.join("examples").join(format!("{bin}.rs")).is_file()
        || package.join("examples").join(bin).join("main.rs").is_file()
    {
        return Ok(true);
    }
    if package_name.as_deref() == Some(bin) && package.join("src/main.rs").is_file() {
        return Ok(true);
    }

    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_with_a_separator_is_refused() {
        let error = resolve("../sh", Profile::Debug, Path::new("."))
            .unwrap_err()
            .to_string();
        assert!(error.contains("not a binary target name"), "{error}");
    }

    #[test]
    fn a_name_is_recognised_in_a_src_bin_path() {
        let root = std::env::temp_dir().join(format!("tui-shot-declare-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src/bin")).unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        fs::write(root.join("src/bin/demo.rs"), "fn main() {}").unwrap();
        assert!(declares(&root, "demo").unwrap());
        assert!(!declares(&root, "other").unwrap());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_name_is_recognised_in_a_bin_table() {
        let root = std::env::temp_dir().join(format!("tui-shot-bin-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"x\"\n\n[[bin]]\nname = \"demo\"\npath = \"src/main.rs\"\n",
        )
        .unwrap();
        assert!(declares(&root, "demo").unwrap());
        fs::remove_dir_all(&root).unwrap();
    }
}
