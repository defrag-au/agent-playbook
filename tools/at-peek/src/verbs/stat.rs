//! `at-peek stat` — how big is it, and when did it change.
//!
//! The verb for "did the write land", which is the question an agent asks after every edit and
//! which otherwise costs a `wc -l` and a `ls -l`. A binary file is reported as binary rather
//! than counted, because a line count for one is a number that means nothing.

use crate::contract::{iso8601_utc, Fail, Report, MAX_LIMIT};
use crate::verbs::{again, open, plural, Opts, Outcome};
use crate::TOOL;

pub fn run(targets: &[String], opts: &Opts) -> Result<Outcome, Fail> {
    let mut report = Report::new();
    if let Some(asked) = opts.limit_clamped_from {
        report.bound(format!("--limit {asked} clamped to {MAX_LIMIT}"));
    }
    report.header(
        TOOL,
        "stat",
        opts.root.name(),
        &plural(targets.len(), "path"),
    );

    let shown = targets.len().min(opts.limit);
    let mut rows: Vec<Row> = Vec::with_capacity(shown);
    for arg in targets.iter().take(opts.limit) {
        let opened = open(opts, arg)?;
        let (lines, kind) = match opened.binary {
            Some(_) => ("-".to_string(), "binary".to_string()),
            None => (
                opened.lines().to_string(),
                language(&opened.rel).to_string(),
            ),
        };
        rows.push(Row {
            rel: opened.rel,
            lines,
            bytes: format!("{} B", opened.len),
            kind,
            modified: opened
                .modified
                .map(iso8601_utc)
                .unwrap_or_else(|| "-".to_string()),
        });
    }

    let width = Widths::of(&rows);
    for row in &rows {
        report.content(width.row(row));
    }

    report.bound(if shown == targets.len() {
        plural(targets.len(), "path")
    } else {
        format!(
            "{shown} of {} · --limit {}",
            plural(targets.len(), "path"),
            opts.limit
        )
    });
    if shown < targets.len() {
        report.next(
            again(
                opts,
                "stat",
                targets,
                &[format!("--limit {}", targets.len().min(MAX_LIMIT))],
            ),
            format!("all {}", plural(targets.len(), "path")),
        );
    }

    Ok(Outcome::from_report(report))
}

struct Row {
    rel: String,
    lines: String,
    bytes: String,
    kind: String,
    modified: String,
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
    fn of(rows: &[Row]) -> Widths {
        let mut width = Widths {
            rel: 0,
            lines: 1,
            bytes: 0,
            kind: 4,
        };
        for row in rows {
            width.rel = width.rel.max(row.rel.len());
            width.lines = width.lines.max(row.lines.len());
            width.bytes = width.bytes.max(row.bytes.len());
            width.kind = width.kind.max(row.kind.len());
        }
        width
    }

    fn row(&self, row: &Row) -> String {
        format!(
            "{rel:<relw$}  {lines:>linew$} lines  {bytes:>bytesw$}  {kind:<kindw$}  {modified}",
            rel = row.rel,
            lines = row.lines,
            bytes = row.bytes,
            kind = row.kind,
            modified = row.modified,
            relw = self.rel,
            linew = self.lines,
            bytesw = self.bytes,
            kindw = self.kind,
        )
    }
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
