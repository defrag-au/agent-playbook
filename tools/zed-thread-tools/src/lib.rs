//! Audit, and compaction-based recovery, for Zed agent thread transcripts.
//!
//! Zed stores each thread as one row in `threads.db`: a zstd-compressed JSON
//! document whose top-level shape is the `DbThread` struct from
//! `crates/agent/src/db.rs` plus a `version` field. The audit binary reads that
//! store; it never opens a database read-write. The recover binary is the only
//! writer, and it only appends a `Compaction(Summary)` message — the same
//! message variant Zed's own `/compact` writes. Existing messages are never
//! removed or rewritten.
//!
//! See `README.md` in this crate for the two commands and the safety
//! properties.

pub mod archive;
pub mod audit;
pub mod db;
pub mod metadata;
pub mod recover;
pub mod tui;
mod util;

use std::fmt;
use std::path::PathBuf;

pub use audit::{
    audit, explain, AuditOptions, AuditReport, ThreadAudit, ThreadExplanation, ToolOutputDetail,
};
pub use db::{default_db_path, ThreadHeader, ThreadRecord};
pub use recover::{
    prepare_recovery, write_recovery, PreparedRecovery, RecoverOptions, RecoveryMode, RecoveryPlan,
    WriteOutcome,
};

/// The crate version, so a caller can tell two installs apart.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Everything this crate can fail on. Kept as one enum so a binary can print a
/// single actionable line rather than a chain of wrapped display strings.
#[derive(Debug)]
pub enum Error {
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    Json(serde_json::Error),
    UnsupportedDataType(String),
    UnsupportedVersion(String),
    ThreadNotFound(String),
    DatabaseNotFound(PathBuf),
    InvalidThread(String),
    EmptySummary,
    ZedRunning,
    BackupExists(PathBuf),
    SidebarThreadNotFound(String),
    NothingToDo,
    BundleInvalid(String),
    BundleExists(PathBuf),
    ThreadConflict(String),
    UnsupportedBundleVersion(u32),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "SQLite: {error}"),
            Self::Io(error) => write!(formatter, "I/O: {error}"),
            Self::Json(error) => write!(formatter, "JSON: {error}"),
            Self::UnsupportedDataType(kind) => {
                write!(formatter, "unsupported thread data type {kind:?}")
            }
            Self::UnsupportedVersion(version) => write!(
                formatter,
                "unsupported thread format version {version:?}; this tool understands {}",
                db::DB_THREAD_VERSION
            ),
            Self::ThreadNotFound(id) => write!(formatter, "thread {id:?} was not found"),
            Self::DatabaseNotFound(path) => {
                write!(formatter, "thread database not found: {}", path.display())
            }
            Self::InvalidThread(message) => write!(formatter, "invalid thread: {message}"),
            Self::EmptySummary => write!(formatter, "the compaction summary is empty"),
            Self::ZedRunning => write!(
                formatter,
                "Zed appears to be running; quit it completely before writing to its thread store"
            ),
            Self::BackupExists(path) => write!(
                formatter,
                "refusing to overwrite existing backup {}",
                path.display()
            ),
            Self::SidebarThreadNotFound(id) => {
                write!(formatter, "sidebar thread {id:?} was not found")
            }
            Self::NothingToDo => write!(formatter, "nothing to do"),
            Self::BundleInvalid(message) => write!(formatter, "invalid bundle: {message}"),
            Self::BundleExists(path) => write!(
                formatter,
                "refusing to overwrite existing bundle {}",
                path.display()
            ),
            Self::ThreadConflict(id) => write!(
                formatter,
                "thread {id:?} already exists; refusing to import over it"
            ),
            Self::UnsupportedBundleVersion(version) => write!(
                formatter,
                "unsupported bundle format version {version}; this tool understands {}",
                archive::BUNDLE_FORMAT_VERSION
            ),
        }
    }
}

impl std::error::Error for Error {}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
