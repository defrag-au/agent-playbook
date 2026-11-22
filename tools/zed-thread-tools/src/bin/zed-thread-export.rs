//! `zed-thread-export` — a decoded, budgeted snapshot of Zed agent threads.

use std::path::PathBuf;

use clap::Parser;
use zed_thread_tools::audit::format_bytes;
use zed_thread_tools::db;
use zed_thread_tools::export::{self, ExportOptions, MB};

#[derive(Parser)]
#[command(
    name = "zed-thread-export",
    version,
    about = "Write a decoded snapshot of Zed agent threads for an external reader",
    long_about = "Write a decoded snapshot of Zed agent threads for an external reader.\n\n\
                  Each exported thread is one JSON file, decoded from the blob Zed stores in \
                  `threads.db`, beside a typed `manifest.json` and a short README. Whole thread \
                  trees are selected newest first, and a tree is either included whole or left \
                  out when it would take the total past the size budget. The store is opened \
                  read-only. The output is unredacted: it contains prompts, tool output and local \
                  paths."
)]
struct Cli {
    /// Path to Zed's threads.db
    #[arg(long, value_name = "PATH")]
    db: Option<PathBuf>,

    /// Directory to write; it must not already exist
    #[arg(long, value_name = "DIR")]
    out: PathBuf,

    /// Decoded thread bytes to export, in decimal megabytes (0 = every thread)
    #[arg(long, default_value_t = 50, value_name = "MB")]
    max_mb: u64,
}

fn main() {
    let cli = Cli::parse();
    if let Err(error) = run(cli) {
        eprintln!("zed-thread-export: {error}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let Some(db_path) = cli.db.or_else(db::default_db_path) else {
        eprintln!("zed-thread-export: could not determine the Zed thread database path; pass --db");
        std::process::exit(1);
    };

    let options = ExportOptions {
        max_bytes: cli.max_mb.saturating_mul(MB),
    };
    let outcome = export::export(&db_path, &cli.out, &options)?;

    println!(
        "zed-thread-export: {} threads in {} trees, {}, written to {}",
        outcome.threads,
        outcome.trees,
        format_bytes(outcome.bytes),
        outcome.output_dir.display()
    );
    if outcome.skipped_over_budget > 0 {
        println!(
            "zed-thread-export: skipped {} trees over the budget",
            outcome.skipped_over_budget
        );
    }
    if outcome.skipped_undecodable > 0 {
        println!(
            "zed-thread-export: skipped {} undecodable trees",
            outcome.skipped_undecodable
        );
    }

    Ok(())
}
