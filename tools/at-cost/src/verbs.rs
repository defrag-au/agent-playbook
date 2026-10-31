//! The three answers: which repository costs the most, what is still running under one, and
//! which thread inside one process is hot.
//!
//! Every row is derived from a lifetime total, so the arithmetic is a division rather than a
//! sample, and the gaps — processes we could not read, arguments we replaced — are printed as
//! caveats rather than left for the reader to notice.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::contract::{plural, plural_of, Fail, Outcome, Report};
use crate::sample::{self, Proc, Snapshot};

/// How many rows a verb will print, after any clamp.
pub struct Opts {
    pub limit: usize,
    pub limit_clamped_from: Option<usize>,
}

/// A repository's share of the process table.
struct Bucket {
    label: String,
    cpu_ns: u64,
    wakeups: u64,
    procs: usize,
    orphans: usize,
    /// The age of the oldest process in the bucket: the wall time this repository has had
    /// something running for.
    oldest: u64,
}

pub fn repos(opts: &Opts) -> Result<Outcome, Fail> {
    let snapshot = sample::snapshot()?;
    let home = home();

    let mut buckets: HashMap<&Path, Vec<&Proc>> = HashMap::new();
    let mut outside = 0usize;
    let mut outside_cpu = 0u64;
    for proc in &snapshot.procs {
        match proc.repo.as_deref() {
            Some(repo) => buckets.entry(repo).or_default().push(proc),
            None => {
                outside += 1;
                outside_cpu += proc.cpu_ns;
            }
        }
    }

    let mut rows: Vec<Bucket> = buckets
        .into_iter()
        .map(|(repo, procs)| {
            let oldest = procs
                .iter()
                .filter_map(|proc| proc.age_seconds(snapshot.taken_at))
                .max()
                .unwrap_or(0);
            Bucket {
                label: label(repo, &home),
                cpu_ns: procs.iter().map(|proc| proc.cpu_ns).sum(),
                wakeups: procs.iter().map(|proc| proc.wakeups).sum(),
                procs: procs.len(),
                orphans: procs.iter().filter(|proc| proc.ppid == 1).count(),
                oldest,
            }
        })
        .collect();
    rows.sort_by(|left, right| {
        right
            .cpu_ns
            .cmp(&left.cpu_ns)
            .then_with(|| left.label.cmp(&right.label))
    });

    let total = rows.len();
    let shown = rows.len().min(opts.limit);
    let width = rows
        .iter()
        .take(shown)
        .map(|row| row.label.chars().count())
        .max()
        .unwrap_or(4)
        .clamp(4, 44);

    let mut report = Report::new();
    report.header(
        crate::TOOL,
        "repos",
        &home,
        &format!(
            "{} · {}",
            plural_of(snapshot.procs.len(), "process", "processes"),
            plural_of(total, "repository", "repositories")
        ),
    );
    report.note(format!(
        "# {:<width$}  {:>7}  {:>5}  {:>8}  {:>5}  {:>7}  {:>7}",
        "repo", "cpu", "avg%", "wakeups", "procs", "orphans", "oldest"
    ));

    for row in rows.iter().take(shown) {
        let average = if row.oldest == 0 {
            "-".to_string()
        } else {
            format!("{:.1}", row.cpu_ns as f64 / 1e9 / row.oldest as f64 * 100.0)
        };
        report.content(format!(
            "{:<width$}  {:>7}  {:>5}  {:>8}  {:>5}  {:>7}  {:>7}",
            clip(&row.label, width),
            duration(row.cpu_ns as f64 / 1e9),
            average,
            count(row.wakeups),
            row.procs,
            row.orphans,
            duration(row.oldest as f64),
        ));
    }

    if shown < total {
        report.bound(format!(
            "{shown} of {total} repositories; the rest are below the limit"
        ));
    }
    if let Some(asked) = opts.limit_clamped_from {
        report.bound(format!("--limit {asked} clamped to {}", opts.limit));
    }
    if outside > 0 {
        report.bound(format!(
            "{} have no repository ancestor · {} cpu between them",
            plural_of(outside, "process", "processes"),
            duration(outside_cpu as f64 / 1e9),
        ));
    }
    if snapshot.unreadable > 0 {
        report.bound(format!(
            "{} could not be read (another user, or a kernel task)",
            plural_of(snapshot.unreadable, "process", "processes")
        ));
    }
    if snapshot.redacted_arguments > 0 {
        report.bound(format!(
            "{} replaced in a command column",
            plural(snapshot.redacted_arguments, "credential-shaped argument")
        ));
    }

    if shown < total {
        report.next(
            format!("{TOOL} repos --limit {total}"),
            "the repositories below the limit",
        );
    } else if let Some(first) = rows.first() {
        report.next(
            format!("{TOOL} tree {}", first.label),
            "what is still running under it",
        );
    }

    Ok(Outcome::from_report(report))
}

