//! `at-peek slice` — the lines you asked for, numbered, with the file's true size attached.
//!
//! This is the `sed -n '40,60p'` replacement, and the difference that matters is what it says
//! when it stops: `sed` prints twenty lines and says nothing about the other three hundred and
//! ninety-two, which is how a truncated read becomes a confident report. Every slice ends with
//! the file's real size and, when the limit cut it, the range that resumes.

use crate::contract::{Fail, Report, MAX_LIMIT, MAX_LINE_WIDTH};
use crate::verbs::{open, Opts, Outcome};
use crate::TOOL;

pub fn run(targets: &[String], opts: &Opts) -> Result<Outcome, Fail> {
    let mut report = Report::new();
    if let Some(asked) = opts.limit_clamped_from {
        report.bound(format!("--limit {asked} clamped to {MAX_LIMIT}"));
    }

    // One budget for the whole invocation: `--limit` bounds what a caller sees, not what each
    // target sees, so a batch of ten targets cannot print ten times the cap.
    let mut budget = opts.limit;

    for (index, arg) in targets.iter().enumerate() {
        if budget == 0 {
            report.bound(format!(
                "--limit {} reached · {} not shown",
                opts.limit,
                crate::verbs::plural(targets.len() - index, "target")
            ));
            break;
        }

        let opened = open(opts, arg)?;
        report.header(
            TOOL,
            "slice",
            opts.root.name(),
            &opened.target.describe(&opened.rel),
        );
        let rel = opened.rel.clone();

        if let Some(offset) = opened.binary {
            report.bound(format!(
                "caveat: binary file, NUL at byte {offset} · not line-addressable"
            ));
            continue;
        }

        let total = opened.lines();
        if total == 0 {
            report.bound(format!("{rel} is empty"));
            continue;
        }

        let (start, asked) = match opened.target.range {
            Some(range) => (range.start(), range.requested_lines()),
            None => (1, total),
        };
        if start > total {
            report.bound(format!(
                "line {start} is past the end of {rel} ({total} lines)"
            ));
            continue;
        }
        if start + asked - 1 > total {
            report.bound(format!(
                "the requested range ends past the file's {total} lines"
            ));
        }

        let wanted = asked.min(total - start + 1);
        let end = start + wanted - 1;
        let shown = wanted.min(budget);
        let number_width = (start + shown - 1).to_string().len();

        let mut truncated = 0;
        for (offset, line) in opened.text.lines().skip(start - 1).take(shown).enumerate() {
            let number = start + offset;
            let (text, cut) = truncate(line);
            if cut {
                truncated += 1;
            }
            report.content(format!("{number:>number_width$}  {text}"));
        }
        budget -= shown;

        report.bound(if shown == total {
            format!("{total} lines · whole file")
        } else {
            format!("{shown} of {total} lines · at-peek slice {rel}:1-{total} for the rest")
        });
        if truncated > 0 {
            report.bound(format!(
                "caveat: {truncated} line(s) wider than {MAX_LINE_WIDTH} characters, truncated"
            ));
        }
        if shown < wanted {
            let next = start + shown;
            report.bound(format!(
                "--limit {} reached · next: at-peek slice {rel}:{next}-{end}",
                opts.limit
            ));
        }
    }

    Ok(Outcome::from_report(report))
}

/// Cut a line at the width cap and say so, rather than silently shortening it. One minified
/// file in a search result is otherwise a whole context window.
fn truncate(line: &str) -> (String, bool) {
    let mut out = String::new();
    for (index, character) in line.chars().enumerate() {
        if index == MAX_LINE_WIDTH {
            let extra = line.chars().count() - index;
            out.push_str(&format!(" ...(+{extra} characters)"));
            return (out, true);
        }
        out.push(character);
    }
    (out, false)
}
