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
use crate::verbs::{plural, read_text, Content, Opts, Outcome};
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

pub fn run(paths: &[String], opts: &Opts, mode: Mode) -> Result<Outcome, Fail> {
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

    let listed = match mode {
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
            let hits = rows.iter().map(|(_, hits)| hits.to_string().len()).max().unwrap_or(1);
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
    report.bound(coverage(mode, listed, &counted, total, opts.limit));

    Ok(Outcome::from_report(report))
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

fn coverage(
    mode: Mode,
    listed: usize,
    counted: &[(String, usize)],
    total: usize,
    limit: usize,
) -> String {
    let files = plural(counted.len(), "file");
    let hits = matches(total);
    match mode {
        Mode::Matches => {
            if listed < total {
                format!("{listed} of {hits} in {files} · --limit {limit}")
            } else {
                format!("{hits} in {files}")
            }
        }
        Mode::Count => {
            if listed < counted.len() {
                format!(
                    "{hits} in {files} · {listed} of {} files shown (--limit {limit})",
                    counted.len()
                )
            } else {
                format!("{hits} in {files}")
            }
        }
        Mode::FilesOnly => {
            if listed < counted.len() {
                format!(
                    "{files} · {hits} · {listed} of {} shown (--limit {limit})",
                    counted.len()
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
