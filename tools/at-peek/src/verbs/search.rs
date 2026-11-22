//! `at-peek search` — every mention, bounded, with the true total attached.
//!
//! The verb that displaces `rg | head`, and it displaces it on two things a pipeline cannot
//! do: it prints the real number of matches beneath the ones it showed, and it walks a
//! directory itself, so a question that would otherwise need a loop needs no loop.
//!
//! Matches are counted over every file the walk considers, even after the listing is full.
//! That is the point — `# 50 of 143` is the difference between a fact and a guess — and it is
//! why the walk is bounded and the bound is stated.
//!
//! The answer is a [`SearchFacts`] value. [`run`] renders it for the CLI; a harness calls
//! [`collect`] and keeps the facts, which is what lets the same read be typed rather than
//! scraped from stdout.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

use crate::contract::{
    secret_shaped, truncate, Fail, Report, MAX_FILES, MAX_LIMIT, MAX_LINE_WIDTH,
};
use crate::verbs::{again, plural, read_text, Content, Opts, Outcome};
use crate::TOOL;

/// Directory names the walk refuses outright. Not gitignore semantics — the output names what
/// it skipped — just the names that would otherwise turn a search of a repository into a walk
/// of a build tree. Real ignore handling is `at-peek tree`'s job, and it needs a dependency
/// this one has not taken.
const SKIP_DIRS: &[&str] = &[
    ".direnv",
    ".git",
    ".tmp",
    "dist",
    "node_modules",
    "result",
    "target",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Mode {
    Matches,
    Count,
    FilesOnly,
}

impl Mode {
    /// `--count` and `--files-only` ask different questions, so asking both is a usage error
    /// rather than a precedence rule nobody can remember.
    pub fn of(count: bool, files_only: bool) -> Result<Mode, Fail> {
        match (count, files_only) {
            (true, true) => Err(Fail::usage(
                "--count and --files-only ask different questions · pass one",
            )),
            (true, false) => Ok(Mode::Count),
            (false, true) => Ok(Mode::FilesOnly),
            (false, false) => Ok(Mode::Matches),
        }
    }
}

/// One matching line, as the walk found it. The text is the line itself, not a truncated copy:
/// the width cap is a rendering decision, and a caller that wants the line should get it whole.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Found {
    /// Relative to the root, forward slashes — the form a transcript cites.
    pub path: String,
    /// 1-based, as a reader counts.
    pub line: usize,
    pub text: String,
}

/// One file's share of the matches.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FileHits {
    pub path: String,
    pub hits: usize,
}

/// What the walk did, without the files themselves.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WalkFacts {
    pub considered: usize,
    pub capped: bool,
    pub skipped_dirs: Vec<String>,
    pub symlinks: usize,
    pub unreadable: usize,
}

/// The answer to a search, before it is a report.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SearchFacts {
    pub pattern: String,
    /// The paths named after the pattern, in the order given; empty means the whole root.
    pub named: Vec<String>,
    pub mode: Mode,
    /// The lines the limit left room for, in path order. Empty for the counting modes.
    pub rows: Vec<Found>,
    /// Every file with at least one match, in path order.
    pub counted: Vec<FileHits>,
    /// Matches over every file the walk considered, not only the listed ones.
    pub total: usize,
    pub walk: WalkFacts,
    pub binary: usize,
    pub not_utf8: usize,
    pub secret_skipped: usize,
    /// Lines over the width cap. Kept here rather than counted while rendering because the
    /// bound is part of the answer, and `--summary` states it even though it prints no rows.
    pub truncated_lines: usize,
}

impl SearchFacts {
    /// The rows this answer lists: the body itself, or the count a summary uses instead.
    fn listed(&self, opts: &Opts) -> usize {
        if opts.summary {
            match self.mode {
                Mode::Matches => self.total,
                Mode::Count | Mode::FilesOnly => self.counted.len(),
            }
        } else {
            match self.mode {
                Mode::Matches => self.rows.len(),
                Mode::Count | Mode::FilesOnly => self.counted.len().min(opts.limit),
            }
        }
    }

