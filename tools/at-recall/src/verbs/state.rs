//! `at-recall state` — what am I looking at.
//!
//! The verb an agent runs before it decides anything, and the replacement for the
//! `git status` + `git branch` + `git log -1` pipeline that costs three approvals and prints five
//! times more than it answers. It answers in the order the questions are asked: which branch, which
//! commit, is something half-finished, what has changed.
//!
//! The in-progress check is a read of the files git itself uses to remember a merge, rebase,
//! cherry-pick, revert or bisect. They are named in [`PENDING`] *and* in the help, so "none" is a
//! claim about a known list rather than a shrug.

use at_core::contract::{plural_of, Fail, Report, MAX_LIMIT};

use crate::git::{Noun, GIT};
use crate::status::Status;
use crate::verbs::{again, plural, Budget, Opts, Outcome};
use crate::TOOL;

/// The files git leaves behind when an operation is half-done, and what each one means. A list
/// rather than a probe per case: one `git rev-parse --git-path` invocation resolves all of them.
pub const PENDING: &[(&str, &str)] = &[
    ("MERGE_HEAD", "merge"),
    ("rebase-merge", "rebase"),
    ("rebase-apply", "rebase"),
    ("CHERRY_PICK_HEAD", "cherry-pick"),
    ("REVERT_HEAD", "revert"),
    ("BISECT_LOG", "bisect"),
];

pub fn run(opts: &Opts) -> Result<Outcome, Fail> {
    let mut report = Report::new();
    if let Some(asked) = opts.limit_clamped_from {
        report.bound(format!("--limit {asked} clamped to {MAX_LIMIT}"));
    }

    let status = Status::read(&opts.git)?;
    let head = match status.oid() {
        Some(_) => Some(Head::read(opts)?),
        None => None,
    };

    report.header(
        TOOL,
        "state",
        opts.root.name(),
        &match &head {
            Some(head) => format!("{} · {}", status.branch(), head.short),
            None => format!("{} · no commits yet", status.branch()),
        },
    );

    if let Some(head) = &head {
        report.content(format!(
            "{:<9}  {}  {}  {}",
            "head", head.date, head.author, head.subject
        ));
    }
    report.content(format!("{:<9}  {}", "upstream", status.upstream()));
    report.content(format!("{:<9}  {}", "pending", pending(opts)?.join(", ")));

    let mut budget = Budget::new(opts.limit);
    for line in status.lines() {
        budget.push(&mut report, line);
    }

    let total = status.len();
    let shown = total - budget.dropped();
    report.bound(if shown == total {
        plural(total, "path")
    } else {
        format!(
            "{shown} of {} · --limit {}",
            plural(total, "path"),
            opts.limit
        )
    });
    if status.collapsed_directories() > 0 {
        let collapsed = status.collapsed_directories();
        report.bound(format!(
            "caveat: {} shown collapsed, ending in `/`",
            plural_of(collapsed, "untracked directory", "untracked directories")
        ));
    }
    if status.unparsed() > 0 {
        report.bound(format!(
            "caveat: {} status record(s) did not parse and are not listed",
            status.unparsed()
        ));
    }
    budget.width_caveat(&mut report);

    // The exits, in the contract's order: widen a cut answer, then the one question this answer
    // implies. A clean tree offers neither, which is the useful reading of a silent footer.
    if budget.dropped() > 0 {
        report.next(
            again(
                opts,
                "state",
                None,
                &[],
                &[format!("--limit {}", total.min(MAX_LIMIT))],
            ),
            format!("all {}", plural(total, "path")),
        );
    } else if status.tracks_changes() {
        report.next(again(opts, "diff", None, &[], &[]), "what changed in them");
    }

    Ok(Outcome::from_report(report))
}

/// HEAD, as git describes it in one `log -1`.
struct Head {
    short: String,
    date: String,
    author: String,
    subject: String,
}

impl Head {
    /// `%s` is a single line by construction, so `log -1` is always one line and the four fields can
    /// be split on a separator no name can contain.
    fn read(opts: &Opts) -> Result<Head, Fail> {
        let raw = opts.git.run(
            Noun::Log,
            &[
                "-1",
                // Explicit, because `log.showSignature` in a repository's own config would make git
                // verify the signature — a subprocess this tool did not agree to run.
                "--no-show-signature",
                "--format=%h%x1f%as%x1f%an%x1f%s",
            ],
        )?;
        let line = raw.trim_end_matches('\n');
        let fields: Vec<&str> = line.split('\u{1f}').collect();
        match fields[..] {
            [short, date, author, subject] => Ok(Head {
                short: short.to_string(),
                date: date.to_string(),
                author: author.to_string(),
                subject: subject.to_string(),
            }),
            // A format that did not render as written is not something to guess at: reporting the
            // wrong commit as HEAD is worse than reporting that this could not be read.
            _ => Err(Fail::environment(format!(
                "{GIT} log printed {} fields where the requested format has 4",
                fields.len()
            ))),
        }
    }
}

/// Which of the half-finished operations are in progress, by the files git leaves in `.git`.
/// `--git-path` resolves them in one invocation and against the real git directory, so a linked
/// worktree — where they are not under `.git` at the top level — reports correctly.
fn pending(opts: &Opts) -> Result<Vec<String>, Fail> {
    let mut args: Vec<String> = Vec::new();
    for (file, _) in PENDING {
        args.push("--git-path".to_string());
        args.push((*file).to_string());
    }
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    let raw = opts.git.run(Noun::RevParse, &borrowed)?;
    let lines: Vec<&str> = raw.lines().collect();
    // One line per `--git-path`, in the order asked. A different count means git answered a
    // different question, and pairing the answers up anyway would name the wrong operation.
    if lines.len() != PENDING.len() {
        return Err(Fail::environment(format!(
            "{GIT} rev-parse --git-path printed {} paths where {} were asked for",
            lines.len(),
            PENDING.len()
        )));
    }

    let mut found: Vec<String> = Vec::new();
    for (path, operation) in lines.iter().zip(PENDING.iter()) {
        let path = std::path::Path::new(path);
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            opts.root.dir().join(path)
        };
        if absolute.exists() && !found.iter().any(|seen| seen == operation.1) {
            found.push(operation.1.to_string());
        }
    }
    if found.is_empty() {
        found.push("none".to_string());
    }
    Ok(found)
}
