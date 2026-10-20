#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::{params, Connection};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new(prefix: &str) -> Self {
        let counter = NEXT_DIR.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("{prefix}-{}-{counter}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

pub fn create_db(path: &Path) -> Connection {
    let connection = Connection::open(path).expect("open fixture database");
    connection
        .execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                summary TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                data_type TEXT NOT NULL,
                data BLOB NOT NULL,
                parent_id TEXT,
                worktree_branch TEXT,
                folder_paths TEXT,
                folder_paths_order TEXT,
                created_at TEXT
            );",
        )
        .expect("create threads table");
    connection
}

pub fn insert_thread(
    connection: &Connection,
    id: &str,
    summary: &str,
    updated_at: &str,
    folder_paths: Option<&str>,
    data_type: &str,
    data: &[u8],
) {
    insert_thread_with_parent(
        connection,
        id,
        None,
        summary,
        updated_at,
        folder_paths,
        data_type,
        data,
    );
}

#[allow(clippy::too_many_arguments)]
pub fn insert_thread_with_parent(
    connection: &Connection,
    id: &str,
    parent_id: Option<&str>,
    summary: &str,
    updated_at: &str,
    folder_paths: Option<&str>,
    data_type: &str,
    data: &[u8],
) {
    connection
        .execute(
            "INSERT INTO threads \
             (id, summary, updated_at, data_type, data, parent_id, folder_paths, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?3)",
            params![
                id,
                summary,
                updated_at,
                data_type,
                data,
                parent_id,
                folder_paths
            ],
        )
        .expect("insert fixture thread");
}

pub fn create_sidebar_db(path: &Path) -> Connection {
    let connection = Connection::open(path).expect("open fixture sidebar database");
    connection
        .execute_batch(
            "CREATE TABLE sidebar_threads (
                thread_id BLOB PRIMARY KEY,
                session_id TEXT,
                agent_id TEXT,
                title TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                created_at TEXT,
                folder_paths TEXT,
                folder_paths_order TEXT,
                archived INTEGER DEFAULT 0,
                main_worktree_paths TEXT,
                main_worktree_paths_order TEXT,
                remote_connection TEXT,
                interacted_at TEXT,
                title_override TEXT
            );
            CREATE TABLE archived_git_worktrees (
                id INTEGER PRIMARY KEY,
                worktree_path TEXT NOT NULL,
                main_repo_path TEXT NOT NULL,
                branch_name TEXT,
                staged_commit_hash TEXT,
                unstaged_commit_hash TEXT,
                original_commit_hash TEXT
            );
            CREATE TABLE thread_archived_worktrees (
                thread_id BLOB NOT NULL,
                archived_worktree_id INTEGER NOT NULL,
                PRIMARY KEY (thread_id, archived_worktree_id)
            );",
        )
        .expect("create sidebar tables");
    connection
}

pub fn insert_sidebar_thread(
    connection: &Connection,
    thread_id: &[u8],
    session_id: Option<&str>,
    title: &str,
    updated_at: &str,
    archived: bool,
) {
    connection
        .execute(
            "INSERT INTO sidebar_threads \
             (thread_id, session_id, agent_id, title, updated_at, created_at, folder_paths, \
              archived, main_worktree_paths) \
             VALUES (?1, ?2, 'native_agent', ?3, ?4, ?4, '/repo/project', ?5, '/repo/project')",
            params![thread_id, session_id, title, updated_at, archived],
        )
        .expect("insert fixture sidebar thread");
}

pub fn compress(json: &str) -> Vec<u8> {
    zstd::stream::encode_all(json.as_bytes(), 3).expect("compress fixture thread")
}

pub fn sample_messages() -> &'static str {
    r#"[
        {"User":{"id":"u1","content":[{"Text":"hello"}]}},
        {"Agent":{"content":[
            {"Text":"hi"},
            {"ToolUse":{"id":"t1","name":"read_file","raw_input":"{}","input":{"path":"/repo/file.rs"},"is_input_complete":true,"thought_signature":null}}
        ],"tool_results":{"t1":{
            "tool_use_id":"t1",
            "tool_name":"read_file",
            "is_error":false,
            "content":[{"Text":"0123456789"}],
            "output":null
        }},"reasoning_details":null}}
    ]"#
}

/// The older shape: one content block rather than an array.
pub fn legacy_messages() -> &'static str {
    r#"[
        {"User":{"id":"u2","content":[{"Text":"legacy"}]}},
        {"Agent":{"content":[{"Text":"old result"}],"tool_results":{"t2":{
            "tool_use_id":"t2",
            "tool_name":"read_file",
            "is_error":false,
            "content":{"Text":"0123456789"},
            "output":null
        }},"reasoning_details":null}}
    ]"#
}

pub fn thread_json(title: &str, updated_at: &str, messages: &str, extra: &str) -> String {
    format!(
        r#"{{"title":"{title}","messages":{messages},"updated_at":"{updated_at}","detailed_summary":null,"initial_project_snapshot":null,"cumulative_token_usage":{{}},"request_token_usage":{{}},"model":{{"provider":"test","model":"fake"}},"profile":null,"subagent_context":null,"speed":null,"thinking_enabled":false,"thinking_effort":null,"draft_prompt":null,"ui_scroll_position":null,"sandboxed_terminal_temp_dir":null,"sandbox_grants":{{}},"version":"0.3.0"{extra}}}"#
    )
}