    /// The contract's report: what the CLI prints, and what a harness returns as tool text.
    pub fn report(&self, opts: &Opts) -> Report {
        let mut report = Report::new();
        if let Some(asked) = opts.limit_clamped_from {
            report.bound(format!("--limit {asked} clamped to {MAX_LIMIT}"));
        }
        if let Some(asked) = opts.max_files_clamped_from {
            report.bound(format!("--max-files {asked} clamped to {MAX_FILES}"));
        }

        let scope = match self.named.len() {
            0 => ".".to_string(),
            1 => self.named[0].clone(),
            n => plural(n, "path"),
        };
        report.header(
            TOOL,
            "search",
            opts.root.name(),
            &format!("/{}/ in {scope}", self.pattern),
        );

        // The rows, unless the frame was asked for. `--summary` is the coarse answer again: how
        // many matches, where they are not, and where the walk stopped.
        let listed = if opts.summary {
            self.listed(opts)
        } else {
            match self.mode {
                Mode::Matches => {
                    for row in &self.rows {
                        let (text, _) = truncate(&row.text);
                        report.content(format!("{}:{}:{}", row.path, row.line, text));
                    }
                    self.rows.len()
                }
                Mode::Count => {
                    // Both columns are padded, so a column of counts can be compared by eye. The
                    // widths come from the rows actually listed, so a limit does not leave a gap on
                    // every line.
                    let rows = &self.counted[..self.counted.len().min(opts.limit)];
                    let paths = rows.iter().map(|row| row.path.len()).max().unwrap_or(0);
                    let hits = rows
                        .iter()
                        .map(|row| row.hits.to_string().len())
                        .max()
                        .unwrap_or(1);
                    for row in rows {
                        report.content(format!("{:<paths$}  {:>hits$}", row.path, row.hits));
                    }
                    rows.len()
                }
                Mode::FilesOnly => {
                    for row in self.counted.iter().take(opts.limit) {
                        report.content(row.path.clone());
                    }
                    self.counted.len().min(opts.limit)
                }
            }
        };

        report.bound(walk_bound(&self.walk));
        if let Some(skipped) = skip_bound(
            self.secret_skipped,
            self.binary,
            self.not_utf8,
            self.walk.symlinks,
            self.walk.unreadable,
        ) {
            report.bound(skipped);
        }
        if self.walk.capped {
            report.bound(format!(
                "stopped after {} files considered (--max-files {})",
                self.walk.considered, opts.max_files
            ));
        }
        if self.truncated_lines > 0 {
            report.bound(format!(
                "caveat: {} line(s) wider than {MAX_LINE_WIDTH} characters, truncated",
                self.truncated_lines
            ));
        }
        let found = Findings {
            listed,
            total: self.total,
            counted: &self.counted,
            walk: &self.walk,
        };
        let mut coverage = coverage(self.mode, &found, opts.limit);
        if opts.summary && listed > 0 {
            coverage.push_str(", not shown");
        }
        report.bound(coverage);
        exits(
            &mut report,
            opts,
            self.mode,
            &Query {
                pattern: &self.pattern,
                named: &self.named,
            },
            &found,
        );
        report
    }

    /// The report, with the exit the answer implies. A summary that found something is a frame
    /// rather than an empty success: the count is the answer even though no rows were printed.
    pub fn outcome(&self, opts: &Opts) -> Outcome {
        let report = self.report(opts);
        if opts.summary && self.listed(opts) > 0 {
            Outcome::from_frame(report)
        } else {
            Outcome::from_report(report)
        }
    }
}

#[derive(Default)]
struct Walk {
    /// A set, so a path named twice is searched once and results come out in path order
    /// however the arguments were ordered.
    files: BTreeSet<PathBuf>,
    considered: usize,
    capped: bool,
    skipped_dirs: BTreeSet<&'static str>,
    symlinks: usize,
    unreadable: usize,
}

impl Walk {
    fn facts(&self) -> WalkFacts {
        WalkFacts {
            considered: self.considered,
            capped: self.capped,
            skipped_dirs: self
                .skipped_dirs
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
            symlinks: self.symlinks,
            unreadable: self.unreadable,
        }
    }
}

/// What was asked: the pattern, and the paths named after it. Kept together because an exit has to
/// repeat both to be the same question — a widen that dropped a named path would search somewhere
/// else and call it the same read.
struct Query<'a> {
    pattern: &'a str,
    named: &'a [String],
}

/// What the walk found, as the numbers a bound and an exit are written from. Grouped because they
/// are read together and mean nothing apart: `listed` is rows printed, `total` is matches over every
/// file considered, and the two differ only by the limit.
struct Findings<'a> {
    listed: usize,
    total: usize,
    counted: &'a [FileHits],
    walk: &'a WalkFacts,
}

