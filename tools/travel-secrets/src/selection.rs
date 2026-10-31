//! Turn whitelist names into the concrete `.gpg` files to carry.
//!
//! A whitelist entry may be a directory (walked recursively) or one entry, with or without the
//! `.gpg` suffix. Symlinks are refused everywhere: `pass` follows them on read, writes through
//! them on insert, and fails inconsistently on delete, so a selection built from links is not a
//! subset. Paths are contained by `at_core::paths::Root`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path};

use at_core::contract::Fail;
use at_core::paths::Root;

use crate::whitelist::Whitelist;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The pass entry path without `.gpg`, as a manifest reader would say it.
    pub entry: String,
    /// The file path relative to the store, with `.gpg`.
    pub file: String,
}

pub fn select(store: &Path, whitelist: &Whitelist) -> Result<Vec<Entry>, Fail> {
    let root = Root::at(store)?;
    let store = root.dir().to_path_buf();
    let mut selected = BTreeMap::new();

    for raw in &whitelist.entries {
        let raw = raw.trim().trim_end_matches('/');
        if raw.is_empty() || raw == "." {
            return Err(Fail::usage("the whitelist names an empty path"));
        }
        let candidate = Path::new(raw);
        if candidate.is_absolute()
            || candidate
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(Fail::refused(format!(r"`{raw}` escapes the store")));
        }

        let path = store.join(candidate);
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(Fail::refused(format!(
                        "{} is a symlink · symlinks are not followed",
                        path.display()
                    )));
                }
                if metadata.is_dir() {
                    walk(&root, &path, &mut selected)?;
                } else if metadata.is_file() {
                    if raw.ends_with(".gpg") {
                        add(&root, &path, &mut selected)?;
                    } else {
                        return Err(Fail::refused(format!(
                            "{} is a file, not a directory or `.gpg` entry",
                            path.display()
                        )));
                    }
                } else {
                    return Err(Fail::refused(format!(
                        "{} is not a file or directory",
                        path.display()
                    )));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let with_extension = store.join(format!("{raw}.gpg"));
                match fs::symlink_metadata(&with_extension) {
                    Ok(metadata) if metadata.file_type().is_symlink() => {
                        return Err(Fail::refused(format!(
                            "{} is a symlink · symlinks are not followed",
                            with_extension.display()
                        )));
                    }
                    Ok(metadata) if metadata.is_file() => {
                        add(&root, &with_extension, &mut selected)?;
                    }
                    _ => {
                        return Err(Fail::refused(format!(
                            "{} is not a directory or a `.gpg` entry",
                            path.display()
                        )));
                    }
                }
            }
            Err(error) => {
                return Err(Fail::environment(format!(
                    "cannot read {}: {error}",
                    path.display()
                )));
            }
        }
    }

    if selected.is_empty() {
        return Err(Fail::refused("the whitelist selected no entries"));
    }

    Ok(selected.into_values().collect())
}

fn walk(root: &Root, dir: &Path, selected: &mut BTreeMap<String, Entry>) -> Result<(), Fail> {
    let entries = fs::read_dir(dir)
        .map_err(|error| Fail::environment(format!("cannot read {}: {error}", dir.display())))?;

    for entry in entries {
        let entry = entry.map_err(|error| {
            Fail::environment(format!("cannot read {}: {error}", dir.display()))
        })?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            Fail::environment(format!("cannot read {}: {error}", path.display()))
        })?;

        if metadata.file_type().is_symlink() {
            return Err(Fail::refused(format!(
                "{} is a symlink · symlinks are not followed",
                path.display()
            )));
        }
        if metadata.is_dir() {
            if name == ".git" {
                continue;
            }
            walk(root, &path, selected)?;
        } else if metadata.is_file() {
            if name == ".gpg-id" || name == ".DS_Store" {
                continue;
            }
            if !name.ends_with(".gpg") {
                return Err(Fail::refused(format!(
                    "{} is not a `.gpg` entry",
                    path.display()
                )));
            }
            add(root, &path, selected)?;
        } else {
            return Err(Fail::refused(format!(
                "{} is not a file or directory",
                path.display()
            )));
        }
    }

    Ok(())
}

