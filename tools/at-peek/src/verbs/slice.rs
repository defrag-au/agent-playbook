//! `at-peek slice` — the lines you asked for, numbered, with the file's true size attached.
//!
//! This is the `sed -n '40,60p'` replacement, and the difference that matters is what it says when
//! it stops: `sed` prints twenty lines and says nothing about the other three hundred and ninety-two,
//! which is how a truncated read becomes a confident report. Every slice states the file's real size
//! and, when the limit cut the range, offers the range that resumes as an exit rather than as prose
//! inside a bound — the reader should be able to run the continuation without retyping it.
//!
//! The answer is a [`SliceFacts`] value. [`collect`] reads and bounds it; [`SliceFacts::report`]
//! renders the CLI's bytes. A harness calls `collect` and keeps the facts, which is what lets the
//! same read be typed rather than scraped from stdout.

use crate::contract::{truncate, Fail, Report, MAX_LIMIT, MAX_LINE_WIDTH};
use crate::verbs::{again, open, plural, Opts, Outcome};
use crate::TOOL;

/// One target's share of the answer.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SlicedTarget {
    /// Relative to the root, forward slashes.
    pub path: String,
    /// The target as the caller wrote it, with its range.
    pub requested: String,
    /// Byte offset of the first NUL, when the file is not line-addressable.
    pub binary: Option<usize>,
    /// Lines in the file, as a reader counts them.
    pub total: usize,
    /// The first line the target asked for.
    pub start: usize,
    /// Lines the target asked for before the file or the budget cut it.
    pub wanted: usize,
    /// The numbered lines the budget left room for, already truncated to the width cap.
    pub lines: Vec<String>,
    /// How many of `lines` are here — the count the bound is written from.
    pub shown: usize,
    /// Lines over the width cap.
    pub truncated: usize,
    /// The file was empty.
    pub empty: bool,
    /// The requested start is past the end of the file.
    pub past_end: bool,
    /// The requested range ends past the file, but was still readable.
    pub range_ends_past: bool,
    /// The budget cut the requested lines.
    pub limit_reached: bool,
    /// Targets after this one that were never reached when the budget cut it.
    pub later: usize,
}

/// The answer to a slice, before it is a report.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SliceFacts {
    pub targets: Vec<SlicedTarget>,
    /// Whole targets the invocation never reached because the budget was already spent.
    pub unreached: usize,
    /// The continuation: ranges and later targets that were not read.
    pub unfinished: Vec<String>,
}

/// Read every target, applying the shared budget in the order the caller named them.
pub fn collect(targets: &[String], opts: &Opts) -> Result<SliceFacts, Fail> {
    // One budget for the whole invocation: `--limit` bounds what a caller sees, not what each
    // target sees, so a batch of ten targets cannot print ten times the cap.
    let mut budget = opts.limit;
    // The continuation is assembled as the loop discovers what was cut: the rest of the range the
    // budget stopped inside, and every target after it that was never reached. One exit for the
    // invocation, not one per target.
    let mut facts = SliceFacts::default();

    for (index, arg) in targets.iter().enumerate() {
        if budget == 0 {
            facts.unreached = targets.len() - index;
            facts.unfinished.extend(targets[index..].iter().cloned());
            break;
        }

        let opened = open(opts, arg)?;
        let mut sliced = SlicedTarget {
            path: opened.rel.clone(),
            requested: opened.target.describe(&opened.rel),
            binary: opened.binary,
            total: 0,
            start: 0,
            wanted: 0,
            lines: Vec::new(),
            shown: 0,
            truncated: 0,
            empty: false,
            past_end: false,
            range_ends_past: false,
            limit_reached: false,
            later: 0,
        };

        if sliced.binary.is_some() {
            facts.targets.push(sliced);
            continue;
        }

        let total = opened.lines();
        sliced.total = total;
        if total == 0 {
            sliced.empty = true;
            facts.targets.push(sliced);
            continue;
        }

        let (start, asked) = match opened.target.range {
            Some(range) => (range.start(), range.requested_lines()),
            None => (1, total),
        };
        sliced.start = start;
        if start > total {
            sliced.past_end = true;
            facts.targets.push(sliced);
            continue;
        }
        if start + asked - 1 > total {
            sliced.range_ends_past = true;
        }

        let wanted = asked.min(total - start + 1);
        let end = start + wanted - 1;
        let shown = wanted.min(budget);
        let number_width = (start + shown - 1).to_string().len();
        sliced.wanted = wanted;

        for (offset, line) in opened.text.lines().skip(start - 1).take(shown).enumerate() {
            let number = start + offset;
            let (text, cut) = truncate(line);
            if cut {
                sliced.truncated += 1;
            }
            sliced
                .lines
                .push(format!("{number:>number_width$}  {text}"));
        }
        sliced.shown = shown;
        budget -= shown;

        // A limit that cut the request finishes *that* range; a range that simply ended inside the
        // file continues to the end of the file. Either way the later targets travel with it, so one
        // exit continues the whole invocation rather than only the target it happened to be reading.
        let next = start + shown;
        let after = targets.len() - index - 1;
        if shown < wanted {
            sliced.limit_reached = true;
            sliced.later = after;
            facts
                .unfinished
                .push(format!("{}:{next}-{end}", sliced.path));
            facts
                .unfinished
                .extend(targets[index + 1..].iter().cloned());
            facts.targets.push(sliced);
            break;
        }
        if end < total {
            facts
                .unfinished
                .push(format!("{}:{next}-{total}", sliced.path));
            facts
                .unfinished
                .extend(targets[index + 1..].iter().cloned());
        }
        facts.targets.push(sliced);
    }

    Ok(facts)
}

