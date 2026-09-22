//! `at-peek slice` — the lines you asked for, numbered, with the file's true size attached.
//!
//! This is the `sed -n '40,60p'` replacement, and the difference that matters is what it says when
//! it stops: `sed` prints twenty lines and says nothing about the other three hundred and ninety-two,
//! which is how a truncated read becomes a confident report. Every slice states the file's real size
//! and, when the limit cut the range, offers the range that resumes as an exit rather than as prose
//! inside a bound — the reader should be able to run the continuation without retyping it.

use crate::contract::{truncate, Fail, Report, MAX_LIMIT, MAX_LINE_WIDTH};
use crate::verbs::{again, open, plural, Opts, Outcome};
use crate::TOOL;

pub fn run(targets: &[String], opts: &Opts) -> Result<Outcome, Fail> {
    let mut report = Report::new();
    if let Some(asked) = opts.limit_clamped_from {
        report.bound(format!("--limit {asked} clamped to {MAX_LIMIT}"));
    }

    // One budget for the whole invocation: `--limit` bounds what a caller sees, not what each
    // target sees, so a batch of ten targets cannot print ten times the cap.
    let mut budget = opts.limit;
    // The continuation, assembled as the loop discovers what was cut: the rest of the range the
    // budget stopped inside, and every target after it that was never reached. One exit for the
    // invocation, not one per target.
    let mut unfinished: Vec<String> = Vec::new();

    for (index, arg) in targets.iter().enumerate() {
        if budget == 0 {
            report.bound(format!(
                "--limit {} reached · {} not shown",
                opts.limit,
                plural(targets.len() - index, "target")
            ));
            unfinished.extend(targets[index..].iter().cloned());
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
            format!("{shown} of {total} lines")
        });
        if truncated > 0 {
            report.bound(format!(
                "caveat: {truncated} line(s) wider than {MAX_LINE_WIDTH} characters, truncated"
            ));
        }

        // The continuation, in two shapes. A limit that cut the request finishes *that* range; a
        // range that simply ended inside the file continues to the end of the file. Either way the
        // later targets travel with it, so one exit continues the whole invocation rather than
        // only the target it happened to be reading.
        let next = start + shown;
        let later = targets[index + 1..].iter().cloned();
        let after = targets.len() - index - 1;
        if shown < wanted {
            report.bound(format!(
                "{shown} of {wanted} requested lines · --limit {} reached",
                opts.limit
            ));
            unfinished.push(format!("{rel}:{next}-{end}"));
            unfinished.extend(later);
            // Breaking here means the loop's own "not shown" bound never runs, and an unreached
            // target that is not named is a target the reader does not know exists.
            if after > 0 {
                report.bound(format!("{} not shown", plural(after, "target")));
            }
            break;
        }
        if end < total {
            unfinished.push(format!("{rel}:{next}-{total}"));
            unfinished.extend(later);
        }
    }

    if !unfinished.is_empty() {
        // True in both shapes of continuation: finishing a range the limit cut, and reading on from
        // where an explicit range stopped. The command itself shows which ranges it will read.
        report.next(
            again(opts, "slice", &unfinished, &[]),
            "the lines that follow",
        );
    }

    Ok(Outcome::from_report(report))
}
