//! The verbs.
//!
//! Both answer from the same read: list the headers, resolve a title fragment, then either stop
//! there (`find`) or load one blob and build its digest (`digest`). The store is opened read-only
//! and the connection is dropped before the process exits; nothing here writes.

use std::path::PathBuf;

use at_core::contract::{again, shell_word, Exit, Fail, Outcome, Report};
use zed_thread_tools::db;
use zed_thread_tools::digest::{self, DigestOptions, MAX_BUDGET_BYTES};

use crate::TOOL;

/// Rows `find` prints when the caller does not name a limit.
pub const DEFAULT_ROWS: usize = 20;
/// The ceiling `--limit` cannot raise.
pub const MAX_ROWS: usize = 200;

/// What a verb needs that is not its own argument.
#[derive(Debug, Clone)]
pub struct Opts {
    /// The store the caller named, if any. `None` means the platform default.
    pub db: Option<PathBuf>,
    /// Agent-activity bytes the digest may carry.
    pub budget_bytes: usize,
    /// Set when `--budget-kb` asked for more than the ceiling, so the clamp is announced.
    pub budget_clamped_from: Option<usize>,
    /// Rows `find` may print.
    pub limit: usize,
    /// Set when `--limit` asked for more than the ceiling.
    pub limit_clamped_from: Option<usize>,
}

pub fn find(query: &str, opts: &Opts) -> Result<Outcome, Fail> {
    let path = store(opts)?;
    let headers = db::list_headers_at(&path).map_err(environment)?;
    let matches = digest::candidates(&headers, query);

    let mut report = Report::new();
    report.header(TOOL, "find", &path.display().to_string(), query);

    if matches.is_empty() {
        report.bound(format!(
            "no thread title contains {query:?} · {} threads in the store",
            headers.len()
        ));
        return Ok(Outcome {
            report,
            exit: Exit::Nothing,
        });
    }

    let shown = matches.len().min(opts.limit);
    for header in matches.iter().take(shown) {
        report.content(format!(
            "{} · {} · {} · {}",
            header.id,
            header.updated_at,
            size(header.compressed_bytes as usize),
            header.summary
        ));
    }

    if let Some(asked) = opts.limit_clamped_from {
        report.bound(format!("--limit clamped to {MAX_ROWS} (asked for {asked})"));
    }

    if shown < matches.len() {
        report.bound(format!("{shown} of {} threads listed", matches.len()));
        if opts.limit < MAX_ROWS {
            let next = (opts.limit.max(1) * 2).min(MAX_ROWS);
            report.next(
                again(
                    TOOL,
                    "find",
                    &[query.to_owned()],
                    &flags(opts, Some(("--limit", next.to_string()))),
                    None,
                ),
                "widen the listing",
            );
        }
    }

    Ok(Outcome::from_report(report))
}

pub fn digest(query: &str, opts: &Opts) -> Result<Outcome, Fail> {
    let path = store(opts)?;
    let headers = db::list_headers_at(&path).map_err(environment)?;
    let matches = digest::candidates(&headers, query);

    let mut report = Report::new();
    report.header(TOOL, "digest", &path.display().to_string(), query);

    match matches.as_slice() {
        [] => {
            report.bound(format!(
                "no thread title contains {query:?} · {} threads in the store",
                headers.len()
            ));
            Ok(Outcome {
                report,
                exit: Exit::Nothing,
            })
        }
        [header] => {
            report.note(format!("# title: {}", header.summary));
            let record = db::load_record_at(&path, &header.id).map_err(environment)?;
            let built = digest::digest(
                &record,
                &DigestOptions {
                    budget_bytes: opts.budget_bytes,
                },
            )
            .map_err(environment)?;

            for line in built.markdown.lines() {
                report.content(line.to_owned());
            }

            let stats = &built.stats;
            report.bound(format!(
                "{} messages · {} user · {} tool calls · {} thinking omitted",
                stats.messages, stats.user_messages, stats.tool_calls, stats.thinking
            ));
            if stats.images > 0 {
                report.bound(format!(
                    "{} images omitted ({} of base64) — the reason a thread this size cannot be sent",
                    stats.images,
                    size(stats.image_bytes)
                ));
            }
            report.bound(format!(
                "budget {} kB · activity {} of {} lines shown",
                opts.budget_bytes / 1024,
                stats.activity_shown,
                stats.activity_lines
            ));
            if stats.omitted_activity > 0 {
                report.bound(format!(
                    "{} activity lines omitted from the middle; the head and the tail were kept",
                    stats.omitted_activity
                ));
            }
            if stats.unreadable_messages > 0 {
                report.bound(format!(
                    "{} messages could not be read and were skipped",
                    stats.unreadable_messages
                ));
            }
            if let Some(asked) = opts.budget_clamped_from {
                report.bound(format!(
                    "--budget-kb clamped to {} (asked for {asked})",
                    MAX_BUDGET_BYTES / 1024
                ));
            }

            if stats.omitted_activity > 0 && opts.budget_bytes < MAX_BUDGET_BYTES {
                let next = ((opts.budget_bytes / 1024).max(1) * 2).min(MAX_BUDGET_BYTES / 1024);
                report.next(
                    again(
                        TOOL,
                        "digest",
                        &[query.to_owned()],
                        &flags(opts, Some(("--budget-kb", next.to_string()))),
                        None,
                    ),
                    "widen the activity window",
                );
            }

            Ok(Outcome::from_report(report))
        }
        many => {
            report.bound(format!(
                "{} threads match {query:?}; pass the id or a longer fragment",
                many.len()
            ));
            for header in many.iter().take(digest::CANDIDATE_LIMIT) {
                report.content(format!("{} · {}", header.id, header.summary));
            }
            if many.len() > digest::CANDIDATE_LIMIT {
                report.bound(format!(
                    "{} further candidates not shown",
                    many.len() - digest::CANDIDATE_LIMIT
                ));
            }
            Ok(Outcome {
                report,
                exit: Exit::Usage,
            })
        }
    }
}

fn store(opts: &Opts) -> Result<PathBuf, Fail> {
    opts.db.clone().or_else(db::default_db_path).ok_or_else(|| {
        Fail::environment("could not determine Zed's thread store; pass --db <path>")
    })
}

fn environment(error: zed_thread_tools::Error) -> Fail {
    Fail::environment(error.to_string())
}

/// The flags that carry the caller's store and one changed value, so an exit answers in the same
/// store it was asked about.
fn flags(opts: &Opts, extra: Option<(&str, String)>) -> Vec<String> {
    let mut flags = Vec::new();
    if let Some((name, value)) = extra {
        flags.push(format!("{name} {value}"));
    }
    if let Some(path) = &opts.db {
        flags.push(format!("--db {}", shell_word(&path.display().to_string())));
    }
    flags
}

fn size(bytes: usize) -> String {
    const KB: f64 = 1_000.0;
    const MB: f64 = 1_000_000.0;

    let value = bytes as f64;
    if value >= MB {
        format!("{:.1} MB", value / MB)
    } else if value >= KB {
        format!("{:.1} kB", value / KB)
    } else {
        format!("{bytes} B")
    }
}