fn add(root: &Root, path: &Path, selected: &mut BTreeMap<String, Entry>) -> Result<(), Fail> {
    let relative = path
        .strip_prefix(root.dir())
        .map_err(|_| Fail::refused(format!("{} is outside the store", path.display())))?;
    let file = relative.to_string_lossy().replace('\\', "/");
    let entry = file
        .strip_suffix(".gpg")
        .ok_or_else(|| Fail::refused(format!("{} is not a `.gpg` entry", path.display())))?
        .to_string();

    // Canonicalise and contain the file itself, not only the directory the walk started from.
    root.resolve(&file)?;
    selected.insert(entry.clone(), Entry { entry, file });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_store(label: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "travel-secrets-select-{label}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("github/sub")).expect("github");
        fs::create_dir_all(root.join("cloudflare")).expect("cloudflare");
        fs::create_dir_all(root.join("personal")).expect("personal");
        fs::write(root.join("github/foo.gpg"), "a").expect("foo");
        fs::write(root.join("github/sub/bar.gpg"), "b").expect("bar");
        fs::write(root.join("cloudflare/api.gpg"), "c").expect("api");
        fs::write(root.join("personal/secret.gpg"), "d").expect("secret");
        fs::write(root.join(".gpg-id"), "TEST").expect("gpg-id");
        fs::write(root.join("github/.gpg-id"), "TEST").expect("gpg-id");
        fs::write(root.join("guest.txt"), "not a pass entry").expect("guest");
        root
    }

    fn whitelist(entries: &[&str]) -> Whitelist {
        Whitelist {
            entries: entries.iter().map(|entry| entry.to_string()).collect(),
        }
    }

    #[test]
    fn a_directory_selects_every_gpg_below_it() {
        let store = temp_store("directory");
        let entries = select(&store, &whitelist(&["github"])).expect("select");
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.entry.as_str())
                .collect::<Vec<_>>(),
            ["github/foo", "github/sub/bar"]
        );
        fs::remove_dir_all(store).expect("cleanup");
    }

    #[test]
    fn an_individual_entry_is_selected_with_or_without_the_suffix() {
        let store = temp_store("entry");
        let entries = select(&store, &whitelist(&["cloudflare/api"])).expect("select");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].file, "cloudflare/api.gpg");
        fs::remove_dir_all(store).expect("cleanup");
    }

    #[test]
    fn duplicates_are_collapsed() {
        let store = temp_store("duplicates");
        let entries = select(&store, &whitelist(&["github", "github/foo"])).expect("select");
        assert_eq!(entries.len(), 2);
        fs::remove_dir_all(store).expect("cleanup");
    }

    #[test]
    fn a_missing_entry_is_refused() {
        let store = temp_store("missing");
        let error = select(&store, &whitelist(&["does-not-exist"])).expect_err("missing");
        assert_eq!(error.exit, at_core::contract::Exit::Refused);
        fs::remove_dir_all(store).expect("cleanup");
    }

    #[test]
    fn a_path_outside_the_store_is_refused() {
        let store = temp_store("escape");
        let error = select(&store, &whitelist(&["../outside"])).expect_err("escape");
        assert_eq!(error.exit, at_core::contract::Exit::Refused);
        fs::remove_dir_all(store).expect("cleanup");
    }

    #[test]
    fn a_symlink_is_refused() {
        let store = temp_store("symlink");
        std::os::unix::fs::symlink("github/foo.gpg", store.join("link.gpg")).expect("symlink");
        let error = select(&store, &whitelist(&["link"])).expect_err("symlink");
        assert!(error.message.contains("symlink"));
        fs::remove_dir_all(store).expect("cleanup");
    }

    #[test]
    fn an_unexpected_file_is_refused() {
        let store = temp_store("guest");
        let error = select(&store, &whitelist(&["guest.txt"])).expect_err("not an entry");
        assert!(error.message.contains("not a directory"));
        fs::remove_dir_all(store).expect("cleanup");
    }
}
