//! `at-recall log` — what changed lately.
//!
//! The question an agent asks to place itself: "has this file been touched since I read it", "how
//! recent is this design doc", "what is this branch's work". It is one line per commit, which is the
//! form `git log --oneline` and the `| head -4` habit both reach for — except the bound is stated,
//! and the total comes from walking the *same* revision and paths, so the count under the listing is
//! the number the listing was drawn from rather than a guess. The read itself lives in
//! [`crate::facts`], because the `pr` recipe prints the same lines.
//!
//! Two things it deliberately does not do yet: filter by author or date, and follow a file's
//! renames. Both are flags on this question rather than new questions, and neither showed up in a
//! session — so they are not in the grammar until they do.

use at_core::contract::{Fail, Report, MAX_LIMIT};

use crate::facts::{self, Commits};
use crate::verbs::{again, plural, split_rev_and_paths, Budget, Opts, Outcome};
use crate::TOOL;

pub fn run(args: &[String], opts: &Opts) -> Result<Outcome, Fail> {
    let mut report = Report::new();
    if let Some(asked) = opts.limit_clamped_from {
        report.bound(format!("--limit {asked} clamped to {MAX_LIMIT}"));
    }

    let (rev, paths) = split_rev_and_paths(args, opts)?;
    // With no revision, HEAD — stated rather than implied, so the header says what was walked.
    let rev = match rev {
        Some(rev) => Some(rev),
        None if opts.git.head_exists() => Some("HEAD".to_string()),
        None => None,
    };

    let target = match (&rev, paths.len()) {
        (Some(rev), 0) => rev.clone(),
        (Some(rev), 1) => format!("{rev} · {}", paths[0]),
        (Some(rev), n) => format!("{rev} · {}", plural(n, "path")),
        // No commits is a fact about the repository rather than an error, so it is said in the
        // header and the exit code carries it: 1, read fine, nothing to walk.
        (None, _) => "HEAD · no commits yet".to_string(),
    };
    report.header(TOOL, "log", opts.root.name(), &target);

    let Some(rev) = rev else {
        return Ok(Outcome::from_report(report));
    };

    let commits = facts::commits(&opts.git, &rev, &paths, opts.limit)?;
    let shown = push(&mut report, opts, &commits);

    if shown < commits.total {
        report.next(
            again(
                opts,
                "log",
                Some(&rev),
                &paths,
                &[format!("--limit {}", commits.total.min(MAX_LIMIT))],
            ),
            format!("all {}", plural(commits.total, "commit")),
        );
    }

    Ok(Outcome::from_report(report))
}

/// Print the rows and the count under them, and return how many rows were printed. Shared with the
/// `pr` recipe, so a commit line means the same thing in both.
pub fn push(report: &mut Report, opts: &Opts, commits: &Commits) -> usize {
    let mut budget = Budget::new(opts.limit);
    for line in commits.lines() {
        budget.push(report, line);
    }
    let shown = commits.rows.len() - budget.dropped();
    report.bound(if shown == commits.total {
        plural(commits.total, "commit")
    } else {
        format!(
            "{shown} of {} · --limit {}",
            plural(commits.total, "commit"),
            opts.limit
        )
    });
    budget.width_caveat(report);
    shown
}
