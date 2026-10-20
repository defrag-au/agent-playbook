//! Export, import and delete Zed threads.
//!
//! A bundle is a directory with a typed `manifest.json` and one raw data file per
//! `threads.db` row. The raw blob is written unchanged, so unknown fields and
//! the original compression survive a round trip. A native thread's whole
//! subagent tree is included. ACP threads have only sidebar metadata in Zed, so
//! their bundle contains no transcript.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::db;
use crate::metadata::{self, SidebarThread};
use crate::recover::{backup_database, backup_path, zed_is_running};
use crate::util::{hex_decode, slugify};
use crate::{Error, VERSION};

pub const BUNDLE_FORMAT_VERSION: u32 = 1;

/// Archived bundles live in Documents so they are easy to find and back up.
pub fn default_archive_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Documents/zed-thread-archives"))
        .unwrap_or_else(|| PathBuf::from(".zed-thread-archives"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleManifest {
    pub format_version: u32,
    pub tool_version: String,
    pub exported_at: String,
    pub root_thread_id: Option<String>,
    pub title: String,
    pub threads: Vec<BundleThread>,
    pub sidebar_threads: Vec<BundleSidebarThread>,
    #[serde(default)]
    pub archived_worktrees: Vec<BundleArchivedWorktree>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleThread {
    pub id: String,
    pub parent_id: Option<String>,
    pub summary: String,
    pub updated_at: String,
    pub data_type: String,
    pub data_file: String,
    pub worktree_branch: Option<String>,
    pub folder_paths: Option<String>,
    pub folder_paths_order: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleSidebarThread {
    pub thread_id_hex: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleArchivedWorktree {
    pub thread_id_hex: String,
    pub worktree_path: String,
    pub main_repo_path: String,
    pub branch_name: Option<String>,
    pub staged_commit_hash: Option<String>,
    pub unstaged_commit_hash: Option<String>,
    pub original_commit_hash: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BundleEntry {
    pub path: PathBuf,
    pub manifest: Option<BundleManifest>,
    pub error: Option<String>,
}

impl BundleEntry {
    pub fn title(&self) -> &str {
        self.manifest
            .as_ref()
            .map(|manifest| manifest.title.as_str())
            .unwrap_or_else(|| {
                self.path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("invalid bundle")
            })
    }

    pub fn exported_at(&self) -> &str {
        self.manifest
            .as_ref()
            .map(|manifest| manifest.exported_at.as_str())
            .unwrap_or("-")
    }

    pub fn thread_count(&self) -> usize {
        self.manifest
            .as_ref()
            .map(|manifest| manifest.threads.len())
            .unwrap_or(0)
    }

    pub fn sidebar_count(&self) -> usize {
        self.manifest
            .as_ref()
            .map(|manifest| manifest.sidebar_threads.len())
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone)]
pub struct ExportOutcome {
    pub bundle_dir: PathBuf,
    pub threads: usize,
    pub sidebar_threads: usize,
    pub archived_worktrees: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone)]
pub struct ImportOutcome {
    pub threads: usize,
    pub sidebar_threads: usize,
    pub archived_worktrees: usize,
    pub threads_backup: Option<PathBuf>,
    pub zed_backup: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct DeleteOutcome {
    pub threads: usize,
    pub sidebar_threads: usize,
    pub threads_backup: Option<PathBuf>,
    pub zed_backup: Option<PathBuf>,
}

/// Write a bundle for one sidebar row and/or one `threads.db` tree.
pub fn export_thread(
    threads_db: &Path,
    zed_db: &Path,
    archive_dir: &Path,
    sidebar: Option<&SidebarThread>,
    root_id: Option<&str>,
) -> Result<ExportOutcome, Error> {
    if sidebar.is_none() && root_id.is_none() {
        return Err(Error::NothingToDo);
    }

    let threads_connection = if root_id.is_some() {
        Some(db::open_read_only(threads_db)?)
    } else {
        None
    };

    let export_root = match (&threads_connection, root_id) {
        (Some(connection), Some(root_id)) => Some(db::thread_root_id(connection, root_id)?),
        _ => None,
    };
    let thread_ids = match (&threads_connection, export_root.as_deref()) {
        (Some(connection), Some(root)) => db::thread_tree_ids(connection, root)?,
        _ => Vec::new(),
    };

    let mut thread_records = Vec::new();
    if let Some(connection) = &threads_connection {
        for id in &thread_ids {
            thread_records.push(db::load_record(connection, id)?);
        }
    }

    let mut sidebar_rows = Vec::new();
    let mut archived_worktrees = Vec::new();
    if sidebar.is_some() || !thread_ids.is_empty() {
        let connection = metadata::open_read_only(zed_db)?;
        for candidate in metadata::list_sidebar_threads(&connection)? {
            let same_sidebar = sidebar
                .map(|row| row.thread_id == candidate.thread_id)
                .unwrap_or(false);
            let same_session = candidate
                .session_id
                .as_ref()
                .map(|session| thread_ids.iter().any(|id| id == session))
                .unwrap_or(false);
            if same_sidebar || same_session {
                for worktree in sidebar_worktrees(&connection, &candidate)? {
                    archived_worktrees.push(worktree);
                }
                sidebar_rows.push(candidate);
            }
        }
    }
    if let Some(sidebar) = sidebar {
        if !sidebar_rows
            .iter()
            .any(|row| row.thread_id == sidebar.thread_id)
        {
            sidebar_rows.push(sidebar.clone());
        }
    }

    if thread_records.is_empty() && sidebar_rows.is_empty() {
        return Err(Error::NothingToDo);
    }

    let title = sidebar
        .map(|row| row.display_title().to_owned())
        .or_else(|| {
            thread_records
                .first()
                .map(|record| record.header.summary.clone())
        })
        .or_else(|| {
            sidebar_rows
                .first()
                .map(|row| row.display_title().to_owned())
        })
        .unwrap_or_else(|| "thread".to_owned());

    std::fs::create_dir_all(archive_dir)?;
    let bundle_dir = unique_bundle_dir(archive_dir, &title, export_root.as_deref(), sidebar);
    let result = write_bundle(
        &bundle_dir,
        &title,
        export_root.as_deref(),
        &thread_records,
        &sidebar_rows,
        &archived_worktrees,
    );
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&bundle_dir);
    }
    result
}

fn write_bundle(
    bundle_dir: &Path,
    title: &str,
    root_id: Option<&str>,
    thread_records: &[db::ThreadRecord],
    sidebar_rows: &[SidebarThread],
    archived_worktrees: &[BundleArchivedWorktree],
) -> Result<ExportOutcome, Error> {
    std::fs::create_dir_all(bundle_dir.join("threads"))?;

    let mut threads = Vec::with_capacity(thread_records.len());
    let mut bytes = 0_u64;
    for record in thread_records {
        if !safe_id(&record.header.id) {
            return Err(Error::BundleInvalid(format!(
                "thread id {:?} is not a safe file name",
                record.header.id
            )));
        }
        let data_file = format!("threads/{}.data", record.header.id);
        std::fs::write(bundle_dir.join(&data_file), &record.data)?;
        bytes += record.data.len() as u64;
        threads.push(BundleThread {
            id: record.header.id.clone(),
            parent_id: record.header.parent_id.clone(),
            summary: record.header.summary.clone(),
            updated_at: record.header.updated_at.clone(),
            data_type: record.header.data_type.clone(),
            data_file,
            worktree_branch: record.header.worktree_branch.clone(),
            folder_paths: record.header.folder_paths.clone(),
            folder_paths_order: record.header.folder_paths_order.clone(),
            created_at: record.header.created_at.clone(),
        });
    }

    let sidebar_threads: Vec<BundleSidebarThread> = sidebar_rows
        .iter()
        .map(|row| BundleSidebarThread {
            thread_id_hex: row.thread_id_hex(),
            session_id: row.session_id.clone(),
            agent_id: row.agent_id.clone(),
            title: row.title.clone(),
            updated_at: row.updated_at.clone(),
            created_at: row.created_at.clone(),
            folder_paths: row.folder_paths.clone(),
            folder_paths_order: row.folder_paths_order.clone(),
            archived: row.archived,
            main_worktree_paths: row.main_worktree_paths.clone(),
            main_worktree_paths_order: row.main_worktree_paths_order.clone(),
            remote_connection: row.remote_connection.clone(),
            interacted_at: row.interacted_at.clone(),
            title_override: row.title_override.clone(),
        })
        .collect();

    let manifest = BundleManifest {
        format_version: BUNDLE_FORMAT_VERSION,
        tool_version: VERSION.to_owned(),
        exported_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        root_thread_id: root_id.map(str::to_owned),
        title: title.to_owned(),
        threads,
        sidebar_threads,
        archived_worktrees: archived_worktrees.to_vec(),
    };
    std::fs::write(
        bundle_dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;

    Ok(ExportOutcome {
        bundle_dir: bundle_dir.to_path_buf(),
        threads: manifest.threads.len(),
        sidebar_threads: manifest.sidebar_threads.len(),
        archived_worktrees: manifest.archived_worktrees.len(),
        bytes,
    })
}

fn unique_bundle_dir(
    archive_dir: &Path,
    title: &str,
    root_id: Option<&str>,
    sidebar: Option<&SidebarThread>,
) -> PathBuf {
    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    let slug = slugify(title);
    let id_source = root_id
        .map(str::to_owned)
        .or_else(|| sidebar.map(SidebarThread::thread_id_hex))
        .unwrap_or_else(|| "thread".to_owned());
    let short_id: String = id_source
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .take(8)
        .collect();
    let base = format!("{stamp}-{slug}-{short_id}");
    let mut candidate = archive_dir.join(&base);
    let mut suffix = 2;
    while candidate.exists() {
        candidate = archive_dir.join(format!("{base}-{suffix}"));
        suffix += 1;
    }
    candidate
}

fn sidebar_worktrees(
    connection: &Connection,
    sidebar: &SidebarThread,
) -> Result<Vec<BundleArchivedWorktree>, Error> {
    let mut statement = connection.prepare(
        "SELECT a.worktree_path, a.main_repo_path, a.branch_name, a.staged_commit_hash, \
         a.unstaged_commit_hash, a.original_commit_hash \
         FROM archived_git_worktrees a \
         JOIN thread_archived_worktrees t ON a.id = t.archived_worktree_id \
         WHERE t.thread_id = ?1",
    )?;
    let rows = statement.query_map([&sidebar.thread_id], |row| {
        Ok(BundleArchivedWorktree {
            thread_id_hex: sidebar.thread_id_hex(),
            worktree_path: row.get(0)?,
            main_repo_path: row.get(1)?,
            branch_name: row.get(2)?,
            staged_commit_hash: row.get(3)?,
            unstaged_commit_hash: row.get(4)?,
            original_commit_hash: row.get(5)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Error::from)
}

pub fn list_bundles(archive_dir: &Path) -> Result<Vec<BundleEntry>, Error> {
    if !archive_dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    for entry in std::fs::read_dir(archive_dir)? {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }
        let manifest_path = path.join("manifest.json");
        if !manifest_path.exists() {
            continue;
        }
        let manifest = std::fs::read(&manifest_path)
            .map_err(Error::from)
            .and_then(|bytes| {
                serde_json::from_slice::<BundleManifest>(&bytes).map_err(|error| {
                    Error::BundleInvalid(format!("{}: {error}", manifest_path.display()))
                })
            });
        match manifest {
            Ok(manifest) => entries.push(BundleEntry {
                path,
                manifest: Some(manifest),
                error: None,
            }),
            Err(error) => entries.push(BundleEntry {
                path,
                manifest: None,
                error: Some(error.to_string()),
            }),
        }
    }
    entries.sort_by(|left, right| {
        right
            .exported_at()
            .cmp(left.exported_at())
            .then_with(|| right.path.cmp(&left.path))
    });
    Ok(entries)
}

/// Import one bundle. Existing ids are never overwritten.
pub fn import_bundle(
    threads_db: &Path,
    zed_db: &Path,
    bundle_dir: &Path,
    allow_running: bool,
) -> Result<ImportOutcome, Error> {
    let manifest_path = bundle_dir.join("manifest.json");
    let bytes = std::fs::read(&manifest_path)?;
    let manifest: BundleManifest = serde_json::from_slice(&bytes)
        .map_err(|error| Error::BundleInvalid(format!("manifest: {error}")))?;
    if manifest.format_version != BUNDLE_FORMAT_VERSION {
        return Err(Error::UnsupportedBundleVersion(manifest.format_version));
    }
    if manifest.threads.is_empty() && manifest.sidebar_threads.is_empty() {
        return Err(Error::NothingToDo);
    }

    let mut sidebar_ids = HashMap::new();
    for sidebar in &manifest.sidebar_threads {
        let id = hex_decode(&sidebar.thread_id_hex).ok_or_else(|| {
            Error::BundleInvalid(format!("bad sidebar id {:?}", sidebar.thread_id_hex))
        })?;
        if id.len() != 16 {
            return Err(Error::BundleInvalid(format!(
                "sidebar id {:?} is not 16 bytes",
                sidebar.thread_id_hex
            )));
        }
        if sidebar_ids
            .insert(sidebar.thread_id_hex.clone(), id)
            .is_some()
        {
            return Err(Error::BundleInvalid(format!(
                "duplicate sidebar id {:?}",
                sidebar.thread_id_hex
            )));
        }
    }

    for thread in &manifest.threads {
        if !safe_id(&thread.id) {
            return Err(Error::BundleInvalid(format!(
                "unsafe thread id {:?}",
                thread.id
            )));
        }
        let data_path = Path::new(&thread.data_file);
        if data_path.is_absolute()
            || data_path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(Error::BundleInvalid(format!(
                "unsafe data file {:?}",
                thread.data_file
            )));
        }
        if !bundle_dir.join(data_path).exists() {
            return Err(Error::BundleInvalid(format!(
                "missing data file {:?}",
                thread.data_file
            )));
        }
    }
    for worktree in &manifest.archived_worktrees {
        if !sidebar_ids.contains_key(&worktree.thread_id_hex) {
            return Err(Error::BundleInvalid(format!(
                "archived worktree references missing sidebar id {:?}",
                worktree.thread_id_hex
            )));
        }
    }

    if !allow_running && zed_is_running() {
        return Err(Error::ZedRunning);
    }

    if !manifest.threads.is_empty() {
        let connection = db::open_read_only(threads_db)?;
        for thread in &manifest.threads {
            let exists: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM threads WHERE id = ?1)",
                [&thread.id],
                |row| row.get(0),
            )?;
            if exists {
                return Err(Error::ThreadConflict(thread.id.clone()));
            }
        }
    }
    if !manifest.sidebar_threads.is_empty() {
        let connection = metadata::open_read_only(zed_db)?;
        for sidebar in &manifest.sidebar_threads {
            let id = &sidebar_ids[&sidebar.thread_id_hex];
            let exists: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM sidebar_threads WHERE thread_id = ?1)",
                [id],
                |row| row.get(0),
            )?;
            if exists {
                return Err(Error::ThreadConflict(sidebar.thread_id_hex.clone()));
            }
        }
    }

    let mut threads_connection = if manifest.threads.is_empty() {
        None
    } else {
        Some(db::open_read_write(threads_db)?)
    };
    let threads_backup = match &threads_connection {
        Some(connection) => {
            let backup = backup_path(threads_db, "zed-thread-tools-import");
            backup_database(connection, &backup)?;
            Some(backup)
        }
        None => None,
    };

    if let Some(connection) = threads_connection.as_mut() {
        let transaction = connection.transaction()?;
        for thread in &manifest.threads {
            let data = std::fs::read(bundle_dir.join(&thread.data_file))?;
            transaction.execute(
                "INSERT INTO threads \
                 (id, summary, updated_at, data_type, data, parent_id, worktree_branch, \
                  folder_paths, folder_paths_order, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    thread.id,
                    thread.summary,
                    thread.updated_at,
                    thread.data_type,
                    data,
                    thread.parent_id,
                    thread.worktree_branch,
                    thread.folder_paths,
                    thread.folder_paths_order,
                    thread.created_at,
                ],
            )?;
        }
        transaction.commit()?;
    }

    let zed_backup = match write_sidebar_rows(zed_db, &manifest, &sidebar_ids) {
        Ok(backup) => backup,
        Err(error) => {
            if let Err(cleanup) = remove_thread_rows(threads_db, &manifest) {
                return Err(Error::BundleInvalid(format!(
                    "{error}; rollback also failed: {cleanup}"
                )));
            }
            return Err(error);
        }
    };

    Ok(ImportOutcome {
        threads: manifest.threads.len(),
        sidebar_threads: manifest.sidebar_threads.len(),
        archived_worktrees: manifest.archived_worktrees.len(),
        threads_backup,
        zed_backup,
    })
}

fn write_sidebar_rows(
    zed_db: &Path,
    manifest: &BundleManifest,
    sidebar_ids: &HashMap<String, Vec<u8>>,
) -> Result<Option<PathBuf>, Error> {
    if manifest.sidebar_threads.is_empty() && manifest.archived_worktrees.is_empty() {
        return Ok(None);
    }

    let mut connection = metadata::open_read_write(zed_db)?;
    let backup = backup_path(zed_db, "zed-thread-tools-import");
    backup_database(&connection, &backup)?;
    let transaction = connection.transaction()?;

    for sidebar in &manifest.sidebar_threads {
        let thread_id = &sidebar_ids[&sidebar.thread_id_hex];
        transaction.execute(
            "INSERT INTO sidebar_threads \
             (thread_id, session_id, agent_id, title, updated_at, created_at, folder_paths, \
              folder_paths_order, archived, main_worktree_paths, main_worktree_paths_order, \
              remote_connection, interacted_at, title_override) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                thread_id,
                sidebar.session_id,
                sidebar.agent_id,
                sidebar.title,
                sidebar.updated_at,
                sidebar.created_at,
                sidebar.folder_paths,
                sidebar.folder_paths_order,
                sidebar.archived,
                sidebar.main_worktree_paths,
                sidebar.main_worktree_paths_order,
                sidebar.remote_connection,
                sidebar.interacted_at,
                sidebar.title_override,
            ],
        )?;
    }

    for worktree in &manifest.archived_worktrees {
        transaction.execute(
            "INSERT INTO archived_git_worktrees \
             (worktree_path, main_repo_path, branch_name, staged_commit_hash, \
              unstaged_commit_hash, original_commit_hash) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                worktree.worktree_path,
                worktree.main_repo_path,
                worktree.branch_name,
                worktree.staged_commit_hash,
                worktree.unstaged_commit_hash,
                worktree.original_commit_hash,
            ],
        )?;
        let archived_worktree_id = transaction.last_insert_rowid();
        let thread_id = &sidebar_ids[&worktree.thread_id_hex];
        transaction.execute(
            "INSERT INTO thread_archived_worktrees (thread_id, archived_worktree_id) \
             VALUES (?1, ?2)",
            params![thread_id, archived_worktree_id],
        )?;
    }

    transaction.commit()?;
    Ok(Some(backup))
}

