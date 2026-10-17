//! The Zed thread store: schema, read-only opening, and decompression.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, OptionalExtension};

use crate::Error;

/// The only data type this tool understands. Zed also has `json`, which is
/// accepted on read and never written.
pub const DATA_TYPE_ZSTD: &str = "zstd";
pub const DATA_TYPE_JSON: &str = "json";

/// The thread format version this tool understands. `DbThread::VERSION` in
/// Zed's `crates/agent/src/db.rs` is the source of truth; a different version
/// gets a refusal rather than a guess.
pub const DB_THREAD_VERSION: &str = "0.3.0";

/// The columns the audit needs, plus the one the recover command copies when
/// it forks a thread. `worktree_branch` is present in the live database even
/// though the current Zed schema does not create it.
#[derive(Debug, Clone)]
pub struct ThreadHeader {
    pub id: String,
    pub summary: String,
    pub updated_at: String,
    pub data_type: String,
    pub compressed_bytes: u64,
    pub parent_id: Option<String>,
    pub worktree_branch: Option<String>,
    pub folder_paths: Option<String>,
    pub folder_paths_order: Option<String>,
    pub created_at: Option<String>,
}

impl ThreadHeader {
    /// `folder_paths` is a newline-separated list, not JSON. The order column
    /// exists, but the audit only needs the paths themselves.
    pub fn project_paths(&self) -> Vec<String> {
        self.folder_paths
            .as_deref()
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(str::to_owned)
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct ThreadRecord {
    pub header: ThreadHeader,
    pub data: Vec<u8>,
}

/// The natural location of Zed's thread store. The macOS path is the one this
/// tool was written against; the other two are the same convention Zed uses.
pub fn default_db_path() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        return std::env::var_os("HOME").map(|home| {
            PathBuf::from(home).join("Library/Application Support/Zed/threads/threads.db")
        });
    }

    if cfg!(target_os = "windows") {
        return std::env::var_os("APPDATA")
            .map(|appdata| PathBuf::from(appdata).join("Zed/threads/threads.db"));
    }

    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
        return Some(PathBuf::from(data_home).join("zed/threads/threads.db"));
    }

    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".local/share/zed/threads/threads.db"))
}

/// Open the store with no write capability at all. `query_only` is a second
/// guard on the connection, not a substitute for the read-only flag.
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

/// Open the store for the recover command. The caller is responsible for
/// quitting Zed first; `recover::write_recovery` checks for that.
pub fn open_read_write(path: &Path) -> Result<Connection, Error> {
    if !path.exists() {
        return Err(Error::DatabaseNotFound(path.to_path_buf()));
    }

    let connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    Ok(connection)
}

/// One row per thread, without loading the blobs. `length(data)` gives the
/// compressed size for free, which is what the first pass of the audit sorts
/// and filters on.
pub fn list_headers(connection: &Connection) -> Result<Vec<ThreadHeader>, Error> {
    let mut statement = connection.prepare(
        "SELECT id, summary, updated_at, data_type, length(data), parent_id, \
         worktree_branch, folder_paths, folder_paths_order, created_at \
         FROM threads \
         ORDER BY updated_at DESC, created_at DESC",
    )?;

    let rows = statement.query_map([], |row| {
        Ok(ThreadHeader {
            id: row.get(0)?,
            summary: row.get(1)?,
            updated_at: row.get(2)?,
            data_type: row.get(3)?,
            compressed_bytes: row.get::<_, i64>(4)?.max(0) as u64,
            parent_id: row.get(5)?,
            worktree_branch: row.get(6)?,
            folder_paths: row.get(7)?,
            folder_paths_order: row.get(8)?,
            created_at: row.get(9)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Error::from)
}

/// Load one thread, including its compressed blob. The audit calls this once
/// per matching thread, so only one blob is resident at a time.
pub fn load_record(connection: &Connection, id: &str) -> Result<ThreadRecord, Error> {
    let mut statement = connection.prepare(
        "SELECT id, summary, updated_at, data_type, data, parent_id, \
         worktree_branch, folder_paths, folder_paths_order, created_at \
         FROM threads WHERE id = ?1",
    )?;

    let mut rows = statement.query_map([id], |row| {
        let data: Vec<u8> = row.get(4)?;
        let compressed_bytes = data.len() as u64;
        Ok(ThreadRecord {
            header: ThreadHeader {
                id: row.get(0)?,
                summary: row.get(1)?,
                updated_at: row.get(2)?,
                data_type: row.get(3)?,
                compressed_bytes,
                parent_id: row.get(5)?,
                worktree_branch: row.get(6)?,
                folder_paths: row.get(7)?,
                folder_paths_order: row.get(8)?,
                created_at: row.get(9)?,
            },
            data,
        })
    })?;

    match rows.next() {
        Some(row) => row.map_err(Error::from),
        None => Err(Error::ThreadNotFound(id.to_owned())),
    }
}

/// Walk `parent_id` up to the top-level thread. Exporting a subagent includes
/// its ancestors so a reimport cannot produce a dangling parent reference.
pub fn thread_root_id(connection: &Connection, id: &str) -> Result<String, Error> {
    let mut current = id.to_owned();
    loop {
        let parent: Option<Option<String>> = connection
            .query_row(
                "SELECT parent_id FROM threads WHERE id = ?1",
                [&current],
                |row| row.get(0),
            )
            .optional()?;
        match parent {
            Some(Some(parent)) => current = parent,
            _ => return Ok(current),
        }
    }
}

/// The root thread and every subagent below it, in tree order. Zed deletes the
/// whole tree when it deletes a thread, so export and delete do too.
pub fn thread_tree_ids(connection: &Connection, root_id: &str) -> Result<Vec<String>, Error> {
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM threads WHERE id = ?1)",
        [root_id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(Error::ThreadNotFound(root_id.to_owned()));
    }

    let mut statement = connection.prepare(
        "WITH RECURSIVE tree(id) AS (
             SELECT ?1
             UNION ALL
             SELECT threads.id FROM threads JOIN tree ON threads.parent_id = tree.id
         )
         SELECT id FROM tree",
    )?;
    let rows = statement.query_map([root_id], |row| row.get::<_, String>(0))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Error::from)
}

/// Decode a stored blob to the JSON bytes. `json` rows are passed through;
/// `zstd` rows are decompressed with the same frame format Zed writes.
pub fn decode_thread(data_type: &str, data: &[u8]) -> Result<Vec<u8>, Error> {
    match data_type {
        DATA_TYPE_ZSTD => Ok(zstd::stream::decode_all(data)?),
        DATA_TYPE_JSON => Ok(data.to_vec()),
        other => Err(Error::UnsupportedDataType(other.to_owned())),
    }
}
