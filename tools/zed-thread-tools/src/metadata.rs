//! Zed's `sidebar_threads` metadata store.
//!
//! This is the database the Agent panel uses for its thread list. It lives in
//! `db/<channel>/db.sqlite`, beside `threads/threads.db`, and is separate from
//! the transcript store. A thread can exist in either store without the other:
//! ACP threads have sidebar metadata but no `threads.db` row, and a row can
//! remain in `threads.db` after its sidebar entry is gone.

use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::Utc;
use rusqlite::{Connection, OpenFlags};

use crate::recover::{backup_database, zed_is_running};
use crate::util::hex_encode;
use crate::Error;

#[derive(Debug, Clone)]
pub struct SidebarThread {
    pub thread_id: Vec<u8>,
    pub session_id: Option<String>,
    pub agent_id: Option<String>,
    pub title: String,
    pub updated_at: String,
    pub created_at: Option<String>,
    pub folder_paths: Option<String>,
    pub folder_paths_order: Option<String>,
    pub archived: bool,
    pub main_worktree_paths: Option<String>,
    pub main_worktree_paths_order: Option<String>,
    pub remote_connection: Option<String>,
    pub interacted_at: Option<String>,
    pub title_override: Option<String>,
}

impl SidebarThread {
    pub fn thread_id_hex(&self) -> String {
        hex_encode(&self.thread_id)
    }

    pub fn display_title(&self) -> &str {
        self.title_override
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .unwrap_or(&self.title)
    }

    pub fn project_paths(&self) -> Vec<&str> {
        self.folder_paths
            .as_deref()
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .collect()
    }

    pub fn main_worktree_paths(&self) -> Vec<&str> {
        self.main_worktree_paths
            .as_deref()
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .collect()
    }
}

/// `~/Library/Application Support/Zed`, `~/.local/share/zed`, or `%APPDATA%\Zed`.
pub fn zed_data_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        return std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join("Library/Application Support/Zed"));
    }

    if cfg!(target_os = "windows") {
        return std::env::var_os("APPDATA").map(|appdata| PathBuf::from(appdata).join("Zed"));
    }

    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
        return Some(PathBuf::from(data_home).join("zed"));
    }

    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share/zed"))
}

/// Find the active channel database. `0-stable` wins if it exists; otherwise the
/// first `db/*/db.sqlite` in name order is used.
pub fn default_zed_db_path() -> Option<PathBuf> {
    let db_dir = zed_data_dir()?.join("db");
    let stable = db_dir.join("0-stable").join("db.sqlite");
    if stable.exists() {
        return Some(stable);
    }

    let mut candidates: Vec<PathBuf> = std::fs::read_dir(&db_dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path().join("db.sqlite"))
        .filter(|path| path.exists())
        .collect();
    candidates.sort();
    candidates
        .iter()
        .find(|path| path.to_string_lossy().contains("stable"))
        .cloned()
        .or_else(|| candidates.into_iter().next())
}

pub fn open_read_only(path: &Path) -> Result<Connection, Error> {
    if !path.exists() {
        return Err(Error::DatabaseNotFound(path.to_path_buf()));
    }

    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.pragma_update(None, "query_only", true)?;
    Ok(connection)
}

pub(crate) fn open_read_write(path: &Path) -> Result<Connection, Error> {
    if !path.exists() {
        return Err(Error::DatabaseNotFound(path.to_path_buf()));
    }

    let connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    Ok(connection)
}

pub fn list_sidebar_threads(connection: &Connection) -> Result<Vec<SidebarThread>, Error> {
    let mut statement = connection.prepare(
        "SELECT thread_id, session_id, agent_id, title, updated_at, created_at, \
         folder_paths, folder_paths_order, archived, main_worktree_paths, \
         main_worktree_paths_order, remote_connection, interacted_at, title_override \
         FROM sidebar_threads \
         ORDER BY updated_at DESC",
    )?;

    let rows = statement.query_map([], |row| {
        Ok(SidebarThread {
            thread_id: row.get(0)?,
            session_id: row.get(1)?,
            agent_id: row.get(2)?,
            title: row.get(3)?,
            updated_at: row.get(4)?,
            created_at: row.get(5)?,
            folder_paths: row.get(6)?,
            folder_paths_order: row.get(7)?,
            archived: row.get::<_, i64>(8)? != 0,
            main_worktree_paths: row.get(9)?,
            main_worktree_paths_order: row.get(10)?,
            remote_connection: row.get(11)?,
            interacted_at: row.get(12)?,
            title_override: row.get(13)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Error::from)
}

/// Set the sidebar archive flag and make a backup first.
pub fn set_archived(
    db_path: &Path,
    thread_id: &[u8],
    archived: bool,
    allow_running: bool,
) -> Result<PathBuf, Error> {
    if !allow_running && zed_is_running() {
        return Err(Error::ZedRunning);
    }

    let mut connection = open_read_write(db_path)?;
    let date = Utc::now().format("%Y%m%d");
    let file_name = db_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("database");
    let backup = db_path.with_file_name(format!("{file_name}.zed-thread-tools-archive-{date}.bak"));
    if !backup.exists() {
        backup_database(&connection, &backup)?;
    }

    let transaction = connection.transaction()?;
    let updated = transaction.execute(
        "UPDATE sidebar_threads SET archived = ?1 WHERE thread_id = ?2",
        rusqlite::params![archived, thread_id],
    )?;
    if updated != 1 {
        return Err(Error::SidebarThreadNotFound(hex_encode(thread_id)));
    }
    transaction.commit()?;
    Ok(backup)
}
