//! `at-crate files` — a crate's shape without leaving the terminal.
//!
//! The replacement for `find <crate> -maxdepth 2 -type d` and `ls -la <crate>`. Two levels by
//! default is the resolution the browser-shot session kept asking for: enough to see `src/`,
//! `examples/`, `json/` and `benches/`, without printing every source file underneath them.

use at_core::contract::Fail;

use crate::target;
use crate::verbs::{self, Options};
use crate::walk;

pub fn run(args: &[String], opts: &Options) -> Result<verbs::Outcome, Fail> {
    let raw = verbs::exactly_one(args, "files", "crate[/path]")?;
    let target = target::CrateTarget::parse(raw)?;
    if target.range.is_some() {
        return Err(Fail::usage(format!(
            "`{raw}` has a line range; files takes a path, and slice takes the range"
        )));
    }
    let store = opts.store()?;
    let project = opts.project()?;
    let listing = store.installed()?;
    let resolved = verbs::resolve(&target.crate_name, opts, &store, &project, &listing)?;
    let root = resolved.dir.clone().ok_or_else(|| {
        Fail::environment(format!(
            "{} has no extracted source directory · it is not a registry or resolved git package",
            resolved.label()
        ))
    })?;

    let abs = target::resolve_in(&root, &target.inner, &resolved.name)?;
    verbs::check_secret(&abs, opts, &target.describe())?;
    let tree = walk::tree(&abs, opts.depth, opts.limit, opts.include_secret_paths)?;

    let mut report = verbs::report_for(opts, "files", &resolved.label(), &target.describe());
    if let Some(real) = &resolved.spelling {
        report.bound(format!("resolved `{}` to `{real}`", target.crate_name));
    }
    for entry in &tree.entries {
        let depth = entry.rel.matches('/').count();
        let indent = "  ".repeat(depth);
        if entry.is_dir {
            report.content(format!("{indent}{}/", entry.rel));
        } else {
            report.content(format!("{indent}{}  {}", entry.rel, verbs::size(entry.len)));
        }
    }

    let mut bounds = vec![format!(
        "{} entries · depth {}",
        tree.entries.len(),
        opts.depth
    )];
    if tree.not_descended > 0 {
        bounds.push(format!(
            "{} directories not descended (--depth {})",
            tree.not_descended, opts.depth
        ));
    }
    if tree.truncated {
        bounds.push(format!(
            "--limit {} reached · more entries exist",
            opts.limit
        ));
    }
    if !tree.skipped_dirs.is_empty() {
        let names: Vec<&str> = tree.skipped_dirs.iter().copied().collect();
        bounds.push(format!("skipped by rule: {}", names.join(", ")));
    }
    if tree.secret_skipped > 0 {
        bounds.push(format!("{} secret-shaped not listed", tree.secret_skipped));
    }
    if tree.symlinks > 0 {
        bounds.push(format!("{} symbolic link(s) not followed", tree.symlinks));
    }
    if tree.unreadable > 0 {
        bounds.push(format!("{} unreadable entries", tree.unreadable));
    }
    report.bound(bounds.join(" · "));

    if tree.not_descended > 0 && opts.depth < 6 {
        report.next(
            opts.again(
                "files",
                &[target.describe()],
                &[format!("--depth {}", opts.depth + 1)],
            ),
            "one level deeper",
        );
    }
    if tree.truncated {
        report.next(
            opts.again(
                "files",
                &[target.describe()],
                &[
                    format!("--depth {}", opts.depth),
                    format!(
                        "--limit {}",
                        opts.limit
                            .saturating_mul(2)
                            .min(at_core::contract::MAX_LIMIT)
                    ),
                ],
            ),
            "more entries",
        );
    }

    Ok(verbs::Outcome::from_report(report))
}
