//! `at-recall pr` — the facts a PR description is written from.
//!
//! The motivating case, and the one the whole toolkit was designed around: *"write me a PR
//! description"* starts an archaeology — branch, base, `merge-base`, `log --oneline base..HEAD`,
//! `diff --stat`, then reads to work out what the diff means — and every step is a separate
//! approval. This verb is that archaeology as one read, in one trust tier, with the intermediate
//! held as structs rather than piped as text.
//!
//! Four rules it follows, from the recipe section of the design doc:
//!
//! * **It prints facts, never conclusions.** `1 manifest · 1 lockfile` — not "this is a breaking
//!   change". The categories are mechanical and stated; the judgement is the reader's.
//! * **Every section carries a bound.** Each label line carries its count, and a section the limit
//!   cut says so underneath.
//! * **Recipes are code, not configuration.** The sections are named in the catalogue, so `--with`
//!   selects from a set a reviewer can read.
//! * **One trust tier.** It reads commits and no working-tree file, so the filter-driver refusal
//!   that guards `diff` cannot apply here — there is no conversion for a repository to name a
//!   program for.
//!
//! What it does not do: write the description. The reason a change exists is the one thing here that
//! is not derivable from the repository, and that is the reader's to supply.

use at_core::contract::{Exit, Fail, Report, MAX_LIMIT};

use crate::catalogue::PR_SECTIONS;
use crate::facts::{self, FileStat};
use crate::git::Noun;
use crate::verbs::{again, plural, Budget, Opts, Outcome};
use crate::TOOL;

pub fn run(opts: &Opts) -> Result<Outcome, Fail> {
    let mut report = Report::new();
    if let Some(asked) = opts.limit_clamped_from {
        report.bound(format!("--limit {asked} clamped to {MAX_LIMIT}"));
    }

    let wanted = Sections::of(opts)?;
    let base = resolve_base(opts)?;
    let commit_range = format!("{base}..HEAD");
    // Three dots: what the branch adds on top of the point where it diverged, which is what a pull
    // request shows — and both sides are commits, so no working-tree file is converted.
    let diff_range = format!("{base}...HEAD");

    report.header(TOOL, "pr", opts.root.name(), &format!("base {base}"));
    // Always true, and the caveat a reader needs most: the base is whatever this repository has
    // locally, and nothing here fetches anything.
    report.bound(format!(
        "caveat: {base} is a local ref and this tool never fetches — the remote may be ahead"
    ));

    let commits = if wanted.commits {
        Some(facts::commits(&opts.git, &commit_range, &[], opts.limit)?)
    } else {
        None
    };
    // `areas` is counted from the diffstat, so asking for it reads the diffstat without printing
    // its rows.
    let reading_files = wanted.diffstat || wanted.areas;
    let files = if reading_files {
        Some(facts::numstat(
            &opts.git,
            &Some(diff_range.clone()),
            &[],
            false,
        )?)
    } else {
        None
    };
    // The whitespace delta is reported rather than left behind a flag: "half of this is
    // reindentation" is a fact a reviewer wants, and one they should not have to know to ask for.
    let without_whitespace = if reading_files {
        Some(facts::numstat(
            &opts.git,
            &Some(diff_range.clone()),
            &[],
            true,
        )?)
    } else {
        None
    };

    let mut budget = Budget::new(opts.limit);
    let mut cut_commits = 0;
    let mut cut_files = 0;
    // Section labels are content but never spend budget — they are what says a section was cut — so
    // they are counted separately for the total at the end.
    let mut labels = 0usize;

    if let Some(commits) = &commits {
        let before = budget.dropped();
        report.content(label("commits", plural(commits.total, "commit")));
        labels += 1;
        for line in commits.lines() {
            budget.push(&mut report, format!("  {line}"));
        }
        let shown = commits.rows.len().saturating_sub(budget.dropped() - before);
        if shown < commits.total {
            cut_commits = commits.total;
            // No `--limit` here: the limit is shared across the sections, so quoting it beside one
            // section's count would suggest raising it by *this* section's shortfall is enough.
            // The exit below names the read that answers this section alone.
            report.bound(format!("{shown} of {}", plural(commits.total, "commit")));
        }
    }

    if let Some(files) = &files {
        // `areas` is counted from these files but does not print them, so the rows belong to the
        // `diffstat` section and only it may spend budget on them.
        if wanted.diffstat {
            let before = budget.dropped();
            report.content(label(
                "diffstat",
                format!("{}, {}", plural(files.len(), "file"), facts::totals(files)),
            ));
            labels += 1;
            let widths = Widths::of(files);
            for file in files {
                budget.push(&mut report, widths.row(file));
            }
            let shown = files.len().saturating_sub(budget.dropped() - before);
            if shown < files.len() {
                cut_files = files.len();
                report.bound(format!("{shown} of {}", plural(files.len(), "file")));
            }
            if let Some(delta) = without_whitespace
                .as_ref()
                .and_then(|without| whitespace_delta(without, files))
            {
                report.bound(delta);
            }
        }
        if wanted.areas {
            report.content(area_line(files));
            labels += 1;
        }
    }

    // A branch with nothing on top of its base is the one answer a reader branches on the code
    // alone, and the labels below are content, so the frame would otherwise read as a result.
    let empty = commits.as_ref().is_none_or(|c| c.total == 0)
        && files.as_ref().is_none_or(|f| f.is_empty());
    if empty {
        report.bound(format!("nothing between {base} and HEAD"));
    }

    // The full answer rather than the part of it the budget saw: `log` is capped at `--limit` as
    // well, so a commit the limit never fetched was never counted as dropped, and a total that left
    // it out would understate the answer it claims to measure.
    let rows = commits.as_ref().map_or(0, |c| c.total)
        + match &files {
            Some(files) if wanted.diffstat => files.len(),
            _ => 0,
        };
    let emitted = report.content_lines();
    if emitted < rows + labels {
        // The per-section bounds say which section lost rows; this one says how much of the answer
        // went missing in total, which is the only line that can state a section that vanished.
        report.bound(format!(
            "{emitted} of {} lines · --limit {} reached",
            rows + labels,
            opts.limit
        ));
    }
    budget.width_caveat(&mut report);
    exits(
        &mut report,
        opts,
        &commit_range,
        &diff_range,
        cut_commits,
        cut_files,
        // "The hunks" is an offer to read more of the same change set, so a branch that has no
        // change set at all does not offer it — there is nothing behind the table.
        !empty && files.is_some(),
    );

    Ok(outcome(report, empty))
}

