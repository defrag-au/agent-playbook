//! `at-recall log` — what changed lately.
//!
//! The question an agent asks to place itself: "has this file been touched since I read it", "how
//! recent is this design doc", "what is this branch's work". It is one line per commit, which is the
//! form `git log --oneline` and the `| head -4` habit both reach for — except the bound is stated,
//! and the count comes from `rev-list` over the *same* revision and paths, so the total under the
//! listing is the number of commits the listing was drawn from rather than a guess.
//!
//! Two things it deliberately does not do yet: filter by author or date, and follow a file's
//! renames. Both are flags on this question rather than new questions, and neither showed up in a
//! session — so they are not in the grammar until they do.

use at_core::contract::{Fail, Report, MAX_LIMIT};

use crate::git::{with_paths, Noun};
use crate::verbs::{again, plural, split_rev_and_paths, Budget, Opts, Outcome};
use crate::TOOL;

/// One commit, as four fields. `%s` is a single line by construction, so a commit is a line and the
/// separator can be a byte no subject contains.
const FORMAT: &str = "--format=%h%x1f%as%x1f%an%x1f%s";

const SEPARATOR: char = '\u{1f}';

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

    let total = count(opts, &rev, &paths)?;
    let commits = walk(opts, &rev, &paths, opts.limit)?;

    let mut budget = Budget::new(opts.limit);
    let widths = Widths::of(&commits);
    for commit in &commits {
        budget.push(&mut report, widths.row(commit));
    }

    let shown = commits.len() - budget.dropped();
    report.bound(if shown == total {
        plural(total, "commit")
    } else {
        format!(
            "{shown} of {} · --limit {}",
            plural(total, "commit"),
            opts.limit
        )
    });
    budget.width_caveat(&mut report);

    if shown < total {
        report.next(
            again(
                opts,
                "log",
                Some(&rev),
                &paths,
                &[format!("--limit {}", total.min(MAX_LIMIT))],
            ),
            format!("all {}", plural(total, "commit")),
        );
    }

    Ok(Outcome::from_report(report))
}

/// How many commits the same revision and paths hold, from the traversal that formats nothing.
///
/// `rev-list` walks what `log` walks — asserted by `the_count_matches_the_listing` against a
/// fixture with a merge in it, because a total that quietly disagrees with the rows above it is the
/// failure this toolkit exists to remove.
fn count(opts: &Opts, rev: &str, paths: &[String]) -> Result<usize, Fail> {
    let mut args: Vec<String> = vec!["--count".to_string(), rev.to_string()];
    with_paths(&mut args, paths);
    let raw = opts.git.run(Noun::RevList, &borrowed(&args))?;
    raw.trim().parse::<usize>().map_err(|_| {
        Fail::environment(format!(
            "git rev-list --count printed `{}`, which is not a number",
            raw.trim()
        ))
    })
}

/// The commits to show, newest first.
fn walk(opts: &Opts, rev: &str, paths: &[String], limit: usize) -> Result<Vec<Commit>, Fail> {
    let mut args: Vec<String> = vec![
        // Explicit, because `log.showSignature` in a repository's own config would make git verify
        // the signature — a subprocess this tool did not agree to run.
        "--no-show-signature".to_string(),
        format!("--max-count={limit}"),
        FORMAT.to_string(),
        rev.to_string(),
    ];
    with_paths(&mut args, paths);
    let raw = opts.git.run(Noun::Log, &borrowed(&args))?;

    let mut commits = Vec::new();
    for line in raw.lines().filter(|line| !line.is_empty()) {
        let fields: Vec<&str> = line.split(SEPARATOR).collect();
        match fields[..] {
            [short, date, author, subject] => commits.push(Commit {
                short: short.to_string(),
                date: date.to_string(),
                author: author.to_string(),
                subject: subject.to_string(),
            }),
            _ => {
                return Err(Fail::environment(format!(
                    "{} printed {} fields where the requested format has 4",
                    crate::git::GIT,
                    fields.len()
                )))
            }
        }
    }
    Ok(commits)
}

struct Commit {
    short: String,
    date: String,
    author: String,
    subject: String,
}

/// Column widths from the commits being shown, so the output is as narrow as its data allows and
/// identical for identical input — the shape `at-peek stat` uses.
struct Widths {
    short: usize,
    author: usize,
}

impl Widths {
    fn of(commits: &[Commit]) -> Widths {
        let mut widths = Widths {
            short: 1,
            author: 1,
        };
        for commit in commits {
            widths.short = widths.short.max(commit.short.len());
            widths.author = widths.author.max(commit.author.len());
        }
        widths
    }

    fn row(&self, commit: &Commit) -> String {
        format!(
            "{short:<shortw$}  {date}  {author:<authorw$}  {subject}",
            short = commit.short,
            date = commit.date,
            author = commit.author,
            subject = commit.subject,
            shortw = self.short,
            authorw = self.author,
        )
    }
}

fn borrowed(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}
