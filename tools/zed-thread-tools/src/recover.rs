//! Compaction-based recovery for a thread that has outgrown its context.
//!
//! The operation is deliberately narrow: append one
//! `Compaction(Summary)` message, which is the same message Zed's own
//! `/compact` writes. Zed's `extend_request_history_until` then sends the
//! summary (plus recent user messages) to the model instead of the whole
//! transcript. No existing message is removed or rewritten, so the full
//! history remains in the thread.
//!
//! The tool refuses to write when Zed is running, makes a `VACUUM INTO`
//! backup first, and requires `--write` to leave dry-run mode.

use std::collections::BTreeMap;
use std::fmt;
use std::fmt::Write as _;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection};
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use crate::db::{self, ThreadRecord};
use crate::Error;

#[derive(Debug, Clone)]
pub struct RecoverOptions {
    pub thread_id: String,
    pub summary: String,
    pub title: Option<String>,
    pub fork: bool,
    pub dry_run: bool,
    pub backup: Option<PathBuf>,
    pub allow_running: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RecoveryMode {
    InPlace,
    Fork,
}

impl fmt::Display for RecoveryMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InPlace => formatter.write_str("in place"),
            Self::Fork => formatter.write_str("fork"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RecoveryPlan {
    pub thread_id: String,
    pub mode: RecoveryMode,
    pub insertion_index: usize,
    pub messages_before: usize,
    pub messages_after: usize,
    pub decoded_bytes_before: usize,
    pub decoded_bytes_after: usize,
    pub summary_bytes: usize,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct PreparedRecovery {
    pub plan: RecoveryPlan,
    pub new_data: Vec<u8>,
    pub new_title: String,
    pub new_updated_at: String,
}

#[derive(Debug, Clone)]
pub struct WriteOutcome {
    pub backup: PathBuf,
    pub thread_id: String,
    pub created: bool,
}

/// Build the new thread document in memory. This validates the format version
/// and the presence of the fields the operation needs, and refuses anything it
/// does not understand. It does not touch the database.
pub fn prepare_recovery(
    record: &ThreadRecord,
    summary: &str,
    title: Option<&str>,
    fork: bool,
) -> Result<PreparedRecovery, Error> {
    let summary = summary.trim();
    if summary.is_empty() {
        return Err(Error::EmptySummary);
    }

    let decoded = db::decode_thread(&record.header.data_type, &record.data)?;
    let text = std::str::from_utf8(&decoded)
        .map_err(|error| Error::InvalidThread(format!("thread JSON is not UTF-8: {error}")))?;
    let mut fields: BTreeMap<String, Box<RawValue>> = serde_json::from_str(text)?;

    let version = json_string(&fields, "version")?
        .ok_or_else(|| Error::InvalidThread("missing version field".to_owned()))?;
    if version != db::DB_THREAD_VERSION {
        return Err(Error::UnsupportedVersion(version));
    }

    let messages_raw = fields
        .get("messages")
        .ok_or_else(|| Error::InvalidThread("missing messages array".to_owned()))?;
    let mut messages: Vec<Box<RawValue>> = serde_json::from_str(messages_raw.get())?;
    let messages_before = messages.len();

    // Mirror Zed's automatic compaction: a trailing unanswered user message
    // (or a Resume marker) stays after the summary, so the next request still
    // answers it. Otherwise the summary goes at the end.
    let insertion_index = match messages.last() {
        Some(message) => match serde_json::from_str::<MessageKind>(message.get())? {
            MessageKind::User(_) | MessageKind::Resume => messages.len().saturating_sub(1),
            MessageKind::Agent(_) | MessageKind::Compaction(_) => messages.len(),
        },
        None => 0,
    };

    let compaction = RawValue::from_string(serde_json::to_string(
        &CompactionMessage::Compaction(CompactionInfo::Summary(summary)),
    )?)?;
    messages.insert(insertion_index, compaction);

    let messages_json = serde_json::to_string(&messages)?;
    fields.insert("messages".to_owned(), RawValue::from_string(messages_json)?);

    let updated_at = Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true);
    fields.insert(
        "updated_at".to_owned(),
        RawValue::from_string(serde_json::to_string(&updated_at)?)?,
    );

    let original_title = json_string(&fields, "title")?
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| record.header.summary.clone());
    let new_title = match title.map(str::trim).filter(|title| !title.is_empty()) {
        Some(title) => title.to_owned(),
        None if fork => format!("{original_title} (recovered)"),
        None => original_title,
    };
    fields.insert(
        "title".to_owned(),
        RawValue::from_string(serde_json::to_string(&new_title)?)?,
    );

    // A fork has no prior summary panel; point it at the recovery summary.
    // An in-place recovery leaves the old detailed summary alone.
    if fork {
        fields.insert(
            "detailed_summary".to_owned(),
            RawValue::from_string(serde_json::to_string(summary)?)?,
        );
    }

    let new_json = serde_json::to_vec(&fields)?;
    let new_data = zstd::stream::encode_all(new_json.as_slice(), 3)?;

    Ok(PreparedRecovery {
        plan: RecoveryPlan {
            thread_id: record.header.id.clone(),
            mode: if fork {
                RecoveryMode::Fork
            } else {
                RecoveryMode::InPlace
            },
            insertion_index,
            messages_before,
            messages_after: messages_before + 1,
            decoded_bytes_before: decoded.len(),
            decoded_bytes_after: new_json.len(),
            summary_bytes: summary.len(),
            updated_at: updated_at.clone(),
        },
        new_data,
        new_title,
        new_updated_at: updated_at,
    })
}

/// Write the prepared document. In place updates the existing row; fork inserts
/// a new top-level row and leaves the original untouched. A `VACUUM INTO`
/// backup is made before either.
pub fn write_recovery(
    db_path: &Path,
    record: &ThreadRecord,
    prepared: &PreparedRecovery,
    options: &RecoverOptions,
) -> Result<WriteOutcome, Error> {
    if options.dry_run {
        return Err(Error::InvalidThread(
            "write_recovery called in dry-run mode".to_owned(),
        ));
    }
    if record.header.id != options.thread_id {
        return Err(Error::InvalidThread(format!(
            "prepared thread {:?} does not match requested thread {:?}",
            record.header.id, options.thread_id
        )));
    }
    if !options.allow_running && zed_is_running() {
        return Err(Error::ZedRunning);
    }

    let backup = options
        .backup
        .clone()
        .unwrap_or_else(|| default_backup_path(db_path));
    if backup.exists() {
        return Err(Error::BackupExists(backup));
    }

    let mut connection = db::open_read_write(db_path)?;
    backup_database(&connection, &backup)?;

    let transaction = connection.transaction()?;
    let thread_id = if options.fork {
        let new_id = new_session_id();
        transaction.execute(
            "INSERT INTO threads \
             (id, parent_id, worktree_branch, folder_paths, folder_paths_order, \
              summary, updated_at, data_type, data, created_at) \
             VALUES (?1, NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                new_id,
                record.header.worktree_branch,
                record.header.folder_paths,
                record.header.folder_paths_order,
                prepared.new_title,
                prepared.new_updated_at,
                db::DATA_TYPE_ZSTD,
                prepared.new_data,
                prepared.new_updated_at,
            ],
        )?;
        new_id
    } else {
        let updated = transaction.execute(
            "UPDATE threads SET summary = ?1, updated_at = ?2, data_type = ?3, data = ?4 \
             WHERE id = ?5",
            params![
                prepared.new_title,
                prepared.new_updated_at,
                db::DATA_TYPE_ZSTD,
                prepared.new_data,
                record.header.id,
            ],
        )?;
        if updated != 1 {
            return Err(Error::ThreadNotFound(record.header.id.clone()));
        }
        record.header.id.clone()
    };
    transaction.commit()?;

