//! `zed-thread-audit` — read-only sizing and metadata for Zed agent threads.

use std::path::PathBuf;

use chrono::NaiveDate;
use clap::Parser;
use zed_thread_tools::audit::{self, AuditOptions};
use zed_thread_tools::db;

#[derive(Parser)]
#[command(
    name = "zed-thread-audit",
    version,
    about = "Read-only audit of Zed agent thread transcripts",
    long_about = "Read-only audit of Zed agent thread transcripts.\n\n\
                  Reports thread size, message and tool-call counts, tool-output bytes, \
                  compaction state, model and project paths. Use --explain to attribute \
                  a thread's largest outputs to a tool and message. It never prints message \
                  content and never opens the database read-write."
)]
struct Cli {
    /// Path to Zed's threads.db
    #[arg(long, value_name = "PATH")]
    db: Option<PathBuf>,

    /// Number of threads to show, largest decoded first (0 = all)
    #[arg(long, default_value_t = 20)]
    top: usize,

    /// Only threads last updated before this date
    #[arg(long, value_name = "YYYY-MM-DD")]
    older_than: Option<String>,

    /// Only threads whose project paths contain this text
    #[arg(long, value_name = "TEXT")]
    project: Option<String>,

    /// Emit the typed JSON report instead of the table
    #[arg(long)]
    json: bool,

    /// Explain one thread's largest tool outputs: size, message, tool and request argument
    #[arg(long, value_name = "THREAD_ID_OR_TITLE")]
    explain: Option<String>,

    /// Number of outputs shown by --explain (0 = all)
    #[arg(long, default_value_t = 10)]
    limit: usize,
}

fn main() {
    let cli = Cli::parse();
    if let Err(error) = run(cli) {
        eprintln!("zed-thread-audit: {error}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let Some(db_path) = cli.db.or_else(db::default_db_path) else {
        eprintln!("zed-thread-audit: could not determine the Zed thread database path; pass --db");
        std::process::exit(1);
    };

    let older_than = match cli.older_than.as_deref() {
        Some(value) => Some(
            NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|error| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("invalid --older-than date {value:?}: {error}"),
                )
            })?,
        ),
        None => None,
    };

    let options = AuditOptions {
        top: cli.top,
        older_than,
        project: cli.project,
    };

    if let Some(thread) = cli.explain.as_deref() {
        let explanation = audit::explain(&db_path, thread, cli.limit)?;
        if cli.json {
            serde_json::to_writer_pretty(std::io::stdout(), &explanation)?;
            println!();
        } else {
            audit::print_explanation(&explanation);
        }
        return Ok(());
    }

    let report = audit::audit(&db_path, &options)?;

    if cli.json {
        serde_json::to_writer_pretty(std::io::stdout(), &report)?;
        println!();
    } else {
        audit::print_report(&report);
    }

    Ok(())
}
