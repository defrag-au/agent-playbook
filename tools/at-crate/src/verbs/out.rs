//! `at-crate out` — what a build script generated, without running one.
//!
//! Build output lives under `target/**/build/<crate>-<hash>/out`, and the hash is opaque. This
//! verb lists those directories and their files in path order, and says how many there are. It
//! never claims a directory is the current build: a stale `out` from an earlier feature set is a
//! directory like any other, and `--build-dir` is how a reader chooses.

use std::fs;

use at_core::contract::{iso8601_utc, Fail};

use crate::verbs::{self, Options};
use crate::walk;

pub fn run(args: &[String], opts: &Options) -> Result<verbs::Outcome, Fail> {
    if args.len() > 1 {
        return Err(Fail::usage(
            "`out` takes one crate name, or none for every build-script output",
        ));
    }
    let query = match args.first().map(String::as_str) {
        Some(query) => Some(verbs::bare_crate(query, "out")?),
        None => None,
    };

    let project = opts.project_root()?;
    let target = project.target_dir(opts.target_dir.as_deref(), &opts.cwd)?;
    let Some(target) = target else {
        return Err(Fail::environment(
            "no target directory found · --project <path> or --target-dir <path> names one",
        ));
    };

    let all = walk::build_outs(&target);
    let outs: Vec<walk::BuildOut> = match query {
        Some(query) => all
            .into_iter()
            .filter(|out| verbs::normalised(&out.crate_name) == verbs::normalised(query))
            .collect(),
        None => all,
    };

    let root = query.unwrap_or("every crate");
    let mut report = verbs::report_for(opts, "out", root, "build-script output");
    let total = outs.len();
    let shown = total.min(opts.limit);

    for out in outs.iter().take(shown) {
        report.content(out.label(project.root.as_deref()));
        let files = walk::files(&out.out, opts.limit, opts.include_secret_paths)?;
        for file in files.files.iter().take(opts.limit) {
            report.content(format!("  {}  {}", file.rel, verbs::size(file.len)));
        }
        let mut bounds = vec![format!("{} files", files.files.len())];
        if files.capped {
            bounds.push(format!("--limit {} reached", opts.limit));
        }
        if files.secret_skipped > 0 {
            bounds.push(format!("{} secret-shaped not listed", files.secret_skipped));
        }
        if files.unreadable > 0 {
            bounds.push(format!("{} unreadable", files.unreadable));
        }
        report.bound(format!("  {}", bounds.join(" · ")));
    }

    let newest = outs
        .iter()
        .filter_map(|out| fs::metadata(&out.out).ok()?.modified().ok())
        .max();
    let mut bounds = vec![format!(
        "{} of {} build directories · target {}",
        shown,
        total,
        target.display()
    )];
    if let Some(newest) = newest {
        bounds.push(format!("newest written {}", iso8601_utc(newest)));
    }
    if total == 0 {
        bounds.push("no build-script output · build first".to_string());
    } else if total > 1 {
        bounds.push("--build-dir <path> chooses one for slice or search".to_string());
    }
    report.bound(bounds.join(" · "));

    if let Some(query) = query {
        if total == 1 {
            if let Some(first) = first_file(&outs[0].out, opts)? {
                report.next(
                    opts.again(
                        "slice",
                        &[format!("{query}/{}:1-40", first)],
                        &["--build".to_string()],
                    ),
                    "read the generated file",
                );
            }
        }
    }

    Ok(verbs::Outcome::from_report(report))
}

fn first_file(out: &std::path::Path, opts: &Options) -> Result<Option<String>, Fail> {
    let files = walk::files(out, opts.limit, opts.include_secret_paths)?;
    Ok(files.files.first().map(|file| file.rel.clone()))
}