    Ok(WriteOutcome {
        backup,
        thread_id,
        created: options.fork,
    })
}

pub fn print_plan(record: &ThreadRecord, prepared: &PreparedRecovery, dry_run: bool) {
    println!("# zed-thread-recover · {}", record.header.id);
    println!("# title: {}", prepared.new_title);
    println!(
        "# mode: {} · insert at message {} of {}",
        prepared.plan.mode, prepared.plan.insertion_index, prepared.plan.messages_before
    );
    println!(
        "# messages: {} -> {} · decoded: {} -> {} · summary: {}",
        prepared.plan.messages_before,
        prepared.plan.messages_after,
        format_bytes(prepared.plan.decoded_bytes_before),
        format_bytes(prepared.plan.decoded_bytes_after),
        format_bytes(prepared.plan.summary_bytes),
    );

    if dry_run {
        println!("# dry run: no database was changed. Re-run with --write to apply.");
    }
}

pub fn print_outcome(outcome: &WriteOutcome) {
    if outcome.created {
        println!("# created recovered thread {}", outcome.thread_id);
    } else {
        println!("# updated thread {} in place", outcome.thread_id);
    }
    println!("# backup: {}", outcome.backup.display());
    println!("# reopen Zed and send \"continue\" to resume from the compaction summary");
}

