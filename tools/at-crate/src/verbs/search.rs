//! `at-crate search` — every mention in installed source or build output, bounded by construction.
//!
//! The `grep -rn … ~/.cargo/registry/src/...` replacement. It differs from grep in two ways that
//! matter for a transcript: it counts every match the walk considers, even after the listing is
//! full, and it prints a `# next:` command that widens or deepens the same question. Rows carry
//! the crate and the path inside it, so a hit can be cited without reconstructing the registry
//! path by hand.

use std::collections::BTreeSet;
use std::path::PathBuf;

use at_core::contract::{truncate, Fail, Report, MAX_LINE_WIDTH};
use at_core::text::{read_text, Content};
use regex::Regex;

use crate::project::Project;
use crate::store::{Listing, Store};
use crate::target::{self, CrateTarget};
use crate::verbs::{self, build, plural, Options};
use crate::walk;
use crate::TOOL;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Matches,
    Count,
    FilesOnly,
}

impl Mode {
    fn of(opts: &Options) -> Mode {
        if opts.count {
            Mode::Count
        } else if opts.files_only {
            Mode::FilesOnly
        } else {
            Mode::Matches
        }
    }

    fn flag(self) -> Option<String> {
        match self {
            Mode::Matches => None,
            Mode::Count => Some("--count".to_string()),
            Mode::FilesOnly => Some("--files-only".to_string()),
        }
    }
}

struct Scope {
    /// The file or directory the walk starts from.
    root: PathBuf,
    /// The text every row starts with. A file scope is the display path exactly; a directory
    /// scope has the walked file's relative path appended.
    display: String,
    /// One line for the header.
    label: String,
    is_file: bool,
}

#[derive(Default)]
struct Aggregates {
    files: usize,
    capped: bool,
    symlinks: usize,
    unreadable: usize,
    secret_skipped: usize,
    binary: usize,
    not_utf8: usize,
    truncated_lines: usize,
    skipped_dirs: BTreeSet<&'static str>,
    total: usize,
    seen: BTreeSet<PathBuf>,
}

