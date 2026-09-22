//! The reads two or more verbs need, as structs.
//!
//! This module exists for one property: **a recipe composes the same reads its sibling verbs use**,
//! so `pr`'s commit count and `log`'s commit count cannot disagree, and `pr`'s diffstat is `diff`'s
//! diffstat. A recipe that orchestrated the other verbs as *processes* would be re-parsing their
//! text — the pipe problem in a nicer suit — and it would need the union of every tier's grant.
//!
//! What lives here is git-shaped: a read, the arguments it needs, and the struct it fills. What does
//! not live here is any rendering decision that belongs to one verb — a section's bound, or whether
//! a count is worth printing at all.

use at_core::contract::Fail;

use crate::git::{with_paths, Git, Noun};

/// What every `diff` run carries, whatever the caller asked for. `--no-color` because a repository
/// can configure colour on and this output is read by a machine; `--no-ext-diff` and `--no-textconv`
/// because both names a program for git to run, and this tool runs one program only.
const NEUTRAL: &[&str] = &["--no-color", "--no-ext-diff", "--no-textconv"];

/// `--ignore-space`: compare lines ignoring whitespace, which is `git diff -w`.
const IGNORE_SPACE: &str = "--ignore-all-space";

/// `--format` for one line per commit: `%s` is a single line by construction, so a commit is a line
/// and the separator can be a byte no subject contains.
const FORMAT: &str = "--format=%h%x1f%as%x1f%an%x1f%s";

const SEPARATOR: char = '\u{1f}';

// ---------------------------------------------------------------------------------------------
// What is different
// ---------------------------------------------------------------------------------------------

/// One line of `--numstat`.
pub struct FileStat {
    /// `None` for a binary file: git prints `-` where a count would be, and a line count for a
    /// binary file is a number that means nothing.
    pub added: Option<u64>,
    pub deleted: Option<u64>,
    /// The path as git wrote it, which for a rename is the factored `old => new` form.
    pub label: String,
    /// The one or two real names behind `label`, because a rename is one line with two paths and the
    /// deny-list has to see both.
    pub names: Vec<String>,
}

impl FileStat {
    /// The first column: the counts, or the word for a file that has none.
    pub fn counts(&self) -> String {
        match (self.added, self.deleted) {
            (Some(added), Some(deleted)) => format!("+{added} -{deleted}"),
            _ => "binary".to_string(),
        }
    }

    pub fn is_binary(&self) -> bool {
        self.added.is_none()
    }
}

/// One `--numstat` row per changed file, in git's path order.
pub fn numstat(
    git: &Git,
    rev: &Option<String>,
    paths: &[String],
    ignore_space: bool,
) -> Result<Vec<FileStat>, Fail> {
    let args = diff_args(rev, paths, &["--numstat"], ignore_space);
    let raw = git.run(Noun::Diff, &borrowed(&args))?;
    Ok(raw
        .lines()
        .filter(|line| !line.is_empty())
        .filter_map(parse_numstat)
        .collect())
}

/// The argument list for one `diff` run: the neutralising flags, the mode's own flags, the revision
/// if there is one, then `--` and the paths. Public because a verb that runs a second shape of the
/// same comparison — the hunks behind a table — must spell it exactly as the first one was spelled.
pub fn diff_args(
    rev: &Option<String>,
    paths: &[String],
    mode: &[&str],
    ignore_space: bool,
) -> Vec<String> {
    let mut args: Vec<String> = NEUTRAL
        .iter()
        .chain(mode.iter())
        .map(|flag| (*flag).to_string())
        .collect();
    if ignore_space {
        args.push(IGNORE_SPACE.to_string());
    }
    if let Some(rev) = rev {
        args.push(rev.clone());
    }
    with_paths(&mut args, paths);
    args
}

pub fn parse_numstat(line: &str) -> Option<FileStat> {
    let mut parts = line.splitn(3, '\t');
    let added = parts.next()?;
    let deleted = parts.next()?;
    let label = parts.next()?;
    Some(FileStat {
        added: added.parse().ok(),
        deleted: deleted.parse().ok(),
        label: label.to_string(),
        names: names_of(label),
    })
}

/// `src/{old.rs => new.rs}` and `old.rs => new.rs` are git's two factored rename forms, and the
/// brace form factors out a common prefix or suffix — `{old => new}/mod.rs` for a directory that
/// moved. Neither name is on a line of its own, so both are rebuilt here.
///
/// A path containing the literal text ` => ` would be read as a rename. That is accepted: the
/// misreading costs a broader refusal, never a file printed that the deny-list meant to withhold.
fn names_of(label: &str) -> Vec<String> {
    if let (Some(open), Some(close)) = (label.find('{'), label.find('}')) {
        if open < close {
            if let Some((old, new)) = label[open + 1..close].split_once(" => ") {
                let prefix = &label[..open];
                let suffix = &label[close + 1..];
                return vec![
                    format!("{prefix}{old}{suffix}"),
                    format!("{prefix}{new}{suffix}"),
                ];
            }
        }
    }
    match label.split_once(" => ") {
        Some((old, new)) => vec![old.to_string(), new.to_string()],
        None => vec![label.to_string()],
    }
}

/// `+12 -35`, and the binary count when there is one to state.
pub fn totals(files: &[FileStat]) -> String {
    let added: u64 = files.iter().filter_map(|file| file.added).sum();
    let deleted: u64 = files.iter().filter_map(|file| file.deleted).sum();
    let binary = files.iter().filter(|file| file.is_binary()).count();
    if binary == 0 {
        format!("+{added} -{deleted}")
    } else {
        format!("+{added} -{deleted} ({binary} binary)")
    }
}