pub fn tree(target: &str, opts: &Opts) -> Result<Outcome, Fail> {
    let snapshot = sample::snapshot()?;
    let home = home();
    let repo = resolve_repo(&snapshot, target, &home)?;
    let label = label(&repo, &home);

    let mut in_repo: Vec<&Proc> = snapshot
        .procs
        .iter()
        .filter(|proc| proc.repo.as_deref() == Some(repo.as_path()))
        .collect();
    if in_repo.is_empty() {
        return Err(Fail::environment(format!("{label} has no live processes")));
    }

    // A process is a root when its parent is not in this repository: either the parent is gone,
    // or it belongs to something else. The `!` marks the first kind.
    let present: Vec<i32> = in_repo.iter().map(|proc| proc.pid).collect();
    let orphans = in_repo.iter().filter(|proc| proc.ppid == 1).count();
    let mut roots: Vec<&Proc> = in_repo
        .iter()
        .copied()
        .filter(|proc| !present.contains(&proc.ppid))
        .collect();
    roots.sort_by_key(|proc| proc.pid);
    in_repo.sort_by_key(|proc| proc.pid);

    let mut children: HashMap<i32, Vec<&Proc>> = HashMap::new();
    for proc in &in_repo {
        children.entry(proc.ppid).or_default().push(proc);
    }

    let total = in_repo.len();
    let mut report = Report::new();
    report.header(
        crate::TOOL,
        "tree",
        &home,
        &format!("{label} · {}", plural_of(total, "process", "processes")),
    );
    report.note(format!(
        "# {:>7}  {:>7}  {:>5}  {:>8}  command",
        "pid", "cpu", "avg%", "rss"
    ));

    let mut printed = 0usize;
    for root in roots {
        print_branch(
            root,
            0,
            &children,
            &snapshot,
            opts,
            &mut printed,
            &mut report,
        );
        if printed >= opts.limit {
            break;
        }
    }

    if printed < total {
        report.bound(format!(
            "{printed} of {total} processes; the rest are below the limit"
        ));
    }
    if let Some(asked) = opts.limit_clamped_from {
        report.bound(format!("--limit {asked} clamped to {}", opts.limit));
    }
    if orphans > 0 {
        report.bound(format!(
            "{orphans} with a live parent nowhere · `!` marks one"
        ));
    }
    if snapshot.redacted_arguments > 0 {
        report.bound(format!(
            "{} replaced in the command column",
            plural(snapshot.redacted_arguments, "credential-shaped argument")
        ));
    }

    if printed < total {
        report.next(
            format!("{TOOL} tree {label} --limit {total}"),
            "the processes below the limit",
        );
    } else if let Some(hottest) = in_repo
        .iter()
        .filter(|proc| proc.cpu_ns > 0)
        .max_by_key(|proc| proc.cpu_ns)
    {
        report.next(
            format!("{TOOL} pid {}", hottest.pid),
            "which thread inside it is hot",
        );
    }

    Ok(Outcome::from_report(report))
}