impl SliceFacts {
    /// Render the CLI's answer.
    pub fn report(&self, opts: &Opts) -> Outcome {
        let mut report = Report::new();
        if let Some(asked) = opts.limit_clamped_from {
            report.bound(format!("--limit {asked} clamped to {MAX_LIMIT}"));
        }

        for target in &self.targets {
            report.header(TOOL, "slice", opts.root.name(), &target.requested);

            if let Some(offset) = target.binary {
                report.bound(format!(
                    "caveat: binary file, NUL at byte {offset} · not line-addressable"
                ));
                continue;
            }

            if target.empty {
                report.bound(format!("{} is empty", target.path));
                continue;
            }

            if target.past_end {
                report.bound(format!(
                    "line {} is past the end of {} ({} lines)",
                    target.start, target.path, target.total
                ));
                continue;
            }

            if target.range_ends_past {
                report.bound(format!(
                    "the requested range ends past the file's {} lines",
                    target.total
                ));
            }

            for line in &target.lines {
                report.content(line.clone());
            }

            report.bound(if target.shown == target.total {
                format!("{} lines · whole file", target.total)
            } else {
                format!("{} of {} lines", target.shown, target.total)
            });
            if target.truncated > 0 {
                report.bound(format!(
                    "caveat: {} line(s) wider than {MAX_LINE_WIDTH} characters, truncated",
                    target.truncated
                ));
            }
            if target.limit_reached {
                report.bound(format!(
                    "{} of {} requested lines · --limit {} reached",
                    target.shown, target.wanted, opts.limit
                ));
                // Breaking the loop means its own "not shown" bound never runs, and an unreached
                // target that is not named is a target the reader does not know exists.
                if target.later > 0 {
                    report.bound(format!("{} not shown", plural(target.later, "target")));
                }
            }
        }

        if self.unreached > 0 {
            report.bound(format!(
                "--limit {} reached · {} not shown",
                opts.limit,
                plural(self.unreached, "target")
            ));
        }

        if !self.unfinished.is_empty() {
            // True in both shapes of continuation: finishing a range the limit cut, and reading on
            // from where an explicit range stopped. The command itself shows which ranges it will
            // read.
            report.next(
                again(opts, "slice", &self.unfinished, &[]),
                "the lines that follow",
            );
        }

        Outcome::from_report(report)
    }
}

pub fn run(targets: &[String], opts: &Opts) -> Result<Outcome, Fail> {
    collect(targets, opts).map(|facts| facts.report(opts))
}