// ---------------------------------------------------------------------------------------------
// What changed lately
// ---------------------------------------------------------------------------------------------

pub struct Commit {
    pub short: String,
    pub date: String,
    pub author: String,
    pub subject: String,
}

/// Commits, and how many there are in total — read together, because a listing without its true
/// total is the failure this toolkit exists to remove.
pub struct Commits {
    pub total: usize,
    pub rows: Vec<Commit>,
}

/// The commits in `rev` that touch `paths`, newest first, with the count of all of them.
///
/// The count comes from `rev-list --count` and the rows from `log`: different git commands, so the
/// claim that they agree is held by `the_count_matches_the_listing` rather than assumed.
pub fn commits(git: &Git, rev: &str, paths: &[String], limit: usize) -> Result<Commits, Fail> {
    let mut count_args: Vec<String> = vec!["--count".to_string(), rev.to_string()];
    with_paths(&mut count_args, paths);
    let raw = git.run(Noun::RevList, &borrowed(&count_args))?;
    let total = raw.trim().parse::<usize>().map_err(|_| {
        Fail::environment(format!(
            "git rev-list --count printed `{}`, which is not a number",
            raw.trim()
        ))
    })?;

    let mut args: Vec<String> = vec![
        // Explicit, because `log.showSignature` in a repository's own config would make git verify
        // the signature — a subprocess this tool did not agree to run.
        "--no-show-signature".to_string(),
        format!("--max-count={limit}"),
        FORMAT.to_string(),
        rev.to_string(),
    ];
    with_paths(&mut args, paths);
    let raw = git.run(Noun::Log, &borrowed(&args))?;

    let mut rows = Vec::new();
    for line in raw.lines().filter(|line| !line.is_empty()) {
        let fields: Vec<&str> = line.split(SEPARATOR).collect();
        match fields[..] {
            [short, date, author, subject] => rows.push(Commit {
                short: short.to_string(),
                date: date.to_string(),
                author: author.to_string(),
                subject: subject.to_string(),
            }),
            _ => {
                return Err(Fail::environment(format!(
                    "{} printed {} fields where the requested format has 4",
                    crate::git::GIT,
                    fields.len()
                )))
            }
        }
    }
    Ok(Commits { total, rows })
}

impl Commits {
    /// The rows as aligned lines, without printing them: the widths come from the rows themselves,
    /// so the output is as narrow as its data allows and identical for identical input.
    pub fn lines(&self) -> Vec<String> {
        let mut short = 1;
        let mut author = 1;
        for commit in &self.rows {
            short = short.max(commit.short.len());
            author = author.max(commit.author.len());
        }
        self.rows
            .iter()
            .map(|commit| {
                format!(
                    "{short:<shortw$}  {date}  {author:<authorw$}  {subject}",
                    short = commit.short,
                    date = commit.date,
                    author = commit.author,
                    subject = commit.subject,
                    shortw = short,
                    authorw = author,
                )
            })
            .collect()
    }
}

/// The branch's upstream as git records it (`origin/main`), or `None` when the branch tracks
/// nothing. Asked through `for-each-ref` rather than `@{upstream}` so that no `@{` appears anywhere
/// in this tool's arguments: the reflog is not read, and an argument list that cannot name it is one
/// fewer thing to audit.
pub fn upstream_of(git: &Git, branch: &str) -> Option<String> {
    let reference = format!("refs/heads/{branch}");
    git.probe(
        Noun::ForEachRef,
        &["--format=%(upstream:short)", "--count=1", &reference],
    )
    .map(|out| out.trim().to_string())
    .filter(|upstream| !upstream.is_empty())
}

pub fn borrowed(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_path_is_its_own_name() {
        assert_eq!(names_of("src/main.rs"), vec!["src/main.rs"]);
    }

    #[test]
    fn a_factored_rename_yields_both_names() {
        assert_eq!(
            names_of("src/{old.rs => new.rs}"),
            vec!["src/old.rs", "src/new.rs"]
        );
        assert_eq!(
            names_of("src/{old => new}/mod.rs"),
            vec!["src/old/mod.rs", "src/new/mod.rs"]
        );
    }

    #[test]
    fn an_unfactored_rename_yields_both_names() {
        assert_eq!(names_of("a.rs => b/c.rs"), vec!["a.rs", "b/c.rs"]);
    }

    #[test]
    fn a_binary_file_has_no_counts() {
        let file = parse_numstat("-\t-\tassets/logo.png").expect("a numstat line");
        assert_eq!(file.counts(), "binary");
        assert!(file.is_binary());
    }

    #[test]
    fn counts_and_path_survive_a_path_with_a_tab_after_it() {
        let file = parse_numstat("12\t3\tsrc/a weird name.rs").expect("a numstat line");
        assert_eq!(file.counts(), "+12 -3");
        assert_eq!(file.label, "src/a weird name.rs");
    }

    #[test]
    fn totals_state_the_binary_count() {
        let files = vec![
            FileStat {
                added: Some(12),
                deleted: Some(3),
                label: "a.rs".to_string(),
                names: vec!["a.rs".to_string()],
            },
            FileStat {
                added: None,
                deleted: None,
                label: "b.png".to_string(),
                names: vec!["b.png".to_string()],
            },
        ];
        assert_eq!(totals(&files), "+12 -3 (1 binary)");
    }
}