pub fn pid(raw: &str, opts: &Opts) -> Result<Outcome, Fail> {
    let pid: i32 = raw
        .parse()
        .map_err(|_| Fail::usage(format!("`{raw}` is not a process id")))?;
    if pid <= 0 {
        return Err(Fail::usage("process ids are positive"));
    }

    let snapshot = sample::snapshot()?;
    let proc = snapshot
        .procs
        .iter()
        .find(|proc| proc.pid == pid)
        .ok_or_else(|| Fail::environment(format!("no process {pid} in the table")))?;
    let threads = sample::threads(pid)?;
    let total_ticks: u64 = threads.iter().map(|thread| thread.ticks).sum();

    let mut rows: Vec<&sample::Thread> = threads.iter().collect();
    rows.sort_by(|left, right| {
        right
            .ticks
            .cmp(&left.ticks)
            .then_with(|| left.tid.cmp(&right.tid))
    });

    let label = proc
        .repo
        .as_deref()
        .map(|repo| label(repo, &home()))
        .unwrap_or_else(|| "no repository".to_string());

    let total = rows.len();
    let shown = total.min(opts.limit);

    let mut report = Report::new();
    report.header(
        crate::TOOL,
        "pid",
        &home(),
        &format!(
            "{pid} · {} · {label} · {} · {} cpu",
            clip(proc_name(proc), 40),
            plural(total, "thread"),
            duration(proc.cpu_seconds()),
        ),
    );
    report.note(format!(
        "# {:>6}  {:>8}  {:>9}  thread",
        "share", "cpu", "tid"
    ));

    for thread in rows.iter().take(shown) {
        let share = if total_ticks == 0 {
            None
        } else {
            Some(thread.ticks as f64 / total_ticks as f64)
        };
        let share_text = match share {
            Some(share) => format!("{:.1}%", share * 100.0),
            None => "-".to_string(),
        };
        // The share is exact and the seconds beside it are that share applied to the process's
        // own rusage total, so the column can never disagree with the line above it.
        let seconds = share
            .map(|share| duration(proc.cpu_seconds() * share))
            .unwrap_or_else(|| "-".to_string());
        report.content(format!(
            "{:>6}  {:>8}  {:>9}  {}",
            share_text,
            seconds,
            thread.tid,
            if thread.name.is_empty() {
                "-"
            } else {
                thread.name.as_str()
            }
        ));
    }

    if shown < total {
        report.bound(format!(
            "{shown} of {total} threads; the rest are below the limit"
        ));
    }
    if let Some(asked) = opts.limit_clamped_from {
        report.bound(format!("--limit {asked} clamped to {}", opts.limit));
    }
    if total_ticks == 0 {
        report.bound("the platform reported no CPU time for any thread, so there is no share");
    }

    if proc.repo.is_some() && shown == total {
        report.next(format!("{TOOL} tree {label}"), "this process in its tree");
    }

    Ok(Outcome::from_report(report))
}

fn print_branch(
    proc: &Proc,
    depth: usize,
    children: &HashMap<i32, Vec<&Proc>>,
    snapshot: &Snapshot,
    opts: &Opts,
    printed: &mut usize,
    report: &mut Report,
) {
    if *printed >= opts.limit {
        return;
    }
    let average = proc
        .average_percent(snapshot.taken_at)
        .map(|percent| format!("{percent:.1}"))
        .unwrap_or_else(|| "-".to_string());
    let mark = if proc.ppid == 1 { '!' } else { ' ' };
    report.content(format!(
        "{:indent$}{mark}{:>7}  {:>7}  {:>5}  {:>8}  {}",
        "",
        proc.pid,
        duration(proc.cpu_seconds()),
        average,
        bytes(proc.footprint),
        clip(&proc.command, 96),
        indent = depth * 2,
    ));
    *printed += 1;

    if let Some(kids) = children.get(&proc.pid) {
        for child in kids {
            print_branch(child, depth + 1, children, snapshot, opts, printed, report);
            if *printed >= opts.limit {
                return;
            }
        }
    }
}