/// The typed answer. No printing: a caller that runs in-process owns its own stdout.
pub fn collect(paths: &[String], opts: &Opts, mode: Mode) -> Result<SearchFacts, Fail> {
    let Some((pattern, named)) = paths.split_first() else {
        return Err(Fail::usage(
            "`search` needs a pattern · at-peek search <pattern> [path…]",
        ));
    };
    let regex = Regex::new(pattern).map_err(|e| {
        Fail::usage(format!(
            "`{pattern}` is not a valid regex: {}",
            complaint(&e)
        ))
    })?;

    let mut walk = Walk::default();
    if named.is_empty() {
        descend(opts.root.dir(), opts, &mut walk);
    } else {
        for arg in named {
            let abs = opts.root.resolve(arg)?;
            match fs::metadata(&abs) {
                Ok(meta) if meta.is_dir() => descend(&abs, opts, &mut walk),
                Ok(_) => {
                    if walk.files.insert(abs) {
                        walk.considered += 1;
                    }
                }
                Err(e) => return Err(Fail::environment(format!("cannot read {arg}: {e}"))),
            }
        }
    }

    let mut counted: Vec<FileHits> = Vec::new();
    let mut rows: Vec<Found> = Vec::new();
    let mut total = 0;
    let mut truncated_lines = 0;
    let mut secret_skipped = 0;
    let mut binary = 0;
    let mut not_utf8 = 0;

    for path in &walk.files {
        let rel = opts.root.relative(path);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !opts.include_secret_paths && secret_shaped(&name).is_some() {
            secret_skipped += 1;
            continue;
        }

        let text = match read_text(path, &rel) {
            Ok(Content::Text(text)) => text,
            Ok(Content::Binary(_)) => {
                binary += 1;
                continue;
            }
            Ok(Content::NotUtf8(_)) => {
                not_utf8 += 1;
                continue;
            }
            Err(_) => {
                walk.unreadable += 1;
                continue;
            }
        };

        let mut hits = 0;
        for (index, line) in text.lines().enumerate() {
            if !regex.is_match(line) {
                continue;
            }
            hits += 1;
            total += 1;
            if mode == Mode::Matches && rows.len() < opts.limit {
                if truncate(line).1 {
                    truncated_lines += 1;
                }
                rows.push(Found {
                    path: rel.clone(),
                    line: index + 1,
                    text: line.to_string(),
                });
            }
        }
        if hits > 0 {
            counted.push(FileHits { path: rel, hits });
        }
    }

    Ok(SearchFacts {
        pattern: pattern.clone(),
        named: named.to_vec(),
        mode,
        rows,
        counted,
        total,
        walk: walk.facts(),
        binary,
        not_utf8,
        secret_skipped,
        truncated_lines,
    })
}

/// The CLI's entry point: collect, render, and answer with the exit the report implies.
pub fn run(paths: &[String], opts: &Opts, mode: Mode) -> Result<Outcome, Fail> {
    collect(paths, opts, mode).map(|facts| facts.outcome(opts))
}

/// The exits: the body a summary withheld, or a listing the limit cut, then a walk that stopped.
///
/// The mode flag travels with the exit, because `--count` and `--files-only` are different answers
/// to the same question and a widen that quietly returned the other one would not be the same read.
/// A walk that stopped has no count to name, so the suggested ceiling is twice the one that
/// stopped it — stated in the reason, because a number nobody can derive should say where it came
/// from.
fn exits(report: &mut Report, opts: &Opts, mode: Mode, query: &Query, found: &Findings) {
    let mut targets: Vec<String> = vec![query.pattern.to_string()];
    targets.extend(query.named.iter().cloned());
    let shape: Vec<String> = match mode {
        Mode::Matches => Vec::new(),
        Mode::Count => vec!["--count".to_string()],
        Mode::FilesOnly => vec!["--files-only".to_string()],
    };

    let rows = match mode {
        Mode::Matches => found.total,
        Mode::Count | Mode::FilesOnly => found.counted.len(),
    };
    // The rows are matches in one mode and files in the others, and a widen that promised the wrong
    // noun would be describing a different answer.
    let all = match mode {
        Mode::Matches => format!("all {}", matches(rows)),
        Mode::Count | Mode::FilesOnly => format!("all {}", plural(rows, "file")),
    };

    if opts.summary {
        // The frame withheld the rows, so the rows are what it offers; a widen would say nothing,
        // because the frame already counted everything there is.
        if rows > 0 {
            let mut flags = shape.clone();
            flags.push(format!("--limit {}", rows.min(MAX_LIMIT)));
            report.next(again(opts, "search", &targets, &flags), all);
        }
    } else if found.listed < rows {
        let mut flags = shape.clone();
        flags.push(format!("--limit {}", rows.min(MAX_LIMIT)));
        report.next(again(opts, "search", &targets, &flags), all);
    }

    if found.walk.capped {
        let mut flags = shape;
        flags.push(format!(
            "--max-files {}",
            opts.max_files.saturating_mul(2).min(MAX_FILES)
        ));
        report.next(again(opts, "search", &targets, &flags), "twice the walk");
    }
}