/// The recipe's outcome, with the one deviation from the shared rule: an empty branch exits 1 even
/// though its labels printed, because "nothing to describe" is the fact a caller asked for.
fn outcome(report: Report, empty: bool) -> Outcome {
    let mut outcome = Outcome::from_report(report);
    if empty {
        outcome.exit = Exit::Nothing;
    }
    outcome
}

/// Which sections were asked for. An unknown name is a usage error naming the ones that exist,
/// because the set is fixed and a reader who mistypes should not silently get fewer sections.
struct Sections {
    commits: bool,
    diffstat: bool,
    areas: bool,
}

impl Sections {
    fn of(opts: &Opts) -> Result<Sections, Fail> {
        if opts.with.is_empty() {
            return Ok(Sections {
                commits: true,
                diffstat: true,
                areas: true,
            });
        }
        for name in &opts.with {
            if !PR_SECTIONS.contains(&name.as_str()) {
                return Err(Fail::usage(format!(
                    "`{name}` is not a section of this recipe · sections: {}",
                    PR_SECTIONS.join(", ")
                )));
            }
        }
        let has = |name: &str| opts.with.iter().any(|given| given == name);
        Ok(Sections {
            commits: has("commits"),
            diffstat: has("diffstat"),
            areas: has("areas"),
        })
    }
}

/// What the branch diverged from: what the caller named, else the branch's upstream, else
/// `origin/HEAD` — and nothing else, because a guess here would be a base the reader did not choose
/// and cannot see. No base at all is a refusal that names the flag which fixes it.
fn resolve_base(opts: &Opts) -> Result<String, Fail> {
    if let Some(named) = &opts.base {
        // A base is one revision: the recipe builds both ranges (`base..HEAD` and `base...HEAD`) from
        // it, so a range here would be pasted into the middle of another one.
        if named.contains("..") {
            return Err(Fail::usage(format!(
                "`{named}` is a range, and a base is one revision · this recipe builds the range \
                 from the base and HEAD"
            )));
        }
        let base = crate::git::rev(named)?;
        if !opts.git.names_a_revision(&base) {
            return Err(Fail::environment(format!(
                "`{base}` is not a revision in {}",
                opts.root.name()
            )));
        }
        return Ok(base);
    }

    let branch = opts
        .git
        .probe(Noun::RevParse, &["--abbrev-ref", "HEAD"])
        .map(|out| out.trim().to_string())
        .filter(|branch| branch != "HEAD");
    if let Some(branch) = &branch {
        if let Some(upstream) = facts::upstream_of(&opts.git, branch) {
            return Ok(upstream);
        }
    }
    if let Some(head) = opts
        .git
        .probe(Noun::RevParse, &["--abbrev-ref", "origin/HEAD"])
    {
        let head = head.trim();
        if !head.is_empty() {
            return Ok(head.to_string());
        }
    }
    Err(Fail::usage(
        "no base to compare against: this branch tracks nothing and there is no origin/HEAD · name \
         one with --base <rev>"
            .to_string(),
    ))
}