/// Resolve a repository a caller named, by label, by directory name, or by an unambiguous
/// substring — never by guessing which of two matches was meant.
fn resolve_repo(snapshot: &Snapshot, target: &str, home: &str) -> Result<PathBuf, Fail> {
    let mut repos: Vec<&Path> = snapshot
        .procs
        .iter()
        .filter_map(|proc| proc.repo.as_deref())
        .collect();
    repos.sort();
    repos.dedup();

    let labelled: Vec<(&Path, String)> = repos
        .into_iter()
        .map(|repo| (repo, label(repo, home)))
        .collect();

    let exact: Vec<&(&Path, String)> = labelled
        .iter()
        .filter(|(repo, label)| {
            label == target || repo.file_name().is_some_and(|name| name == target)
        })
        .collect();
    let matches: Vec<&(&Path, String)> = if exact.is_empty() {
        labelled
            .iter()
            .filter(|(_, label)| label.contains(target))
            .collect()
    } else {
        exact
    };

    match matches.as_slice() {
        [] => Err(Fail::usage(format!(
            "no repository matches `{target}` · {}",
            if labelled.is_empty() {
                "this table has no repository in it".to_string()
            } else {
                format!("known: {}", names(&labelled))
            }
        ))),
        [only] => Ok(only.0.to_path_buf()),
        several => Err(Fail::usage(format!(
            "`{target}` matches {} · be more specific",
            names(
                &several
                    .iter()
                    .map(|(repo, label)| (*repo, label.clone()))
                    .collect::<Vec<_>>()
            )
        ))),
    }
}

fn names(labelled: &[(&Path, String)]) -> String {
    let mut out: Vec<&str> = labelled.iter().map(|(_, label)| label.as_str()).collect();
    out.sort();
    out.join(", ")
}

/// The repository as a reader types it: relative to `$HOME`, forward slashes.
fn label(repo: &Path, home: &str) -> String {
    repo.strip_prefix(home)
        .unwrap_or(repo)
        .to_string_lossy()
        .replace('\\', "/")
}

fn home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".to_string())
}

fn proc_name(proc: &Proc) -> &str {
    if proc.name.is_empty() {
        &proc.command
    } else {
        &proc.name
    }
}

/// `45s`, `12m30s`, `3h09m`, `2d13h`. A compact duration an eye can rank.
fn duration(seconds: f64) -> String {
    let total = seconds.round().max(0.0) as u64;
    match total {
        0..=59 => format!("{total}s"),
        60..=3_599 => format!("{}m{:02}s", total / 60, total % 60),
        3_600..=86_399 => format!("{}h{:02}m", total / 3_600, (total % 3_600) / 60),
        _ => format!("{}d{:02}h", total / 86_400, (total % 86_400) / 3_600),
    }
}

/// `814`, `8.1k`, `1.2M` — a count wide enough to rank and narrow enough to sit in a column.
fn count(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{:.1}k", n as f64 / 1e3),
        1_000_000..=999_999_999 => format!("{:.1}M", n as f64 / 1e6),
        _ => format!("{:.1}G", n as f64 / 1e9),
    }
}

fn bytes(n: u64) -> String {
    const KIB: f64 = 1024.0;
    let value = n as f64;
    if value >= KIB * KIB * KIB {
        format!("{:.1}G", value / (KIB * KIB * KIB))
    } else if value >= KIB * KIB {
        format!("{:.0}M", value / (KIB * KIB))
    } else if value >= KIB {
        format!("{:.0}K", value / KIB)
    } else {
        format!("{n}")
    }
}

/// Cut a cell at a column width, saying so with `...` rather than ending mid-word invisibly.
fn clip(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let kept: String = text.chars().take(width.saturating_sub(3)).collect();
    format!("{kept}...")
}

const TOOL: &str = crate::TOOL;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_as_an_eye_expects() {
        assert_eq!(duration(45.0), "45s");
        assert_eq!(duration(750.0), "12m30s");
        assert_eq!(duration(11_400.0), "3h10m");
        assert_eq!(duration(217_800.0), "2d12h");
        assert_eq!(duration(-1.0), "0s");
    }

    #[test]
    fn counts_stay_inside_their_column() {
        assert_eq!(count(814), "814");
        assert_eq!(count(8_142), "8.1k");
        assert_eq!(count(8_142_000), "8.1M");
    }

    #[test]
    fn a_cell_is_cut_at_its_width_and_says_so() {
        assert_eq!(clip("short", 10), "short");
        assert_eq!(clip("a longer line than fits", 10), "a longe...");
    }

    #[test]
    fn a_label_is_relative_to_home() {
        assert_eq!(
            label(
                Path::new("/Users/damo/code/defrag/cnft.dev-workers"),
                "/Users/damo"
            ),
            "code/defrag/cnft.dev-workers"
        );
        assert_eq!(label(Path::new("/opt/thing"), "/Users/damo"), "/opt/thing");
    }
}
