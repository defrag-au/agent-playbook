//! `at-crate slice` — the lines you asked for, numbered, with the file's true size attached.
//!
//! The `sed -n '80,210p'` replacement for installed source, and the same shape `at-peek slice`
//! gives a working tree: every slice states the file's real size, and a range that was cut offers
//! the range that resumes as a command rather than as prose. `--build` reads the same
//! crate-relative path under a build-script `out` directory, which is where generated protocol
//! code lives.

use std::path::{Path, PathBuf};

use at_core::contract::{truncate, Fail, Report, MAX_LINE_WIDTH};
use at_core::paths::Range;
use at_core::text::{read_text, Content};

use crate::project::{Project, Resolved};
use crate::store::{Listing, Store};
use crate::target::{self, CrateTarget};
use crate::verbs::{self, build, plural, Options};
use crate::walk;
use crate::TOOL;

enum Located {
    Source { resolved: Resolved, path: PathBuf },
    Build { out: walk::BuildOut, path: PathBuf },
}

impl Located {
    fn path(&self) -> &Path {
        match self {
            Located::Source { path, .. } | Located::Build { path, .. } => path,
        }
    }

    fn header(&self, report: &mut Report, target: &CrateTarget, project: &Project) {
        match self {
            Located::Source { resolved, .. } => {
                report.header(TOOL, "slice", &resolved.label(), &target.describe());
            }
            Located::Build { out, .. } => {
                report.header(
                    TOOL,
                    "slice",
                    &out.label(project.root.as_deref()),
                    &target.describe(),
                );
            }
        }
    }
}

pub fn run(args: &[String], opts: &Options) -> Result<verbs::Outcome, Fail> {
    if args.is_empty() {
        return Err(Fail::usage(
            "`slice` needs a target · at-crate slice <crate>/<path>:<range> [--build]",
        ));
    }

    let store = if opts.build {
        None
    } else {
        Some(opts.store()?)
    };
    let project = if opts.build {
        opts.project_root()?
    } else {
        opts.project()?
    };
    let listing = match &store {
        Some(store) => Some(store.installed()?),
        None => None,
    };
    let source = store.as_ref().zip(listing.as_ref());
    let target_dir = if opts.build {
        project.target_dir(opts.target_dir.as_deref(), &opts.cwd)?
    } else {
        None
    };

    let mut report = Report::new();
    verbs::announce_clamps(&mut report, opts);
    let mut budget = opts.limit;
    let mut unfinished: Vec<String> = Vec::new();

    for (index, arg) in args.iter().enumerate() {
        if budget == 0 {
            report.bound(format!(
                "--limit {} reached · {} not shown",
                opts.limit,
                plural(args.len() - index, "target")
            ));
            unfinished.extend(args[index..].iter().cloned());
            break;
        }

        let target = CrateTarget::parse(arg)?;
        let located = locate(&target, opts, source, &project, target_dir.as_deref())?;
        let text = match read_text(located.path(), &target.describe())? {
            Content::Text(text) => text,
            Content::Binary(offset) => {
                located.header(&mut report, &target, &project);
                report.bound(format!(
                    "caveat: binary file, NUL at byte {offset} · not line-addressable"
                ));
                continue;
            }
            Content::NotUtf8(offset) => {
                return Err(Fail::environment(format!(
                    "{} is not UTF-8 text (first bad byte at offset {offset})",
                    target.describe()
                )))
            }
        };

        located.header(&mut report, &target, &project);
        let total = text.lines().count();
        if total == 0 {
            report.bound(format!("{} is empty", target.describe()));
            continue;
        }

        let (start, asked) = match target.range {
            Some(range) => (range.start(), range.requested_lines()),
            None => (1, total),
        };
        if start > total {
            report.bound(format!(
                "line {start} is past the end of {} ({total} lines)",
                target.describe()
            ));
            continue;
        }
        if start.saturating_add(asked).saturating_sub(1) > total {
            report.bound(format!(
                "the requested range ends past the file's {total} lines"
            ));
        }

        let wanted = asked.min(total - start + 1);
        let end = start + wanted - 1;
        let shown = wanted.min(budget);
        let number_width = (start + shown - 1).to_string().len();

        let mut truncated = 0;
        for (offset, line) in text.lines().skip(start - 1).take(shown).enumerate() {
            let number = start + offset;
            let (text, cut) = truncate(line);
            if cut {
                truncated += 1;
            }
            report.content(format!("{number:>number_width$}  {text}"));
        }
        budget -= shown;

        report.bound(if shown == total {
            format!("{} · whole file", plural(total, "line"))
        } else {
            format!("{shown} of {total} lines")
        });
        if truncated > 0 {
            report.bound(format!(
                "caveat: {truncated} line(s) wider than {MAX_LINE_WIDTH} characters, truncated"
            ));
        }

        if shown < wanted {
            let next = start + shown;
            report.bound(format!(
                "{shown} of {wanted} requested lines · --limit {} reached",
                opts.limit
            ));
            unfinished.push(target.next_range(Range::Span(next, end)));
            unfinished.extend(args[index + 1..].iter().cloned());
            let after = args.len() - index - 1;
            if after > 0 {
                report.bound(format!("{} not shown", plural(after, "target")));
            }
            break;
        }
        if end < total {
            unfinished.push(target.next_range(Range::Span(end + 1, total)));
        }
    }

    if !unfinished.is_empty() {
        report.next(
            opts.again("slice", &unfinished, &[]),
            "the lines that follow",
        );
    }

    Ok(verbs::Outcome::from_report(report))
}

fn locate(
    target: &CrateTarget,
    opts: &Options,
    source: Option<(&Store, &Listing)>,
    project: &Project,
    target_dir: Option<&Path>,
) -> Result<Located, Fail> {
    match source {
        Some((store, listing)) => {
            let resolved = verbs::resolve(&target.crate_name, opts, store, project, listing)?;
            let root = resolved.dir.clone().ok_or_else(|| {
                Fail::environment(format!(
                    "{} has no extracted source directory · it is not a registry or resolved git \
                     package",
                    resolved.label()
                ))
            })?;
            let path = target::resolve_in(&root, &target.inner, &resolved.name)?;
            if !path.is_file() {
                return Err(Fail::environment(format!(
                    "{} is a directory · name a file inside it",
                    target.describe()
                )));
            }
            verbs::check_secret(&path, opts, &target.describe())?;
            Ok(Located::Source { resolved, path })
        }
        None => {
            let (out, path) = build::locate(target, project, target_dir, opts)?;
            verbs::check_secret(&path, opts, &target.describe())?;
            Ok(Located::Build { out, path })
        }
    }
}
