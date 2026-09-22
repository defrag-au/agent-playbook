//! `at-peek search` — every mention, bounded, with the true total attached.
//!
//! The verb that displaces `rg | head`, and it displaces it on two things a pipeline cannot
//! do: it prints the real number of matches beneath the ones it showed, and it walks a
//! directory itself, so a question that would otherwise need a loop needs no loop.
//!
//! Matches are counted over every file the walk considers, even after the listing is full.
//! That is the point — `# 50 of 143` is the difference between a fact and a guess — and it is
//! why the walk is bounded and the bound is stated.

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

#[derive(Clone, Copy, PartialEq, Eq)]
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
    counted: &'a [(String, usize)],
    walk: &'a Walk,
}

pub fn run(paths: &[String], opts: &Opts, mode: Mode) -> Result<Outcome, Fail> {
    let Some((pattern, named)) = paths.split_first() else {
        return Err(Fail::usage(
            "`search` needs a pattern · at-peek search <pattern> [path…]",
        ));
    };
    let query = Query {
        pattern, // `&String` derefs to the `&str` an exit is built from
        named,
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

    let mut counted: Vec<(String, usize)> = Vec::new();
    let mut shown: Vec<(String, usize, String)> = Vec::new();
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
            if mode == Mode::Matches && shown.len() < opts.limit {
                let (rendered, cut) = truncate(line);
                if cut {
                    truncated_lines += 1;
                }
                shown.push((rel.clone(), index + 1, rendered));
            }
        }
        if hits > 0 {
            counted.push((rel, hits));
        }
    }

    let mut report = Report::new();
    if let Some(asked) = opts.limit_clamped_from {
        report.bound(format!("--limit {asked} clamped to {MAX_LIMIT}"));
    }
    if let Some(asked) = opts.max_files_clamped_from {
        report.bound(format!("--max-files {asked} clamped to {MAX_FILES}"));
    }

    let scope = match named.len() {
        0 => ".".to_string(),
        1 => named[0].clone(),
        n => plural(n, "path"),
    };
    report.header(
        TOOL,
        "search",
        opts.root.name(),
        &format!("/{pattern}/ in {scope}"),
    );

    // The rows, unless the frame was asked for. `--summary` is the coarse answer again: how many
    // matches, where they are not, and where the walk stopped.
    let listed = if opts.summary {
        match mode {
            Mode::Matches => total,
            Mode::Count | Mode::FilesOnly => counted.len(),
        }
    } else {
        match mode {
            Mode::Matches => {
                for (rel, line, text) in &shown {
                    report.content(format!("{rel}:{line}:{text}"));
                }
                shown.len()
            }
            Mode::Count => {
                // Both columns are padded, so a column of counts can be compared by eye. The
                // widths come from the rows actually listed, so a limit does not leave a gap on
                // every line.
                let rows = &counted[..counted.len().min(opts.limit)];
                let paths = rows.iter().map(|(rel, _)| rel.len()).max().unwrap_or(0);
                let hits = rows
                    .iter()
                    .map(|(_, hits)| hits.to_string().len())
                    .max()
                    .unwrap_or(1);
                for (rel, count) in rows {
                    report.content(format!("{rel:<paths$}  {count:>hits$}"));
                }
                rows.len()
            }
            Mode::FilesOnly => {
                for (rel, _) in counted.iter().take(opts.limit) {
                    report.content(rel.clone());
                }
                counted.len().min(opts.limit)
            }
        }
    };

    report.bound(walk_bound(&walk));
    if let Some(skipped) = skip_bound(
        secret_skipped,
        binary,
        not_utf8,
        walk.symlinks,
        walk.unreadable,
    ) {
        report.bound(skipped);
    }
    if walk.capped {
        report.bound(format!(
            "stopped after {} files considered (--max-files {})",
            walk.considered, opts.max_files
        ));
    }
    if truncated_lines > 0 {
        report.bound(format!(
            "caveat: {truncated_lines} line(s) wider than {MAX_LINE_WIDTH} characters, truncated"
        ));
    }
    let found = Findings {
        listed,
        total,
        counted: &counted,
        walk: &walk,
    };
    let mut coverage = coverage(mode, &found, opts.limit);
    if opts.summary && listed > 0 {
        coverage.push_str(", not shown");
    }
    report.bound(coverage);
    exits(&mut report, opts, mode, &query, &found);

    // A summary found the matches it is counting without printing them, so the frame is the answer.
    // With nothing found there is no frame to be the answer to, and the honest code is the ordinary
    // "nothing to show".
    Ok(if opts.summary && listed > 0 {
        Outcome::from_frame(report)
    } else {
        Outcome::from_report(report)
    })
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

fn walk_bound(walk: &Walk) -> String {
    let mut text = format!("walked {}", plural(walk.considered, "file"));
    if !walk.skipped_dirs.is_empty() {
        let names: Vec<&str> = walk.skipped_dirs.iter().copied().collect();
        text.push_str(&format!(" · skipped by rule: {}", names.join(", ")));
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
