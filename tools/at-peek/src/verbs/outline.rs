//! `at-peek outline` — a file's sections, in one read.
//!
//! The shape a reader asks for before reading: every ATX heading with the line it is on, outside
//! fenced code blocks, and the file's true line count beneath it. It is what `grep -n '^## '` and a
//! chain of slices are hand-rolling, and the difference is the one the other verbs make — the
//! answer states how much of the file it accounted for instead of showing a window and stopping.
//!
//! Only ATX headings are reported. A setext underline (`===` or `---`) is not guessed at, because
//! a `---` rule and a heading underline are the same bytes and a wrong heading is worse than a
//! missing one. A file with no headings says so, rather than reading as an empty answer.
//!
//! The answer is an [`OutlineFacts`] value. [`collect`] reads it; [`OutlineFacts::report`] renders
//! the CLI's bytes. A harness calls `collect` and keeps the facts rather than parsing the listing.

use crate::contract::{truncate, Fail, Report, MAX_LIMIT, MAX_LINE_WIDTH};
use crate::verbs::{open, plural, Opts, Outcome};
use crate::TOOL;

/// One heading, as it appears in the file.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Heading {
    /// 1-based, as a reader counts.
    pub line: usize,
    /// The number of leading `#` characters, 1 to 6.
    pub depth: usize,
    /// The heading text without the hashes.
    pub text: String,
}

/// One target's share of the answer.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OutlinedTarget {
    /// Relative to the root, forward slashes.
    pub path: String,
    /// The target as the caller wrote it.
    pub requested: String,
    /// Byte offset of the first NUL, when the file is not line-addressable.
    pub binary: Option<usize>,
    /// Lines in the file, as a reader counts them.
    pub total: usize,
    /// Every heading in the file, before the budget.
    pub headings_total: usize,
    /// The headings the budget left room for.
    pub headings: Vec<Heading>,
    /// How many of `headings` are here — the count the bound is written from.
    pub shown: usize,
    /// Headings over the width cap.
    pub truncated: usize,
    /// The file was empty.
    pub empty: bool,
}

/// The answer to an outline, before it is a report.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OutlineFacts {
    pub targets: Vec<OutlinedTarget>,
    /// Whole targets the invocation never reached because the budget was already spent.
    pub unreached: usize,
}

/// Read every target, applying the shared budget in the order the caller named them.
pub fn collect(targets: &[String], opts: &Opts) -> Result<OutlineFacts, Fail> {
    // One budget for the whole invocation: `--limit` bounds what a caller sees, not what each
    // target sees, so a batch of ten files cannot print ten times the cap.
    let mut budget = opts.limit;
    let mut facts = OutlineFacts::default();

    for (index, arg) in targets.iter().enumerate() {
        if budget == 0 {
            facts.unreached = targets.len() - index;
            break;
        }

        let opened = open(opts, arg)?;
        if opened.target.range.is_some() {
            return Err(Fail::usage(format!(
                "outline reads a whole file · `{arg}` names a range; use `at-peek slice` for lines"
            )));
        }
        let mut outlined = OutlinedTarget {
            path: opened.rel.clone(),
            requested: opened.target.describe(&opened.rel),
            binary: opened.binary,
            total: 0,
            headings_total: 0,
            headings: Vec::new(),
            shown: 0,
            truncated: 0,
            empty: false,
        };

        if outlined.binary.is_some() {
            facts.targets.push(outlined);
            continue;
        }

        let total = opened.lines();
        outlined.total = total;
        if total == 0 {
            outlined.empty = true;
            facts.targets.push(outlined);
            continue;
        }

        let found = headings(&opened.text);
        outlined.headings_total = found.len();
        for heading in found.into_iter().take(budget) {
            let (text, cut) = truncate(&heading.text);
            if cut {
                outlined.truncated += 1;
            }
            outlined.headings.push(Heading { text, ..heading });
            budget -= 1;
        }
        outlined.shown = outlined.headings.len();
        facts.targets.push(outlined);
    }

    Ok(facts)
}

impl OutlineFacts {
    /// Render the CLI's answer.
    pub fn report(&self, opts: &Opts) -> Outcome {
        let mut report = Report::new();
        if let Some(asked) = opts.limit_clamped_from {
            report.bound(format!("--limit {asked} clamped to {MAX_LIMIT}"));
        }

        for target in &self.targets {
            report.header(TOOL, "outline", opts.root.name(), &target.requested);

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

            let width = target
                .headings
                .last()
                .map(|heading| heading.line.to_string().len())
                .unwrap_or(1);
            for heading in &target.headings {
                let hashes = "#".repeat(heading.depth);
                report.content(format!(
                    "{line:>width$}  {hashes} {text}",
                    line = heading.line,
                    text = heading.text
                ));
            }

            report.bound(if target.shown == target.headings_total {
                format!(
                    "{} · {}",
                    plural(target.headings_total, "heading"),
                    plural(target.total, "line")
                )
            } else {
                format!(
                    "{} of {} · {} · --limit {}",
                    target.shown,
                    plural(target.headings_total, "heading"),
                    plural(target.total, "line"),
                    opts.limit
                )
            });
            if target.truncated > 0 {
                report.bound(format!(
                    "caveat: {} heading(s) wider than {MAX_LINE_WIDTH} characters, truncated",
                    target.truncated
                ));
            }
        }

        if self.unreached > 0 {
            report.bound(format!(
                "--limit {} reached · {} not shown",
                opts.limit,
                plural(self.unreached, "target")
            ));
        }

        Outcome::from_report(report)
    }
}