/// The exits: widen what was cut, then the hunks behind the table. Each names a *primitive* rather
/// than this recipe, because that is where the detail lives — the recipe's job was to say what there
/// is, not to become the only way to read it.
fn exits(
    report: &mut Report,
    opts: &Opts,
    commit_range: &str,
    diff_range: &str,
    cut_commits: usize,
    cut_files: usize,
    read_files: bool,
) {
    let mut emitted = false;
    if cut_commits > 0 {
        report.next(
            again(
                opts,
                "log",
                Some(commit_range),
                &[],
                &[format!("--limit {}", cut_commits.min(MAX_LIMIT))],
            ),
            format!("all {}", plural(cut_commits, "commit")),
        );
        emitted = true;
    }
    if cut_files > 0 {
        report.next(
            again(
                opts,
                "diff",
                Some(diff_range),
                &[],
                &[format!("--limit {}", cut_files.min(MAX_LIMIT))],
            ),
            format!("all {}", plural(cut_files, "file")),
        );
        emitted = true;
    }
    if !emitted && read_files {
        report.next(
            again(
                opts,
                "diff",
                Some(diff_range),
                &[],
                &["--patch".to_string()],
            ),
            "the hunks",
        );
    }
}

/// What `--ignore-space` would have hidden, stated without being asked for: the files that change
/// only whitespace, and how many of the lines are whitespace.
fn whitespace_delta(ignored: &[FileStat], raw: &[FileStat]) -> Option<String> {
    let only = raw
        .iter()
        .filter(|file| !ignored.iter().any(|kept| kept.label == file.label))
        .count();
    if only == 0 {
        return None;
    }
    let added = sum(raw, |file| file.added) - sum(ignored, |file| file.added);
    let deleted = sum(raw, |file| file.deleted) - sum(ignored, |file| file.deleted);
    Some(format!(
        "whitespace only: {}, +{added} -{deleted} of the lines",
        plural(only, "file")
    ))
}

/// The totals ignoring whitespace are never larger than the raw ones, so the difference is a count
/// of lines that changed only in whitespace — and it is computed as a difference rather than a sum
/// so a binary file's absent counts cannot make it wrong.
fn sum(files: &[FileStat], of: impl Fn(&FileStat) -> Option<u64>) -> u64 {
    files.iter().filter_map(of).sum()
}

/// The section labels share a column width so the counts start at the same character: `diffstat` is
/// the longest label, and a section added later that overflows it widens every row rather than
/// colliding with one.
const LABEL: usize = 9;

/// One section's label and its count.
fn label(name: &str, rest: impl std::fmt::Display) -> String {
    format!("{name:<LABEL$}  {rest}")
}

/// The change set by kind, as `1 manifest · 1 lockfile · 2 docs`, in the order of [`area`]'s rules
/// so two runs over the same repository print the same sentence.
fn areas(files: &[FileStat]) -> String {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for &kind in KINDS {
        let count = files
            .iter()
            .filter(|file| area(&file.label) == kind)
            .count();
        if count > 0 {
            counts.push((kind, count));
        }
    }
    counts
        .into_iter()
        .map(|(kind, count)| format!("{count} {kind}"))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The `areas` section: its own count, then the breakdown — and nothing after the count when there
/// is no breakdown, because `0 files · ` ends a line with a separator that introduces nothing.
fn area_line(files: &[FileStat]) -> String {
    let count = plural(files.len(), "file");
    match areas(files) {
        kinds if kinds.is_empty() => label("areas", count),
        kinds => label("areas", format!("{count} · {kinds}")),
    }
}

/// The kinds a changed path can be, in the order the rules are tried.
const KINDS: &[&str] = &[
    "manifest",
    "lockfile",
    "schema",
    "generated",
    "docs",
    "tests",
    "assets",
    "code",
];

const MANIFESTS: &[&str] = &[
    "Cargo.toml",
    "package.json",
    "flake.nix",
    "pyproject.toml",
    "go.mod",
    "Makefile",
    "Gemfile",
    "composer.json",
];

const LOCKFILES: &[&str] = &[
    "Cargo.lock",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "flake.lock",
    "poetry.lock",
    "Gemfile.lock",
    "composer.lock",
];

const ASSETS: &[&str] = &[
    ".png", ".jpg", ".jpeg", ".gif", ".webp", ".avif", ".ico", ".woff", ".woff2", ".ttf", ".otf",
    ".mp4", ".mov", ".pdf",
];

/// What a changed path is, by its name alone — never by its contents.
///
/// Mechanical, and stated so a reader can check it: the rules are tried in the order of [`KINDS`],
/// first match wins, so a `Cargo.toml` under `tests/` is a manifest — the rarer and more
/// consequential fact — and a `.md` under `docs/` is docs rather than code. Nothing here opens a
/// file, which is what makes the classification a fact rather than an opinion.
fn area(path: &str) -> &'static str {
    let name = path.rsplit('/').next().unwrap_or(path);
    let lower = path.to_lowercase();
    if MANIFESTS.contains(&name) {
        return "manifest";
    }
    if LOCKFILES.contains(&name) {
        return "lockfile";
    }
    if lower.ends_with(".sql") || lower.contains("/migrations/") {
        return "schema";
    }
    if lower.contains("/generated/") || lower.contains(".generated.") || lower.ends_with(".snap") {
        return "generated";
    }
    if lower.ends_with(".md") {
        return "docs";
    }
    if lower.contains("/tests/")
        || lower.contains("/test/")
        || lower.ends_with("_test.rs")
        || lower.starts_with("test_")
        || lower.ends_with(".spec.ts")
        || lower.ends_with(".test.ts")
    {
        return "tests";
    }
    if ASSETS.iter().any(|suffix| lower.ends_with(suffix)) {
        return "assets";
    }
    "code"
}

