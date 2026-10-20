//! The crate-reference contract: every file is indexed, and none is a context bomb.
//!
//! The references are data, not compiled code. These tests keep the index honest and put a
//! hard ceiling on accidental API dumps; the target is 8 KB, the ceiling is 10 KB.

use std::fs;
use std::path::{Path, PathBuf};

const MAX_REFERENCE_BYTES: u64 = 10_000;

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("references/crates")
}

fn reference_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(crates_dir())
        .expect("the references/crates directory")
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "md")
                && path.file_name().is_some_and(|name| name != "CATALOG.md")
        })
        .collect();
    files.sort();
    files
}

#[test]
fn every_reference_is_catalogued_and_within_budget() {
    let catalog =
        fs::read_to_string(crates_dir().join("CATALOG.md")).expect("references/crates/CATALOG.md");

    let files = reference_files();
    assert!(!files.is_empty(), "no crate references found");

    for path in files {
        let name = path
            .file_name()
            .expect("a reference file name")
            .to_string_lossy();
        assert!(
            catalog.contains(&format!("`{name}`")),
            "{name} has no row in references/crates/CATALOG.md"
        );

        let bytes = fs::metadata(&path).expect("reference metadata").len();
        assert!(
            bytes <= MAX_REFERENCE_BYTES,
            "{name} is {bytes} bytes; the ceiling is {MAX_REFERENCE_BYTES}. \
             Split it by task or trim it; the target is 8 KB."
        );

        let text = fs::read_to_string(&path).expect("reference text");
        assert!(
            text.lines()
                .next()
                .is_some_and(|line| line.starts_with("# ")),
            "{name} has no top-level title"
        );
        assert!(
            text.lines().any(|line| line == "## At a glance"),
            "{name} has no `## At a glance` section; it is what the reader loads first"
        );
    }
}

#[test]
fn every_catalog_row_names_a_real_file() {
    let catalog =
        fs::read_to_string(crates_dir().join("CATALOG.md")).expect("references/crates/CATALOG.md");

    for line in catalog.lines().filter(|line| line.starts_with("| ")) {
        for part in line.split('`') {
            if part.ends_with(".md") && part != "CATALOG.md" {
                assert!(
                    crates_dir().join(part).is_file(),
                    "CATALOG.md names {part}, which does not exist"
                );
            }
        }
    }
}
