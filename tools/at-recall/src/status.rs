//! The repository's state, parsed from `git status --porcelain=v2`.
//!
//! One parser for the one git output that carries the branch, the upstream divergence and the
//! working-tree state in a single machine-readable pass. Two verbs read it: `state` renders it, and
//! `diff` uses it to learn which paths a worktree comparison could touch without touching one.
//!
//! It is deliberately in the middle of the crate rather than inside the verb that needs the most
//! from it, because the second reader is a *safety* reader: a path this parser misses is a path
//! whose filter driver would run unchecked.

use at_core::contract::Fail;

use crate::git::{with_paths, Git, Noun, GIT};

/// One changed path.
pub struct Entry {
    /// The two-letter code git reported.
    pub code: String,
    pub path: String,
    /// The other half of a rename, when git detected one.
    pub original: Option<String>,
}

impl Entry {
    fn line(&self) -> String {
        let code = short_code(&self.code);
        match &self.original {
            Some(original) => format!("{code}  {original} -> {}", self.path),
            None => format!("{code}  {}", self.path),
        }
    }

    /// Both names, so a rename is checked on each side.
    fn names(&self) -> impl Iterator<Item = &String> {
        std::iter::once(&self.path).chain(self.original.iter())
    }
}

/// Porcelain v2 writes `.` where `git status --short` writes a space — "unmodified on this side of
/// the comparison". Rendered as the space, because the two-letter column is the one thing a reader
/// is expected to recognise, and `.M` is not a form anyone has seen before.
fn short_code(code: &str) -> String {
    code.replace('.', " ")
}

pub struct Status {
    /// The full HEAD hash, or `None` on a branch with no commits yet.
    oid: Option<String>,
    /// The branch as git prints it, `(detached)` included: this tool does not rename what git named.
    branch: String,
    upstream: String,
    entries: Vec<Entry>,
    /// Records that did not parse. Counted rather than dropped silently, because a missing entry
    /// reads as a change that is not there.
    unparsed: usize,
}

impl Status {
    pub fn read(git: &Git) -> Result<Status, Fail> {
        Status::read_paths(git, &[])
    }

    /// The tracked paths only, for a caller whose question is "does HEAD's version of any file
    /// differ from what is on disk". Untracked names are one of `state`'s answers, and producing
    /// them means walking every untracked directory — so this asks git not to do that walk rather
    /// than filtering its result afterwards.
    ///
    /// It is also the read with no conversion in it. `git status` reports a path as modified from
    /// what it can see without turning the file into the blob it would commit, which is what makes
    /// this safe to run in a repository that names a `clean` filter — unlike a comparison against
    /// the working tree, which has to convert the file and is why `diff` refuses one.
    pub fn read_tracked(git: &Git) -> Result<Status, Fail> {
        Status::read_with(git, &[], "no")
    }

    /// The state of the paths a caller named, or of the whole worktree when none were.
    /// `--untracked-files=normal` is explicit because it is a repository setting
    /// (`status.showUntrackedFiles`), and a repository that had turned it off would otherwise make
    /// this report a clean tree that is not clean.
    pub fn read_paths(git: &Git, paths: &[String]) -> Result<Status, Fail> {
        Status::read_with(git, paths, "normal")
    }

    fn read_with(git: &Git, paths: &[String], untracked: &str) -> Result<Status, Fail> {
        let mut args: Vec<String> = vec![
            "--porcelain=v2".to_string(),
            "--branch".to_string(),
            format!("--untracked-files={untracked}"),
            "-z".to_string(),
        ];
        with_paths(&mut args, paths);
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        Status::parse(&git.run(Noun::Status, &borrowed)?)
    }

    fn parse(raw: &str) -> Result<Status, Fail> {
        let mut status = Status {
            oid: None,
            branch: String::new(),
            upstream: "none".to_string(),
            entries: Vec::new(),
            unparsed: 0,
        };
        let mut saw_branch = false;

        let mut records = raw.split('\0');
        while let Some(record) = records.next() {
            if record.is_empty() {
                continue;
            }
            if let Some(rest) = record.strip_prefix("# branch.oid ") {
                // `(initial)` is git's word for a branch with no commits, not a hash.
                status.oid = (rest != "(initial)").then(|| rest.to_string());
                saw_branch = true;
            } else if let Some(rest) = record.strip_prefix("# branch.head ") {
                status.branch = rest.to_string();
            } else if let Some(rest) = record.strip_prefix("# branch.upstream ") {
                status.upstream = rest.to_string();
            } else if let Some(rest) = record.strip_prefix("# branch.ab ") {
                // `+2 -1`. Absent when there is no upstream, which is why the default above is the
                // bare word rather than an empty field.
                if let Some((ahead, behind)) = rest.split_once(' ') {
                    status.upstream = format!(
                        "{} · ahead {} behind {}",
                        status.upstream,
                        ahead.trim_start_matches('+'),
                        behind.trim_start_matches('-')
                    );
                }
            } else if let Some(path) = record.strip_prefix("? ") {
                status.entries.push(Entry {
                    code: "??".to_string(),
                    path: path.to_string(),
                    original: None,
                });
            } else if record.starts_with("! ") {
                // Ignored paths are not part of "what has changed".
            } else if let Some(rest) = record.strip_prefix("1 ") {
                match split_record(rest, 8) {
                    Some((code, path)) => status.entries.push(Entry {
                        code,
                        path,
                        original: None,
                    }),
                    None => status.unparsed += 1,
                }
            } else if let Some(rest) = record.strip_prefix("2 ") {
                match split_record(rest, 9) {
                    // A `2` record carries the original path as the next NUL-separated field when
                    // `-z` is in effect, which is the only ordering git does not state inline.
                    Some((code, path)) => {
                        let original = records.next().map(|name| name.to_string());
                        status.entries.push(Entry {
                            code,
                            path,
                            original,
                        });
                    }
                    None => status.unparsed += 1,
                }
            } else if let Some(rest) = record.strip_prefix("u ") {
                match split_record(rest, 10) {
                    Some((code, path)) => status.entries.push(Entry {
                        code,
                        path,
                        original: None,
                    }),
                    None => status.unparsed += 1,
                }
            } else {
                status.unparsed += 1;
            }
        }

        if !saw_branch || status.branch.is_empty() {
            return Err(Fail::environment(format!(
                "{GIT} status did not report a branch header · expected `--porcelain=v2 --branch`"
            )));
        }
        Ok(status)
    }