pub fn run(args: &[String], opts: &Options) -> Result<verbs::Outcome, Fail> {
    let Some((pattern, named)) = args.split_first() else {
        return Err(Fail::usage(
            "`search` needs a pattern · at-crate search <pattern> <crate>[/<path>]",
        ));
    };
    if named.is_empty() {
        return Err(Fail::usage(
            "`search` needs a crate to search · at-crate search <pattern> <crate>[/<path>]",
        ));
    }
    let regex = Regex::new(pattern).map_err(|error| {
        Fail::usage(format!(
            "`{pattern}` is not a valid regex: {}",
            complaint(&error)
        ))
    })?;
    let mode = Mode::of(opts);

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

    let mut aggregates = Aggregates::default();
    let mut rows: Vec<(String, usize, String)> = Vec::new();
    let mut counted: Vec<(String, usize)> = Vec::new();
    let mut labels = BTreeSet::new();

    for arg in named {
        let target = CrateTarget::parse(arg)?;
        if target.range.is_some() {
            return Err(Fail::usage(format!(
                "`{arg}` has a line range; search takes a path, and slice takes the range"
            )));
        }
        let scopes = scopes(&target, opts, source, &project, target_dir.as_deref())?;
        for scope in scopes {
            labels.insert(scope.label.clone());
            let walk = walk::files(&scope.root, opts.max_files, opts.include_secret_paths)?;
            aggregates.files += walk.considered;
            aggregates.capped |= walk.capped;
            aggregates.symlinks += walk.symlinks;
            aggregates.unreadable += walk.unreadable;
            aggregates.secret_skipped += walk.secret_skipped;
            aggregates.skipped_dirs.extend(walk.skipped_dirs);

            for file in walk.files {
                if !aggregates.seen.insert(file.path.clone()) {
                    continue;
                }
                let display = if scope.is_file {
                    scope.display.clone()
                } else {
                    format!("{}/{}", scope.display, file.rel)
                };
                let text = match read_text(&file.path, &display) {
                    Ok(Content::Text(text)) => text,
                    Ok(Content::Binary(_)) => {
                        aggregates.binary += 1;
                        continue;
                    }
                    Ok(Content::NotUtf8(_)) => {
                        aggregates.not_utf8 += 1;
                        continue;
                    }
                    Err(_) => {
                        aggregates.unreadable += 1;
                        continue;
                    }
                };

                let mut hits = 0;
                for (index, line) in text.lines().enumerate() {
                    if !regex.is_match(line) {
                        continue;
                    }
                    hits += 1;
                    aggregates.total += 1;
                    if mode == Mode::Matches && rows.len() < opts.limit {
                        let (rendered, cut) = truncate(line);
                        if cut {
                            aggregates.truncated_lines += 1;
                        }
                        rows.push((display.clone(), index + 1, rendered));
                    }
                }
                if hits > 0 {
                    counted.push((display, hits));
                }
            }
        }
    }

    let mut report = Report::new();
    verbs::announce_clamps(&mut report, opts);
    let scope_count = labels.len();
    let scope_text = if scope_count == 1 {
        labels.into_iter().next().unwrap_or_default()
    } else {
        format!("{scope_count} scopes")
    };
    let where_text = if named.len() == 1 {
        named[0].clone()
    } else {
        plural(named.len(), "target")
    };
    report.header(
        TOOL,
        "search",
        &scope_text,
        &format!("/{pattern}/ in {where_text}"),
    );

    let listed = match (opts.summary, mode) {
        (true, Mode::Matches) => aggregates.total,
        (true, Mode::Count | Mode::FilesOnly) => counted.len(),
        (false, Mode::Matches) => {
            for (display, line, text) in &rows {
                report.content(format!("{display}:{line}:{text}"));
            }
            rows.len()
        }
        (false, Mode::Count) => {
            let shown = counted.len().min(opts.limit);
            let paths = counted
                .iter()
                .take(shown)
                .map(|(display, _)| display.len())
                .max()
                .unwrap_or(0);
            let hits = counted
                .iter()
                .take(shown)
                .map(|(_, hits)| hits.to_string().len())
                .max()
                .unwrap_or(1);
            for (display, count) in counted.iter().take(shown) {
                report.content(format!("{display:<paths$}  {count:>hits$}"));
            }
            shown
        }
        (false, Mode::FilesOnly) => {
            let shown = counted.len().min(opts.limit);
            for (display, _) in counted.iter().take(shown) {
                report.content(display.clone());
            }
            shown
        }
    };

    report.bound(walk_bound(&aggregates, opts.max_files));
    if aggregates.truncated_lines > 0 {
        report.bound(format!(
            "caveat: {} line(s) wider than {MAX_LINE_WIDTH} characters, truncated",
            aggregates.truncated_lines
        ));
    }
    report.bound(coverage(
        mode,
        listed,
        &counted,
        aggregates.total,
        opts.limit,
    ));
    if opts.summary && listed > 0 {
        report.bound("--summary · the rows were withheld");
    }
    if opts.build && opts.build_dir.is_none() && scope_count > 1 {
        report.bound(format!(
            "{scope_count} build directories searched · --build-dir <path> narrows to one"
        ));
    }

    let mut flags: Vec<String> = mode.flag().into_iter().collect();
    let rows_total = match mode {
        Mode::Matches => aggregates.total,
        Mode::Count | Mode::FilesOnly => counted.len(),
    };
    if opts.summary {
        if rows_total > 0 {
            flags.push(format!(
                "--limit {}",
                rows_total.min(at_core::contract::MAX_LIMIT)
            ));
            report.next(
                opts.again("search", &positionals(pattern, named), &flags),
                "the rows the frame withheld",
            );
        }
    } else if listed < rows_total {
        flags.push(format!(
            "--limit {}",
            rows_total.min(at_core::contract::MAX_LIMIT)
        ));
        report.next(
            opts.again("search", &positionals(pattern, named), &flags),
            "all of them",
        );
    }
    if aggregates.capped {
        let mut width = mode.flag().into_iter().collect::<Vec<_>>();
        width.push(format!(
            "--max-files {}",
            opts.max_files
                .saturating_mul(2)
                .min(at_core::contract::MAX_FILES)
        ));
        report.next(
            opts.again("search", &positionals(pattern, named), &width),
            "twice the walk",
        );
    }

    Ok(if opts.summary && listed > 0 {
        verbs::Outcome::from_frame(report)
    } else {
        verbs::Outcome::from_report(report)
    })
}