/// Column widths for the diffstat rows, from the rows themselves.
struct Widths {
    counts: usize,
    labels: usize,
}

impl Widths {
    fn of(files: &[FileStat]) -> Widths {
        Widths {
            counts: files
                .iter()
                .map(|file| file.counts().len())
                .max()
                .unwrap_or(0),
            labels: files.iter().map(|file| file.label.len()).max().unwrap_or(0),
        }
    }

    fn row(&self, file: &FileStat) -> String {
        format!(
            "  {:<counts$}  {:<labels$}  {}",
            file.counts(),
            file.label,
            area(&file.label),
            counts = self.counts,
            labels = self.labels,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_classified_by_its_name() {
        assert_eq!(area("Cargo.toml"), "manifest");
        assert_eq!(area("crates/x/Cargo.toml"), "manifest");
        assert_eq!(area("flake.lock"), "lockfile");
        assert_eq!(area("crates/x/migrations/0001_init.sql"), "schema");
        assert_eq!(area("src/schema.sql"), "schema");
        assert_eq!(area("src/api.generated.rs"), "generated");
        assert_eq!(area("docs/design/THING.md"), "docs");
        assert_eq!(area("crates/x/tests/contract.rs"), "tests");
        assert_eq!(area("src/cache_test.rs"), "tests");
        assert_eq!(area("assets/logo.png"), "assets");
        assert_eq!(area("src/cache.rs"), "code");
        assert_eq!(area("README"), "code");
    }

    #[test]
    fn the_rules_are_ordered_and_the_order_is_documented() {
        // A manifest inside a test directory is a manifest: the rarer fact wins, and the precedence
        // is in the function's documentation rather than discovered by a reader of the output.
        assert_eq!(area("tests/Cargo.toml"), "manifest");
        assert_eq!(area("docs/notes.sql"), "schema");
    }

    #[test]
    fn areas_counts_only_what_is_there_and_in_rule_order() {
        let files = vec![
            file("Cargo.toml"),
            file("Cargo.lock"),
            file("src/a.rs"),
            file("src/b.rs"),
            file("docs/x.md"),
        ];
        assert_eq!(areas(&files), "1 manifest · 1 lockfile · 1 docs · 2 code");
    }

    #[test]
    fn a_whitespace_only_file_is_named_and_counted() {
        let raw = vec![file("src/a.rs"), file("src/b.rs")];
        let ignored = vec![file("src/b.rs")];
        assert_eq!(
            whitespace_delta(&ignored, &raw),
            Some("whitespace only: 1 file, +1 -0 of the lines".to_string())
        );
    }

    #[test]
    fn areas_states_its_count_with_nothing_to_break_down() {
        // `0 files · ` would end a line on a separator that introduces nothing.
        assert_eq!(area_line(&[]), "areas      0 files");
        assert_eq!(area_line(&[file("src/a.rs")]), "areas      1 file · 1 code");
    }

    #[test]
    fn every_section_label_fits_the_shared_column() {
        // The labels are the three section names; a longer one would push its own count out of the
        // column rather than widening it, which is the failure this pins.
        for name in PR_SECTIONS {
            assert!(name.len() <= LABEL, "{name} is wider than the label column");
            assert!(label(name, "x").starts_with(&format!("{name}  ")));
        }
    }

    #[test]
    fn no_whitespace_only_file_says_nothing() {
        let raw = vec![file("src/a.rs")];
        assert_eq!(whitespace_delta(&raw, &raw), None);
    }

    fn file(label: &str) -> FileStat {
        FileStat {
            added: Some(1),
            deleted: Some(0),
            label: label.to_string(),
            names: vec![label.to_string()],
        }
    }
}