    pub fn oid(&self) -> Option<&String> {
        self.oid.as_ref()
    }

    pub fn branch(&self) -> String {
        match self.branch.as_str() {
            "(detached)" => "detached HEAD".to_string(),
            other => other.to_string(),
        }
    }

    pub fn upstream(&self) -> &str {
        &self.upstream
    }

    pub fn lines(&self) -> Vec<String> {
        self.entries.iter().map(Entry::line).collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every path this status mentions, a rename contributing both of its names.
    pub fn paths(&self) -> Vec<String> {
        self.entries
            .iter()
            .flat_map(|entry| entry.names().cloned())
            .collect()
    }

    /// How many *tracked* paths differ from HEAD — the count of what a commit does not contain.
    /// Untracked is excluded: an untracked file has no version in HEAD to have changed from, and a
    /// repository with a directory of half-written notes would otherwise carry a caveat forever.
    pub fn tracked_changes(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.code != "??")
            .count()
    }

    /// Whether any *tracked* path has changed. Untracked is the one kind a diff between commits
    /// cannot show, so a worktree of nothing but untracked files has no diff to offer — which is
    /// why `state`'s exit is conditional rather than unconditional.
    pub fn tracks_changes(&self) -> bool {
        self.tracked_changes() > 0
    }

    pub fn unparsed(&self) -> usize {
        self.unparsed
    }

    /// Untracked names ending in `/` are directories git did not expand. Counted so a bound can say
    /// the list is shorter than the change set.
    pub fn collapsed_directories(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.code == "??" && entry.path.ends_with('/'))
            .count()
    }
}

/// Split a `1`, `2` or `u` record: the status code, then the machine fields, then the path as the
/// remainder of the record — so a path containing a space survives, which a plain split would lose.
fn split_record(rest: &str, fields: usize) -> Option<(String, String)> {
    let mut parts = rest.splitn(fields, ' ');
    let code = parts.next()?.to_string();
    let path = parts.nth(fields - 2)?.to_string();
    Some((code, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_modified_record_keeps_a_path_with_spaces() {
        let record = "M. N... 100644 100644 100644 aaaa bbbb src/a b.rs";
        let (code, path) = split_record(record, 8).expect("a 1 record");
        assert_eq!(code, "M.");
        assert_eq!(path, "src/a b.rs");
    }

    #[test]
    fn the_short_form_is_the_one_a_reader_recognises() {
        assert_eq!(short_code("M."), "M ");
        assert_eq!(short_code(".M"), " M");
        assert_eq!(short_code("MM"), "MM");
        assert_eq!(short_code("??"), "??");
    }

    #[test]
    fn a_branch_header_is_required() {
        // Absence here means the output was not what was asked for, and an empty branch would
        // render as a repository with no name.
        assert!(Status::parse("1 .M N... 100644 100644 100644 a b x.rs\0").is_err());
    }

    #[test]
    fn an_unborn_branch_has_no_oid() {
        let raw = "# branch.oid (initial)\0# branch.head main\0";
        let status = Status::parse(raw).expect("a status");
        assert_eq!(status.oid(), None);
        assert_eq!(status.branch(), "main");
    }

    #[test]
    fn a_rename_contributes_both_names() {
        let raw = "# branch.oid abc\0# branch.head main\0\
                   2 R. N... 100644 100644 100644 aaaa bbbb R100 new.rs\0old.rs\0";
        let status = Status::parse(raw).expect("a status");
        assert_eq!(status.paths(), vec!["new.rs", "old.rs"]);
    }

    #[test]
    fn an_unmerged_record_is_counted_rather_than_mistaken_for_a_nothing() {
        // The field counts differ per record type, and a wrong one silently drops the record —
        // which is a conflict an agent would not be told about.
        let raw = "# branch.oid abc\0# branch.head main\0\
                   u UU N... 100644 100644 100644 100644 aaaa bbbb cccc conflict.rs\0";
        let status = Status::parse(raw).expect("a status");
        assert_eq!(status.lines(), vec!["UU  conflict.rs"]);
        assert_eq!(status.unparsed(), 0);
    }

    #[test]
    fn a_record_that_does_not_parse_is_counted() {
        let raw = "# branch.oid abc\0# branch.head main\0\n1 truncated\0";
        let status = Status::parse(raw).expect("a status");
        assert_eq!(status.unparsed(), 1);
    }
}