fn json_string(
    fields: &BTreeMap<String, Box<RawValue>>,
    key: &str,
) -> Result<Option<String>, Error> {
    let Some(raw) = fields.get(key) else {
        return Ok(None);
    };
    if raw.get() == "null" {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(raw.get())?))
}

pub(crate) fn backup_database(connection: &Connection, path: &Path) -> Result<(), Error> {
    let path = path
        .to_str()
        .ok_or_else(|| Error::InvalidThread("backup path is not UTF-8".to_owned()))?;
    let escaped = path.replace('\'', "''");
    connection.execute_batch(&format!("VACUUM INTO '{escaped}'"))?;
    Ok(())
}

fn default_backup_path(db_path: &Path) -> PathBuf {
    backup_path(db_path, "zed-thread-recover")
}

pub(crate) fn backup_path(db_path: &Path, label: &str) -> PathBuf {
    let stamp = Utc::now().format("%Y%m%dT%H%M%S%.6fZ");
    let file_name = db_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("database");
    db_path.with_file_name(format!("{file_name}.{label}-{stamp}.bak"))
}

pub(crate) fn zed_is_running() -> bool {
    zed_is_running_with(process_exists)
}

fn zed_is_running_with(probe: impl Fn(&str) -> bool) -> bool {
    // The macOS bundle has been observed with both `Zed` and `zed` as the
    // executable name, so the platform alone is not enough to pick one.
    ["Zed", "zed"].into_iter().any(probe)
}

fn process_exists(name: &str) -> bool {
    std::process::Command::new("pgrep")
        .args(["-x", name])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn new_session_id() -> String {
    if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
        let mut bytes = [0u8; 16];
        if file.read_exact(&mut bytes).is_ok() {
            bytes[6] = (bytes[6] & 0x0f) | 0x40;
            bytes[8] = (bytes[8] & 0x3f) | 0x80;
            return format_uuid(bytes);
        }
    }

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("recovered-{nanos:x}")
}

fn format_uuid(bytes: [u8; 16]) -> String {
    let mut output = String::with_capacity(36);
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            output.push('-');
        }
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

fn format_bytes(bytes: usize) -> String {
    const KB: f64 = 1_000.0;
    const MB: f64 = 1_000_000.0;
    const GB: f64 = 1_000_000_000.0;

    let value = bytes as f64;
    if value >= GB {
        format!("{:.1} GB", value / GB)
    } else if value >= MB {
        format!("{:.1} MB", value / MB)
    } else if value >= KB {
        format!("{:.1} kB", value / KB)
    } else {
        format!("{bytes} B")
    }
}

/// Only the shape matters here: the messages themselves stay as raw JSON.
#[derive(Deserialize)]
enum MessageKind {
    User(IgnoredAny),
    Agent(IgnoredAny),
    Resume,
    Compaction(IgnoredAny),
}

/// Serde writes this as `{"Compaction":{"Summary":"..."}}`, the externally
/// tagged form of Zed's `Message::Compaction(CompactionInfo::Summary)`.
#[derive(Serialize)]
enum CompactionMessage<'a> {
    Compaction(CompactionInfo<'a>),
}

#[derive(Serialize)]
enum CompactionInfo<'a> {
    Summary(&'a str),
}

#[cfg(test)]
mod tests {
    use super::zed_is_running_with;

    #[test]
    fn recognises_the_lowercase_macos_process_name() {
        assert!(zed_is_running_with(|name| name == "zed"));
    }

    #[test]
    fn recognises_the_older_uppercase_process_name() {
        assert!(zed_is_running_with(|name| name == "Zed"));
    }

    #[test]
    fn is_false_when_neither_process_exists() {
        assert!(!zed_is_running_with(|_| false));
    }
}
