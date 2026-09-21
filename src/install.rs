//! Writing the managed block into a repository's agent file.
//!
//! Only ever replaces the text between the markers, so hand-written content in the file is
//! untouched by construction — the two never occupy the same bytes. That is why this can
//! be a generator without violating the "edit with editor tools" rule: the content is
//! computed, and `check` makes the result verifiable rather than trusted.

use std::fs;
use std::path::{Path, PathBuf};

use crate::render::{extract_block, BEGIN_PREFIX, END_MARKER};

pub enum Outcome {
    Created(PathBuf),
    Updated(PathBuf),
    Unchanged(PathBuf),
    UpToDate(PathBuf),
    /// The block is present but differs from the composed one.
    Stale(PathBuf, String),
    /// Checked, but the file does not exist.
    Missing(PathBuf),
    /// Checked, and the file exists but has no managed block.
    NoBlock(PathBuf),
}

impl Outcome {
    pub fn path(&self) -> &Path {
        match self {
            Outcome::Created(p)
            | Outcome::Updated(p)
            | Outcome::Unchanged(p)
            | Outcome::UpToDate(p)
            | Outcome::Stale(p, _)
            | Outcome::Missing(p)
            | Outcome::NoBlock(p) => p,
        }
    }
}

/// Replace the managed block in `existing` with `block`, or append it.
pub fn splice(existing: &str, block: &str) -> String {
    let lines: Vec<&str> = existing.split_inclusive('\n').collect();

    let begin = lines.iter().position(|l| l.starts_with(BEGIN_PREFIX));
    let end = begin.and_then(|b| {
        lines[b..]
            .iter()
            .position(|l| l.starts_with(END_MARKER))
            .map(|offset| b + offset)
    });

    match (begin, end) {
        (Some(b), Some(e)) => {
            let mut out = String::new();
            for line in &lines[..b] {
                out.push_str(line);
            }
            out.push_str(block);
            for line in &lines[e + 1..] {
                out.push_str(line);
            }
            out
        }
        _ => {
            let mut out = existing.to_string();
            if !out.is_empty() {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push('\n');
            }
            out.push_str(block);
            out
        }
    }
}

pub fn write(repo: &Path, file: &str, block: &str, check: bool) -> Result<Outcome, String> {
    let path = repo.join(file);

    if check {
        let existing = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return Ok(Outcome::Missing(path)),
        };
        let Some(found) = extract_block(&existing) else {
            return Ok(Outcome::NoBlock(path));
        };
        if found == block {
            return Ok(Outcome::UpToDate(path));
        }
        return Ok(Outcome::Stale(path, describe_difference(&found, block)));
    }

    if !path.exists() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        fs::write(&path, block).map_err(|e| format!("{}: {e}", path.display()))?;
        return Ok(Outcome::Created(path));
    }

    let existing = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let merged = splice(&existing, block);
    if merged == existing {
        return Ok(Outcome::Unchanged(path));
    }
    fs::write(&path, merged).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Outcome::Updated(path))
}

/// A window around the first difference. Enough to see what changed without pasting two
/// hundred lines of rule text into a terminal.
fn describe_difference(old: &str, new: &str) -> String {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    let first =
        (0..old_lines.len().max(new_lines.len())).find(|&i| old_lines.get(i) != new_lines.get(i));

    let Some(first) = first else {
        return String::from("(blocks differ only in trailing whitespace)");
    };

    let start = first.saturating_sub(2);
    let mut out = format!(
        "first difference at line {} ({} lines in the file, {} composed)\n",
        first + 1,
        old_lines.len(),
        new_lines.len()
    );

    for i in start..first {
        if let Some(line) = old_lines.get(i) {
            out.push_str(&format!("   {line}\n"));
        }
    }
    for i in first..(first + 6).min(old_lines.len().max(new_lines.len())) {
        match (old_lines.get(i), new_lines.get(i)) {
            (Some(a), Some(b)) if a == b => out.push_str(&format!("   {a}\n")),
            (Some(a), Some(b)) => {
                out.push_str(&format!("  -{a}\n"));
                out.push_str(&format!("  +{b}\n"));
            }
            (Some(a), None) => out.push_str(&format!("  -{a}\n")),
            (None, Some(b)) => out.push_str(&format!("  +{b}\n")),
            (None, None) => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: &str = "<!-- BEGIN agent-playbook (project: x, target: y) — generated, do not edit -->\n\n# Agent rules — x\n\nbody\n<!-- END agent-playbook -->\n";

    #[test]
    fn appends_to_a_file_with_no_block() {
        let merged = splice("# Notes\n\nSomething I wrote.\n", BLOCK);
        assert!(merged.starts_with("# Notes\n\nSomething I wrote.\n"));
        assert!(merged.contains(BEGIN_PREFIX));
        assert!(merged.trim_end().ends_with(END_MARKER));
    }

    #[test]
    fn appends_to_an_empty_file_without_a_leading_blank_line() {
        assert_eq!(splice("", BLOCK), BLOCK);
    }

    #[test]
    fn replaces_only_the_block() {
        let existing = "Above.\n\n<!-- BEGIN agent-playbook (project: x, target: y) -->\nstale\n<!-- END agent-playbook -->\n\nBelow.\n";
        let merged = splice(existing, BLOCK);
        assert!(merged.starts_with("Above.\n\n"));
        assert!(merged.contains("body"));
        assert!(!merged.contains("stale"));
        assert!(merged.ends_with("Below.\n"));
    }

    #[test]
    fn splicing_is_idempotent() {
        // The property that makes `check` meaningful: compose, install, install again,
        // and the file must not move.
        let once = splice("# Notes\n\nProse.\n", BLOCK);
        let twice = splice(&once, BLOCK);
        assert_eq!(once, twice);
    }

    #[test]
    fn a_half_deleted_block_is_treated_as_absent() {
        // An opening marker with no closing one. Appending is recoverable; splicing over
        // an unknown span is not.
        let existing = "Above.\n\n<!-- BEGIN agent-playbook (project: x, target: y) -->\norphan\n";
        let merged = splice(existing, BLOCK);
        assert!(merged.contains("orphan"));
        assert!(merged.trim_end().ends_with(END_MARKER));
    }

    #[test]
    fn difference_names_the_first_changed_line() {
        let diff = describe_difference("a\nb\nc\n", "a\nB\nc\n");
        assert!(diff.contains("line 2"));
        assert!(diff.contains("-b"));
        assert!(diff.contains("+B"));
    }
}