/// The headings a reader would see: an ATX heading outside a fenced code block.
fn headings(text: &str) -> Vec<Heading> {
    let mut found = Vec::new();
    let mut fence: Option<(char, usize)> = None;

    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if let Some((marker, opened)) = fence {
            // A fence closes on its own marker, at least as long as the one that opened it. A
            // backtick fence around a shell snippet is full of `#` comments, and reporting them as
            // headings is the failure this loop exists to prevent.
            if let Some((closing, count)) = fence_marker(trimmed) {
                if closing == marker && count >= opened {
                    fence = None;
                }
            }
            continue;
        }
        if let Some(marker) = fence_marker(trimmed) {
            fence = Some(marker);
            continue;
        }
        if let Some((depth, heading)) = atx(trimmed) {
            found.push(Heading {
                line: index + 1,
                depth,
                text: heading,
            });
        }
    }

    found
}

/// A fence opening: three or more backticks or tildes, indented or not.
fn fence_marker(line: &str) -> Option<(char, usize)> {
    let marker = line.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let count = line.chars().take_while(|c| *c == marker).count();
    (count >= 3).then_some((marker, count))
}

/// An ATX heading: one to six `#` followed by a space or the end of the line, then text.
fn atx(line: &str) -> Option<(usize, String)> {
    let depth = line.chars().take_while(|c| *c == '#').count();
    if depth == 0 || depth > 6 {
        return None;
    }
    let rest = &line[depth..];
    // `#hashtag` is a word, not a heading: the hashes must be followed by a space or end the line.
    if !rest.is_empty() && !rest.starts_with(' ') && !rest.starts_with('\t') {
        return None;
    }
    let text = heading_text(rest);
    if text.is_empty() {
        return None;
    }
    Some((depth, text.to_owned()))
}

/// The text of a heading, with a closing hash run removed only where CommonMark allows one.
///
/// `## Status ##` is the heading `Status`; `## C#` keeps its hash, because a closing run must be
/// preceded by a space. Trimming every trailing `#` would rename a real heading.
fn heading_text(rest: &str) -> &str {
    let text = rest.trim();
    let without = text.trim_end_matches('#');
    if without.is_empty() {
        return "";
    }
    if without.len() < text.len() && without.ends_with(char::is_whitespace) {
        return without.trim_end();
    }
    text
}

pub fn run(targets: &[String], opts: &Opts) -> Result<Outcome, Fail> {
    collect(targets, opts).map(|facts| facts.report(opts))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(text: &str) -> Vec<(usize, usize, String)> {
        headings(text)
            .into_iter()
            .map(|heading| (heading.line, heading.depth, heading.text))
            .collect()
    }

    #[test]
    fn headings_keep_their_line_and_depth() {
        assert_eq!(
            rows("# One\n\ntext\n\n## Two\n"),
            vec![(1, 1, "One".to_owned()), (5, 2, "Two".to_owned())]
        );
    }

    #[test]
    fn a_hash_without_a_space_is_not_a_heading() {
        assert!(rows("#hashtag\n##nope\n").is_empty());
    }

    #[test]
    fn a_fence_hides_everything_inside_it() {
        let text = "# Real\n```sh\n# not a heading\n## nor this\n```\n## After\n";
        assert_eq!(
            rows(text),
            vec![(1, 1, "Real".to_owned()), (6, 2, "After".to_owned())]
        );
    }

    #[test]
    fn a_fence_closes_only_on_its_own_marker() {
        let text = "~~~\n# hidden\n```\n# still hidden\n~~~\n# shown\n";
        assert_eq!(rows(text), vec![(6, 1, "shown".to_owned())]);
    }

    #[test]
    fn a_closing_hash_run_is_decoration_but_c_sharp_is_not() {
        assert_eq!(rows("## Status ##\n"), vec![(1, 2, "Status".to_owned())]);
        assert_eq!(rows("## C#\n"), vec![(1, 2, "C#".to_owned())]);
    }

    #[test]
    fn an_empty_heading_is_not_a_row() {
        assert!(rows("###\n## ###\n").is_empty());
    }

    #[test]
    fn a_deeper_fence_is_not_closed_by_a_shallower_run() {
        let text = "````\n# hidden\n```\n# still hidden\n````\n# shown\n";
        assert_eq!(rows(text), vec![(6, 1, "shown".to_owned())]);
    }
}
