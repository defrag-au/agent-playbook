//! `zed-thread-recover` — append a compaction summary to a Zed thread.
//!
//! This is the only writer in this crate. It never removes or rewrites an
//! existing message, and it refuses to run while Zed is open unless
//! `--allow-running` is passed.

use std::path::PathBuf;

use clap::{ArgGroup, Parser};
use zed_thread_tools::db;
use zed_thread_tools::recover::{self, RecoverOptions};

#[derive(Parser)]
#[command(
    name = "zed-thread-recover",
    version,
    about = "Append a compaction summary so a Zed thread can continue past its context limit",
    long_about = "Append a compaction summary so a Zed thread can continue past its context limit.\n\n\
                  The operation adds one `Compaction(Summary)` message - the same message \
                  Zed's own /compact writes - and leaves every existing message in place. \
                  Zed's next request then sends the summary instead of the whole transcript.\n\n\
                  It is a dry run unless --write is passed. Quit Zed completely before \
                  writing: Zed keeps threads in memory and may overwrite the change."
)]
#[command(group(
    ArgGroup::new("summary_source")
        .required(true)
        .args(["summary_file", "summary"])
))]
struct Cli {
    /// Thread id from the audit report
    #[arg(long, value_name = "ID")]
    thread: String,

    /// File containing the compaction summary (for the model, not for display)
    #[arg(long, value_name = "PATH")]
    summary_file: Option<PathBuf>,

    /// Inline compaction summary
    #[arg(long, value_name = "TEXT")]
    summary: Option<String>,

    /// Replacement title (fork defaults to "<title> (recovered)")
    #[arg(long, value_name = "TITLE")]
    title: Option<String>,

    /// Insert a new top-level thread and leave the original untouched
    #[arg(long)]
    fork: bool,

    /// Apply the change; without this the command only prints the plan
    #[arg(long)]
    write: bool,

    /// Path to Zed's threads.db
    #[arg(long, value_name = "PATH")]
    db: Option<PathBuf>,

    /// Backup destination (default: next to the database, timestamped)
    #[arg(long, value_name = "PATH")]
    backup: Option<PathBuf>,

    /// Do not refuse when Zed is running (not recommended)
    #[arg(long)]
    allow_running: bool,
}

fn main() {
    let cli = Cli::parse();
    if let Err(error) = run(cli) {
        eprintln!("zed-thread-recover: {error}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let Some(db_path) = cli.db.or_else(db::default_db_path) else {
        eprintln!(
            "zed-thread-recover: could not determine the Zed thread database path; pass --db"
        );
        std::process::exit(1);
    };

    let summary = match (cli.summary_file, cli.summary) {
        (Some(path), None) => std::fs::read_to_string(&path).map_err(|error| {
            std::io::Error::new(error.kind(), format!("reading {}: {error}", path.display()))
        })?,
        (None, Some(summary)) => summary,
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "pass exactly one of --summary-file or --summary",
            )
            .into());
        }
    };

    let options = RecoverOptions {
        thread_id: cli.thread.clone(),
        summary,
        title: cli.title,
        fork: cli.fork,
        dry_run: !cli.write,
        backup: cli.backup,
        allow_running: cli.allow_running,
    };

    let connection = db::open_read_only(&db_path)?;
    let record = db::load_record(&connection, &cli.thread)?;
    drop(connection);

    let prepared = recover::prepare_recovery(
        &record,
        &options.summary,
        options.title.as_deref(),
        options.fork,
    )?;

    recover::print_plan(&record, &prepared, options.dry_run);
    if options.dry_run {
        return Ok(());
    }

    let outcome = recover::write_recovery(&db_path, &record, &prepared, &options)?;
    recover::print_outcome(&outcome);
    Ok(())
}