fn positionals(pattern: &str, named: &[String]) -> Vec<String> {
    let mut parts = vec![pattern.to_string()];
    parts.extend(named.iter().cloned());
    parts
}

fn scopes(
    target: &CrateTarget,
    opts: &Options,
    source: Option<(&Store, &Listing)>,
    project: &Project,
    target_dir: Option<&std::path::Path>,
) -> Result<Vec<Scope>, Fail> {
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
            let abs = target::resolve_in(&root, &target.inner, &resolved.name)?;
            if abs.is_file() {
                verbs::check_secret(&abs, opts, &target.describe())?;
            }
            Ok(vec![Scope {
                is_file: abs.is_file(),
                root: abs,
                display: target.path(),
                label: resolved.label(),
            }])
        }
        None => {
            let outs = build::search_roots(target, target_dir, opts)?;
            let mut scopes = Vec::new();
            for out in outs {
                let Ok(abs) = target::resolve_in(&out.out, &target.inner, &target.crate_name)
                else {
                    continue;
                };
                if abs.is_file() {
                    verbs::check_secret(&abs, opts, &target.describe())?;
                }
                let display = match project.root.as_deref() {
                    Some(root) => walk::relative(root, &abs),
                    None => abs.display().to_string(),
                };
                scopes.push(Scope {
                    is_file: abs.is_file(),
                    root: abs,
                    display,
                    label: format!("{} build/{}", out.crate_name, out.hash),
                });
            }
            if scopes.is_empty() {
                let Some(target_dir) = target_dir else {
                    return Err(Fail::environment(
                        "no target directory found · --project <path> or --target-dir <path> names one",
                    ));
                };
                return Err(build::missing(target, project, target_dir, opts));
            }
            Ok(scopes)
        }
    }
}

fn walk_bound(aggregates: &Aggregates, max_files: usize) -> String {
    let mut text = format!("walked {} files", aggregates.files);
    if !aggregates.skipped_dirs.is_empty() {
        let names: Vec<&str> = aggregates.skipped_dirs.iter().copied().collect();
        text.push_str(&format!(" · skipped by rule: {}", names.join(", ")));
    }
    if aggregates.capped {
        text.push_str(&format!(" · stopped at --max-files {max_files}"));
    }
    let mut skipped = Vec::new();
    if aggregates.secret_skipped > 0 {
        skipped.push(format!("{} secret-shaped", aggregates.secret_skipped));
    }
    if aggregates.binary > 0 {
        skipped.push(format!("{} binary", aggregates.binary));
    }
    if aggregates.not_utf8 > 0 {
        skipped.push(format!("{} not UTF-8", aggregates.not_utf8));
    }
    if aggregates.symlinks > 0 {
        skipped.push(format!("{} symbolic link(s)", aggregates.symlinks));
    }
    if aggregates.unreadable > 0 {
        skipped.push(format!("{} unreadable", aggregates.unreadable));
    }
    if !skipped.is_empty() {
        text.push_str(&format!(" · skipped: {}", skipped.join(" · ")));
    }
    text
}

fn coverage(
    mode: Mode,
    listed: usize,
    counted: &[(String, usize)],
    total: usize,
    limit: usize,
) -> String {
    let files = plural(counted.len(), "file");
    let matches = if total == 1 {
        "1 match".to_string()
    } else {
        format!("{total} matches")
    };
    match mode {
        Mode::Matches => {
            if listed < total {
                format!("{listed} of {matches} in {files} · --limit {limit}")
            } else {
                format!("{matches} in {files}")
            }
        }
        Mode::Count => {
            if listed < counted.len() {
                format!(
                    "{matches} in {files} · {listed} of {} files shown (--limit {limit})",
                    counted.len()
                )
            } else {
                format!("{matches} in {files}")
            }
        }
        Mode::FilesOnly => {
            if listed < counted.len() {
                format!(
                    "{files} · {matches} · {listed} of {} shown (--limit {limit})",
                    counted.len()
                )
            } else {
                format!("{files} · {matches}")
            }
        }
    }
}

/// The regex crate reports a parse error as a four-line diagram. A usage message can carry one
/// line, and its last line is the part that says what is actually wrong.
fn complaint(error: &regex::Error) -> String {
    error
        .to_string()
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("invalid pattern")
        .trim()
        .trim_start_matches("error: ")
        .to_string()
}
