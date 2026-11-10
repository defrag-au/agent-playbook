//! `at-peek stat` — how big is it, and when did it change.
//!
//! The verb for "did the write land", which is the question an agent asks after every edit and
//! which otherwise costs a `wc -l` and a `ls -l`. A binary file is reported as binary rather
//! than counted, because a line count for one is a number that means nothing.
//!
//! The answer is a [`StatFacts`] value. [`collect`] reads it; [`StatFacts::report`] renders the
//! CLI's bytes. A harness calls `collect` and keeps the facts rather than parsing the table.

use crate::contract::{iso8601_utc, Fail, Report, MAX_LIMIT};
use crate::verbs::{again, open, plural, Opts, Outcome};
use crate::TOOL;

/// One file's row.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StatRow {
    /// Relative to the root, forward slashes.
    pub path: String,
    /// `None` for a binary file: a line count for one is a number that means nothing.
    pub lines: Option<usize>,
    pub bytes: u64,
    /// The language from the extension, or `binary`.
    pub kind: String,
    /// ISO 8601 UTC, when the filesystem reports a modification time.
    pub modified: Option<String>,
}

/// The answer to a stat, before it is a report.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StatFacts {
    pub rows: Vec<StatRow>,
    /// The paths the caller asked about, in the order given, including those the limit left out.
    pub targets: Vec<String>,
}

pub fn collect(targets: &[String], opts: &Opts) -> Result<StatFacts, Fail> {
    let shown = targets.len().min(opts.limit);
    let mut rows: Vec<StatRow> = Vec::with_capacity(shown);
    for arg in targets.iter().take(opts.limit) {
        let opened = open(opts, arg)?;
        let (lines, kind) = match opened.binary {
            Some(_) => (None, "binary".to_string()),
            None => (Some(opened.lines()), language(&opened.rel).to_string()),
        };
        rows.push(StatRow {
            path: opened.rel,
            lines,
            bytes: opened.len,
            kind,
            modified: opened.modified.map(iso8601_utc),
        });
    }
    Ok(StatFacts {
        rows,
        targets: targets.to_vec(),
    })
}

impl StatFacts {
    /// Render the CLI's answer.
    pub fn report(&self, opts: &Opts) -> Outcome {
        let mut report = Report::new();
        if let Some(asked) = opts.limit_clamped_from {
            report.bound(format!("--limit {asked} clamped to {MAX_LIMIT}"));
        }
        let total = self.targets.len();
        report.header(TOOL, "stat", opts.root.name(), &plural(total, "path"));

        let width = Widths::of(&self.rows);
        for row in &self.rows {
            report.content(width.row(row));
        }

        let shown = self.rows.len();
        report.bound(if shown == total {
            plural(total, "path")
        } else {
            format!(
                "{shown} of {} · --limit {}",
                plural(total, "path"),
                opts.limit
            )
        });
        if shown < total {
            report.next(
                again(
                    opts,
                    "stat",
                    &self.targets,
                    &[format!("--limit {}", total.min(MAX_LIMIT))],
                ),
                format!("all {}", plural(total, "path")),
            );
        }

        Outcome::from_report(report)
    }
}

/// Column widths, computed from the rows so the output is as narrow as the data allows and
/// identical for identical input.
struct Widths {
    rel: usize,
    lines: usize,
    bytes: usize,
    kind: usize,
}

impl Widths {
    fn of(rows: &[StatRow]) -> Widths {
        let mut width = Widths {
            rel: 0,
            lines: 1,
            bytes: 0,
            kind: 4,
        };
        for row in rows {
            width.rel = width.rel.max(row.path.len());
            width.lines = width.lines.max(lines_text(row).len());
            width.bytes = width.bytes.max(bytes_text(row).len());
            width.kind = width.kind.max(row.kind.len());
        }
        width
    }

    fn row(&self, row: &StatRow) -> String {
        format!(
            "{rel:<relw$}  {lines:>linew$} lines  {bytes:>bytesw$}  {kind:<kindw$}  {modified}",
            rel = row.path,
            lines = lines_text(row),
            bytes = bytes_text(row),
            kind = row.kind,
            modified = row.modified.as_deref().unwrap_or("-"),
            relw = self.rel,
            linew = self.lines,
            bytesw = self.bytes,
            kindw = self.kind,
        )
    }
}

fn lines_text(row: &StatRow) -> String {
    match row.lines {
        Some(lines) => lines.to_string(),
        None => "-".to_string(),
    }
}

fn bytes_text(row: &StatRow) -> String {
    format!("{} B", row.bytes)
}

/// Language from the extension. A mapping, not a claim about the file's contents — an unknown
/// extension is `-` rather than a guess.
fn language(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("rs") => "rust",
        Some("toml") => "toml",
        Some("md") => "markdown",
        Some("sh") | Some("bash") => "shell",
        Some("json") => "json",
        Some("yaml") | Some("yml") => "yaml",
        Some("nix") => "nix",
        Some("py") => "python",
        Some("ts") | Some("tsx") => "typescript",
        Some("js") | Some("jsx") => "javascript",
        Some("html") => "html",
        Some("css") => "css",
        Some("sql") => "sql",
        Some("lock") => "lock",
        Some("conf") | Some("cfg") | Some("ini") => "config",
        Some("txt") => "text",
        _ => "-",
    }
}

pub fn run(targets: &[String], opts: &Opts) -> Result<Outcome, Fail> {
    collect(targets, opts).map(|facts| facts.report(opts))
}
