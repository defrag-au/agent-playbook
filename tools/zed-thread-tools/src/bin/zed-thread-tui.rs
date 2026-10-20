//! `zed-thread-tui` — browse the audit and confirm a compaction recovery.

use std::io::IsTerminal;
use std::path::PathBuf;

use clap::Parser;
use zed_thread_tools::archive;
use zed_thread_tools::db;
use zed_thread_tools::metadata;
use zed_thread_tools::tui;

#[derive(Parser)]
#[command(
    name = "zed-thread-tui",
    version,
    about = "Browse Zed agent threads and apply a compaction recovery",
    long_about = "Browse Zed agent threads, archive or delete them, export and import \
                  bundles, and apply a compaction recovery.\n\n\
                  The list is the union of `sidebar_threads` and `threads.db`. Press `a` to \
                  archive, `e` to export, `d` to delete one thread, `i` to import a bundle, \
                  and `r` to prepare a recovery summary. Zed must be quit before any store is \
                  changed, and a backup is made first."
)]
struct Cli {
    /// Path to Zed's threads.db
    #[arg(long, value_name = "PATH")]
    db: Option<PathBuf>,

    /// Path to Zed's db/<channel>/db.sqlite
    #[arg(long, value_name = "PATH")]
    zed_db: Option<PathBuf>,

    /// Directory for recovery summary files
    #[arg(long, value_name = "DIR", default_value = ".tmp")]
    summary_dir: PathBuf,

    /// Directory for exported thread bundles
    #[arg(long, value_name = "DIR")]
    archive_dir: Option<PathBuf>,
}

fn main() {
    let cli = Cli::parse();

    if !std::io::stdout().is_terminal() {
        eprintln!("zed-thread-tui: requires an interactive terminal");
        std::process::exit(1);
    }

    let Some(threads_db) = cli.db.or_else(db::default_db_path) else {
        eprintln!("zed-thread-tui: could not determine the Zed thread database path; pass --db");
        std::process::exit(1);
    };
    let Some(zed_db) = cli.zed_db.or_else(metadata::default_zed_db_path) else {
        eprintln!("zed-thread-tui: could not find Zed's db.sqlite; pass --zed-db");
        std::process::exit(1);
    };
    let archive_dir = cli.archive_dir.unwrap_or_else(archive::default_archive_dir);

    if let Err(error) = tui::run(threads_db, zed_db, cli.summary_dir, archive_dir) {
        eprintln!("zed-thread-tui: {error}");
        std::process::exit(1);
    }
}