fn remove_thread_rows(threads_db: &Path, manifest: &BundleManifest) -> Result<(), Error> {
    if manifest.threads.is_empty() {
        return Ok(());
    }
    let mut connection = db::open_read_write(threads_db)?;
    let transaction = connection.transaction()?;
    for thread in &manifest.threads {
        transaction.execute("DELETE FROM threads WHERE id = ?1", [&thread.id])?;
    }
    transaction.commit()?;
    Ok(())
}

/// Delete one thread. Native threads take their subagent tree with them.
pub fn delete_thread(
    threads_db: &Path,
    zed_db: &Path,
    sidebar: Option<&SidebarThread>,
    root_id: Option<&str>,
    allow_running: bool,
) -> Result<DeleteOutcome, Error> {
    if !allow_running && zed_is_running() {
        return Err(Error::ZedRunning);
    }
    if sidebar.is_none() && root_id.is_none() {
        return Err(Error::NothingToDo);
    }

    let thread_ids = match root_id {
        Some(root_id) => {
            let connection = db::open_read_only(threads_db)?;
            db::thread_tree_ids(&connection, root_id)?
        }
        None => Vec::new(),
    };

    let mut sidebar_rows = Vec::new();
    if sidebar.is_some() || !thread_ids.is_empty() {
        let connection = metadata::open_read_only(zed_db)?;
        for row in metadata::list_sidebar_threads(&connection)? {
            let same_sidebar = sidebar
                .map(|selected| selected.thread_id == row.thread_id)
                .unwrap_or(false);
            let same_session = row
                .session_id
                .as_ref()
                .map(|session| thread_ids.iter().any(|id| id == session))
                .unwrap_or(false);
            if same_sidebar || same_session {
                sidebar_rows.push(row);
            }
        }
    }
    if thread_ids.is_empty() && sidebar_rows.is_empty() {
        return match sidebar {
            Some(sidebar) => Err(Error::SidebarThreadNotFound(sidebar.thread_id_hex())),
            None => Err(Error::NothingToDo),
        };
    }

    let mut threads_backup = None;
    if !thread_ids.is_empty() {
        let mut connection = db::open_read_write(threads_db)?;
        let backup = backup_path(threads_db, "zed-thread-tools-delete");
        backup_database(&connection, &backup)?;
        let transaction = connection.transaction()?;
        for id in &thread_ids {
            transaction.execute("DELETE FROM threads WHERE id = ?1", [id])?;
        }
        transaction.commit()?;
        threads_backup = Some(backup);
    }

    let mut zed_backup = None;
    if !sidebar_rows.is_empty() {
        let mut connection = metadata::open_read_write(zed_db)?;
        let backup = backup_path(zed_db, "zed-thread-tools-delete");
        backup_database(&connection, &backup)?;
        let transaction = connection.transaction()?;
        for row in &sidebar_rows {
            transaction.execute(
                "DELETE FROM thread_archived_worktrees WHERE thread_id = ?1",
                [&row.thread_id],
            )?;
            transaction.execute(
                "DELETE FROM sidebar_threads WHERE thread_id = ?1",
                [&row.thread_id],
            )?;
        }
        transaction.commit()?;
        zed_backup = Some(backup);
    }

    Ok(DeleteOutcome {
        threads: thread_ids.len(),
        sidebar_threads: sidebar_rows.len(),
        threads_backup,
        zed_backup,
    })
}

fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}
