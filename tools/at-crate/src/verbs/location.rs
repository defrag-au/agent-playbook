//! `at-crate where` — which version the project chose, where it was extracted, and what a build
//! script generated for it.
//!
//! This is the verb that replaces `cargo info` plus a `find` over the registry. It answers with
//! three things a reader needs before any read: the exact version, the registry copy that matches
//! it, and the target directories that may hold generated code. When those cannot all be
//! established it says which link is missing rather than guessing one.

use at_core::contract::Fail;

use crate::project::SourceKind;
use crate::verbs::{self, Options};
use crate::walk;

pub fn run(args: &[String], opts: &Options) -> Result<verbs::Outcome, Fail> {
    let query = verbs::bare_crate(verbs::exactly_one(args, "where", "crate name")?, "where")?;
    let store = opts.store()?;
    let project = opts.project()?;
    let listing = store.installed()?;
    let resolved = verbs::resolve(query, opts, &store, &project, &listing)?;

    let mut report = verbs::report_for(opts, "where", &resolved.label(), "source");

    match &resolved.kind {
        SourceKind::Registry => {
            if let Some(dir) = &resolved.dir {
                report.content(dir.display().to_string());
                if let Some(registry) = dir
                    .parent()
                    .and_then(|parent| parent.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
                {
                    report.bound(format!("registry {registry}"));
                }
            }
        }
        SourceKind::Git { url, rev } => {
            if let Some(dir) = &resolved.dir {
                report.content(dir.display().to_string());
                report.bound(format!(
                    "git {url} · rev {}",
                    rev.as_deref().unwrap_or("unknown")
                ));
            } else {
                report.bound(format!(
                    "git {url} · rev {} · no extracted checkout under {}",
                    rev.as_deref().unwrap_or("unknown"),
                    store.home().join("git/checkouts").display()
                ));
            }
        }
        SourceKind::Path => {
            report.bound(
                "path package · Cargo.lock has no source for it, so there is no registry copy; \
                 read it from the workspace",
            );
        }
        SourceKind::Unknown(source) => {
            report.bound(format!(
                "lock source `{source}` is not a registry or git source this tool reads"
            ));
        }
    }

    let origin = match (&project.lock, resolved.from_lock, opts.version.as_deref()) {
        (Some(lock), true, _) => format!("resolved from {}", lock.path.display()),
        (Some(lock), false, Some(version)) => format!(
            "not the lock's choice in {} · --version {version} named this extracted copy",
            lock.path.display()
        ),
        (Some(lock), false, None) => format!(
            "not in {} · this is the only extracted copy",
            lock.path.display()
        ),
        (None, _, Some(version)) => format!(
            "no Cargo.lock above the working directory · --version {version} named this extracted copy"
        ),
        (None, _, None) => "no Cargo.lock above the working directory · this is the only extracted \
                            copy"
            .to_string(),
    };
    report.bound(origin);

    if let Some(real) = &resolved.spelling {
        report.bound(format!("resolved `{query}` to `{real}`"));
    }
    if listing.unreadable > 0 {
        report.bound(format!(
            "caveat: {} not searched",
            at_core::contract::plural_of(
                listing.unreadable,
                "unreadable registry directory",
                "unreadable registry directories"
            )
        ));
    }

    let mut build_dirs = Vec::new();
    match project.target_dir(opts.target_dir.as_deref(), &opts.cwd)? {
        Some(target) => {
            build_dirs = walk::build_outs(&target)
                .into_iter()
                .filter(|out| {
                    verbs::normalised(&out.crate_name) == verbs::normalised(&resolved.name)
                })
                .collect();
            if build_dirs.is_empty() {
                report.bound(format!(
                    "no build-script output under {} · a past build would have left one",
                    target.display()
                ));
            } else {
                for out in build_dirs.iter().take(opts.limit) {
                    report.bound(format!(
                        "build output: {}",
                        out.label(project.root.as_deref())
                    ));
                }
                if build_dirs.len() > opts.limit {
                    report.bound(format!(
                        "{} of {} build directories shown · --limit {}",
                        opts.limit,
                        build_dirs.len(),
                        opts.limit
                    ));
                }
            }
        }
        None => {
            report.bound(
                "no target directory found · --project <path> or --target-dir <path> names one",
            );
        }
    }

    if resolved.dir.is_some() {
        report.next(
            opts.again(
                "files",
                std::slice::from_ref(&resolved.name),
                &["--depth 2".to_string()],
            ),
            "what is in it",
        );
        if !build_dirs.is_empty() {
            report.next(
                opts.again("out", std::slice::from_ref(&resolved.name), &[]),
                "the generated files",
            );
        }
    } else {
        report.next(
            opts.again("installed", std::slice::from_ref(&resolved.name), &[]),
            "every extracted version",
        );
    }

    Ok(verbs::Outcome::from_frame(report))
}