/// Depth-first, one directory at a time, in sorted order, so the same tree always produces the
/// same output. Failures are counted rather than fatal: an unreadable directory in a repository
/// should not stop a search of the rest of it.
fn descend(dir: &Path, opts: &Opts, walk: &mut Walk) {
    let Ok(entries) = fs::read_dir(dir) else {
        walk.unreadable += 1;
        return;
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .collect();
    paths.sort();

    for path in paths {
        if walk.considered >= opts.max_files {
            walk.capped = true;
            return;
        }
        let Ok(meta) = fs::symlink_metadata(&path) else {
            walk.unreadable += 1;
            continue;
        };
        if meta.file_type().is_symlink() {
            walk.symlinks += 1;
            continue;
        }
        if meta.is_dir() {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            match SKIP_DIRS.iter().find(|skip| **skip == name) {
                Some(skip) => {
                    walk.skipped_dirs.insert(skip);
                }
                None => descend(&path, opts, walk),
            }
        } else if meta.is_file() && walk.files.insert(path) {
            walk.considered += 1;
        }
    }
}

fn walk_bound(walk: &WalkFacts) -> String {
    let mut text = format!("walked {}", plural(walk.considered, "file"));
    if !walk.skipped_dirs.is_empty() {
        text.push_str(&format!(
            " · skipped by rule: {}",
            walk.skipped_dirs.join(", ")
        ));
    }
    text
}

fn skip_bound(
    secret: usize,
    binary: usize,
    not_utf8: usize,
    symlinks: usize,
    unreadable: usize,
) -> Option<String> {
    let mut parts = Vec::new();
    if secret > 0 {
        parts.push(format!("{secret} secret-shaped"));
    }
    if binary > 0 {
        parts.push(format!("{binary} binary"));
    }
    if not_utf8 > 0 {
        parts.push(format!("{not_utf8} not UTF-8"));
    }
    if symlinks > 0 {
        parts.push(format!("{symlinks} symbolic link(s) not followed"));
    }
    if unreadable > 0 {
        parts.push(format!("{unreadable} unreadable"));
    }
    if parts.is_empty() {
        None
    } else {
        Some(format!("skipped: {}", parts.join(" · ")))
    }
}

fn coverage(mode: Mode, found: &Findings, limit: usize) -> String {
    let files = plural(found.counted.len(), "file");
    let hits = matches(found.total);
    let rows = found.counted.len();
    match mode {
        Mode::Matches => {
            if found.listed < found.total {
                format!("{} of {hits} in {files} · --limit {limit}", found.listed)
            } else {
                format!("{hits} in {files}")
            }
        }
        Mode::Count => {
            if found.listed < rows {
                format!(
                    "{hits} in {files} · {} of {rows} files shown (--limit {limit})",
                    found.listed
                )
            } else {
                format!("{hits} in {files}")
            }
        }
        Mode::FilesOnly => {
            if found.listed < rows {
                format!(
                    "{files} · {hits} · {} of {rows} shown (--limit {limit})",
                    found.listed
                )
            } else {
                format!("{files} · {hits}")
            }
        }
    }
}

/// `plural` adds an `s`; "match" needs `es`, and a tool that prints "2 matchs" reads like one
/// nobody ran.
fn matches(count: usize) -> String {
    if count == 1 {
        "1 match".to_string()
    } else {
        format!("{count} matches")
    }
}

/// The regex crate reports a parse error as a four-line diagram. A usage message can carry one
/// line, and its last line is the part that says what is actually wrong.
fn complaint(error: &regex::Error) -> String {
    error
        .to_string()
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("invalid pattern")
        .trim()
        .trim_start_matches("error: ")
        .to_string()
}
