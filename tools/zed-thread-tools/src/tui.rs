//! `zed-thread-tui` — browse, archive, export, import and recover Zed threads.
//!
//! The list is the union of two stores: `sidebar_threads` in `db.sqlite` (which
//! includes ACP threads) and the native transcripts in `threads.db`. Native
//! rows carry size/tool/compaction columns from the audit; ACP rows are
//! metadata-only because Zed does not own their transcripts.
//!
//! Writes follow one rule: Zed must be quit, both affected databases are backed
//! up first, and nothing is overwritten on import.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{DateTime, Utc};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Clear, Paragraph, Row, Table, TableState, Wrap};
use ratatui::{DefaultTerminal, Frame};

use crate::archive::{self, BundleEntry};
use crate::audit::{self, AuditOptions, ThreadAudit};
use crate::db;
use crate::metadata::{self, SidebarThread};
use crate::recover::{self, PreparedRecovery, RecoverOptions};
use crate::util::slugify;
use crate::{Error, ThreadRecord};

const SUMMARY_PLACEHOLDER: &str =
    "<!-- REPLACE THIS FILE WITH THE COMPACTION SUMMARY THE MODEL SHOULD SEE -->";

/// Start the TUI against both Zed stores.
pub fn run(
    threads_db: PathBuf,
    zed_db: PathBuf,
    summary_dir: PathBuf,
    archive_dir: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let library = load_library(&threads_db, &zed_db)?;
    let mut app = App::new(threads_db, zed_db, summary_dir, archive_dir, library);
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &mut app);
    ratatui::restore();
    result
}

fn load_library(threads_db: &Path, zed_db: &Path) -> Result<Library, Error> {
    let report = audit::audit(
        threads_db,
        &AuditOptions {
            top: 0,
            older_than: None,
            project: None,
        },
    )?;
    let native_total = report.threads.len();
    let mut audits: HashMap<String, ThreadAudit> = report
        .threads
        .into_iter()
        .map(|thread| (thread.id.clone(), thread))
        .collect();

    let connection = metadata::open_read_only(zed_db)?;
    let sidebar_threads = metadata::list_sidebar_threads(&connection)?;
    let sidebar_total = sidebar_threads.len();
    let mut rows = Vec::with_capacity(sidebar_total + audits.len());

    for sidebar in sidebar_threads {
        let audit = sidebar
            .session_id
            .as_ref()
            .and_then(|session_id| audits.remove(session_id));
        rows.push(ThreadRow {
            sidebar: Some(sidebar),
            audit,
        });
    }
    for audit in audits.into_values() {
        rows.push(ThreadRow {
            sidebar: None,
            audit: Some(audit),
        });
    }

    Ok(Library {
        rows,
        sidebar_total,
        native_total,
    })
}

struct Library {
    rows: Vec<ThreadRow>,
    sidebar_total: usize,
    native_total: usize,
}

struct ThreadRow {
    sidebar: Option<SidebarThread>,
    audit: Option<ThreadAudit>,
}

impl ThreadRow {
    fn key(&self) -> String {
        if let Some(sidebar) = &self.sidebar {
            format!("sidebar:{}", sidebar.thread_id_hex())
        } else if let Some(audit) = &self.audit {
            format!("thread:{}", audit.id)
        } else {
            String::new()
        }
    }

    fn title(&self) -> &str {
        if let Some(sidebar) = &self.sidebar {
            return sidebar.display_title();
        }
        if let Some(audit) = &self.audit {
            if !audit.title.is_empty() {
                return &audit.title;
            }
            if !audit.summary.is_empty() {
                return &audit.summary;
            }
            return &audit.id;
        }
        ""
    }

    fn updated_at(&self) -> &str {
        self.sidebar
            .as_ref()
            .map(|sidebar| sidebar.updated_at.as_str())
            .or_else(|| self.audit.as_ref().map(|audit| audit.updated_at.as_str()))
            .unwrap_or("-")
    }

    fn archived(&self) -> bool {
        self.sidebar
            .as_ref()
            .map(|sidebar| sidebar.archived)
            .unwrap_or(false)
    }

    fn session_id(&self) -> Option<&str> {
        self.sidebar
            .as_ref()
            .and_then(|sidebar| sidebar.session_id.as_deref())
            .or_else(|| self.audit.as_ref().map(|audit| audit.id.as_str()))
    }

    fn agent_id(&self) -> Option<&str> {
        self.sidebar
            .as_ref()
            .and_then(|sidebar| sidebar.agent_id.as_deref())
    }

    fn model(&self) -> Option<&str> {
        self.audit.as_ref().and_then(|audit| audit.model.as_deref())
    }

    fn project_paths(&self) -> Vec<&str> {
        let mut paths = Vec::new();
        if let Some(sidebar) = &self.sidebar {
            for path in sidebar.project_paths() {
                if !paths.contains(&path) {
                    paths.push(path);
                }
            }
        }
        if let Some(audit) = &self.audit {
            for path in &audit.project_paths {
                let path = path.as_str();
                if !paths.contains(&path) {
                    paths.push(path);
                }
            }
        }
        paths
    }

    fn age_seconds(&self) -> Option<i64> {
        self.audit
            .as_ref()
            .and_then(|audit| audit.age_seconds)
            .or_else(|| {
                DateTime::parse_from_rfc3339(self.updated_at())
                    .ok()
                    .map(|updated| Utc::now().signed_duration_since(updated).num_seconds())
            })
    }

    fn decoded_bytes(&self) -> u64 {
        self.audit
            .as_ref()
            .and_then(|audit| audit.decoded_bytes)
            .unwrap_or(0)
    }

    fn compressed_bytes(&self) -> u64 {
        self.audit
            .as_ref()
            .map(|audit| audit.compressed_bytes)
            .unwrap_or(0)
    }

    fn messages(&self) -> Option<usize> {
        self.audit.as_ref().and_then(|audit| audit.messages)
    }

    fn tool_calls(&self) -> Option<usize> {
        self.audit.as_ref().and_then(|audit| audit.tool_calls)
    }

    fn tool_output_bytes(&self) -> Option<u64> {
        self.audit
            .as_ref()
            .and_then(|audit| audit.tool_output_bytes)
    }

    fn compactions(&self) -> Option<usize> {
        self.audit.as_ref().and_then(|audit| audit.compactions)
    }

    fn is_subagent(&self) -> bool {
        self.audit
            .as_ref()
            .map(|audit| audit.subagent)
            .unwrap_or(false)
    }

    fn error(&self) -> Option<&str> {
        self.audit.as_ref().and_then(|audit| audit.error.as_deref())
    }
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        terminal.draw(|frame| app.draw(frame))?;

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        match app.handle_key(key) {
            Action::Quit => return Ok(()),
            Action::Reload => {
                let focus = app.selected_row().map(ThreadRow::key);
                app.reload(focus);
            }
            Action::EditSummary => {
                ratatui::restore();
                let result = app.edit_summary();
                *terminal = ratatui::init();
                app.after_edit(result);
            }
            Action::Continue => {}
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Browse,
    Filter,
    Help,
    ConfirmRecovery,
    ConfirmDelete,
    ImportPicker,
    ConfirmImport,
}

#[derive(Debug, Clone, Copy)]
enum Action {
    Continue,
    Quit,
    Reload,
    EditSummary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sort {
    Decoded,
    Updated,
    Messages,
    ToolBytes,
}

impl Sort {
    fn label(self) -> &'static str {
        match self {
            Self::Decoded => "decoded",
            Self::Updated => "updated",
            Self::Messages => "messages",
            Self::ToolBytes => "tool bytes",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Decoded => Self::Updated,
            Self::Updated => Self::Messages,
            Self::Messages => Self::ToolBytes,
            Self::ToolBytes => Self::Decoded,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Visibility {
    All,
    Active,
    Archived,
}

impl Visibility {
    fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Active => "active",
            Self::Archived => "archived",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::All => Self::Active,
            Self::Active => Self::Archived,
            Self::Archived => Self::All,
        }
    }

    fn matches(self, archived: bool) -> bool {
        match self {
            Self::All => true,
            Self::Active => !archived,
            Self::Archived => archived,
        }
    }
}

#[derive(Debug, Clone)]
enum Status {
    Info(String),
    Error(String),
}

struct PendingRecovery {
    record: ThreadRecord,
    prepared: PreparedRecovery,
    summary: String,
    title: String,
    summary_path: PathBuf,
}

struct PendingDelete {
    sidebar: Option<SidebarThread>,
    root_id: Option<String>,
    title: String,
    thread_rows: usize,
    sidebar_rows: usize,
    compressed_bytes: u64,
    decoded_bytes: u64,
}

struct ImportPicker {
    entries: Vec<BundleEntry>,
    selected: usize,
}

struct PendingImport {
    entry: BundleEntry,
}

struct App {
    threads_db: PathBuf,
    zed_db: PathBuf,
    summary_dir: PathBuf,
    archive_dir: PathBuf,
    rows: Vec<ThreadRow>,
    sidebar_total: usize,
    native_total: usize,
    filter: String,
    filter_input: String,
    filter_before_edit: String,
    mode: Mode,
    sort: Sort,
    visibility: Visibility,
    selected: usize,
    filtered: Vec<usize>,
    table_state: TableState,
    status: Option<Status>,
    pending_recovery: Option<PendingRecovery>,
    pending_delete: Option<PendingDelete>,
    import_picker: Option<ImportPicker>,
    pending_import: Option<PendingImport>,
    summary_path: Option<PathBuf>,
}

impl App {
    fn new(
        threads_db: PathBuf,
        zed_db: PathBuf,
        summary_dir: PathBuf,
        archive_dir: PathBuf,
        library: Library,
    ) -> Self {
        let mut app = Self {
            threads_db,
            zed_db,
            summary_dir,
            archive_dir,
            rows: library.rows,
            sidebar_total: library.sidebar_total,
            native_total: library.native_total,
            filter: String::new(),
            filter_input: String::new(),
            filter_before_edit: String::new(),
            mode: Mode::Browse,
            sort: Sort::Decoded,
            visibility: Visibility::All,
            selected: 0,
            filtered: Vec::new(),
            table_state: TableState::default(),
            status: None,
            pending_recovery: None,
            pending_delete: None,
            import_picker: None,
            pending_import: None,
            summary_path: None,
        };
        app.refresh_filtered();
        app
    }

    fn selected_index(&self) -> Option<usize> {
        self.filtered.get(self.selected).copied()
    }

    fn selected_row(&self) -> Option<&ThreadRow> {
        self.selected_index().map(|index| &self.rows[index])
    }

    fn archived_count(&self) -> usize {
        self.rows.iter().filter(|row| row.archived()).count()
    }

    fn refresh_filtered(&mut self) {
        let needle = self.filter.to_lowercase();
        let mut filtered: Vec<usize> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                self.visibility.matches(row.archived()) && matches_filter(row, &needle)
            })
            .map(|(index, _)| index)
            .collect();
        filtered
            .sort_by(|&left, &right| compare_rows(&self.rows[left], &self.rows[right], self.sort));
        self.filtered = filtered;
        self.clamp_selection();
    }

    fn refresh_with_focus(&mut self, focus: Option<String>) {
        self.refresh_filtered();
        if let Some(key) = focus {
            if let Some(position) = self
                .filtered
                .iter()
                .position(|&index| self.rows[index].key() == key)
            {
                self.selected = position;
            }
        }
        self.clamp_selection();
    }

    fn clamp_selection(&mut self) {
        if self.filtered.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len() - 1;
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.filtered.is_empty() {
            self.selected = 0;
            return;
        }
        let last = self.filtered.len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, last) as usize;
    }

    fn reload(&mut self, focus: Option<String>) {
        match load_library(&self.threads_db, &self.zed_db) {
            Ok(library) => {
                self.rows = library.rows;
                self.sidebar_total = library.sidebar_total;
                self.native_total = library.native_total;
                self.refresh_with_focus(focus);
            }
            Err(error) => {
                self.status = Some(Status::Error(format!("reload failed: {error}")));
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Action {
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
            return Action::Quit;
        }

        match self.mode {
            Mode::Filter => return self.handle_filter_key(key),
            Mode::Help => {
                self.mode = Mode::Browse;
                return Action::Continue;
            }
            Mode::ImportPicker => return self.handle_import_picker_key(key),
            Mode::ConfirmRecovery | Mode::ConfirmDelete | Mode::ConfirmImport => {
                return self.handle_confirm_key(key);
            }
            Mode::Browse => {}
        }

        match key.code {
            KeyCode::Char('q') => Action::Quit,
            KeyCode::Esc => {
                if self.filter.is_empty() {
                    Action::Quit
                } else {
                    self.filter.clear();
                    self.refresh_filtered();
                    Action::Continue
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection(1);
                Action::Continue
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection(-1);
                Action::Continue
            }
            KeyCode::PageDown => {
                self.move_selection(10);
                Action::Continue
            }
            KeyCode::PageUp => {
                self.move_selection(-10);
                Action::Continue
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.selected = 0;
                Action::Continue
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.selected = self.filtered.len().saturating_sub(1);
                Action::Continue
            }
            KeyCode::Char('/') => {
                self.mode = Mode::Filter;
                self.filter_before_edit = self.filter.clone();
                self.filter_input = self.filter.clone();
                Action::Continue
            }
            KeyCode::Char('s') => {
                let focus = self.selected_row().map(ThreadRow::key);
                self.sort = self.sort.next();
                self.refresh_with_focus(focus);
                Action::Continue
            }
            KeyCode::Char('v') => {
                let focus = self.selected_row().map(ThreadRow::key);
                self.visibility = self.visibility.next();
                self.refresh_with_focus(focus);
                Action::Continue
            }
            KeyCode::Char('a') => {
                self.toggle_archive();
                Action::Continue
            }
            KeyCode::Char('e') => {
                self.export_selected();
                Action::Continue
            }
            KeyCode::Char('d') => {
                self.start_delete();
                Action::Continue
            }
            KeyCode::Char('i') => {
                self.open_import_picker();
                Action::Continue
            }
            KeyCode::Char('r') => self.start_recovery(),
            KeyCode::Char('R') => Action::Reload,
            KeyCode::Char('?') => {
                self.mode = Mode::Help;
                Action::Continue
            }
            _ => Action::Continue,
        }
    }

    fn handle_filter_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => {
                self.filter = self.filter_before_edit.clone();
                self.filter_input = self.filter.clone();
                self.mode = Mode::Browse;
                self.refresh_filtered();
            }
            KeyCode::Enter => {
                self.filter = self.filter_input.clone();
                self.mode = Mode::Browse;
                self.refresh_filtered();
            }
            KeyCode::Backspace => {
                self.filter_input.pop();
                self.filter = self.filter_input.clone();
                self.refresh_filtered();
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.filter_input.push(character);
                self.filter = self.filter_input.clone();
                self.refresh_filtered();
            }
            KeyCode::Up => self.move_selection(-1),
            KeyCode::Down => self.move_selection(1),
            KeyCode::PageUp => self.move_selection(-10),
            KeyCode::PageDown => self.move_selection(10),
            _ => {}
        }
        Action::Continue
    }

    fn handle_import_picker_key(&mut self, key: KeyEvent) -> Action {
        let Some(picker) = self.import_picker.as_mut() else {
            self.mode = Mode::Browse;
            return Action::Continue;
        };

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.import_picker = None;
                self.mode = Mode::Browse;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if !picker.entries.is_empty() {
                    picker.selected = (picker.selected + 1).min(picker.entries.len() - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                picker.selected = picker.selected.saturating_sub(1);
            }
            KeyCode::Enter => {
                let Some(entry) = picker.entries.get(picker.selected).cloned() else {
                    return Action::Continue;
                };
                if let Some(error) = &entry.error {
                    self.status = Some(Status::Error(error.clone()));
                    return Action::Continue;
                }
                self.pending_import = Some(PendingImport { entry });
                self.mode = Mode::ConfirmImport;
            }
            _ => {}
        }
        Action::Continue
    }

    fn handle_confirm_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => match self.mode {
                Mode::ConfirmRecovery => self.apply_recovery(),
                Mode::ConfirmDelete => self.apply_delete(),
                Mode::ConfirmImport => self.apply_import(),
                _ => {}
            },
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.pending_recovery = None;
                self.pending_delete = None;
                self.pending_import = None;
                self.import_picker = None;
                self.mode = Mode::Browse;
            }
            _ => {}
        }
        Action::Continue
    }

    fn toggle_archive(&mut self) {
        let Some(index) = self.selected_index() else {
            self.status = Some(Status::Error("no thread selected".to_owned()));
            return;
        };
        let Some(sidebar) = self.rows[index].sidebar.clone() else {
            self.status = Some(Status::Error(
                "this row has no sidebar entry; archive is not applicable".to_owned(),
            ));
            return;
        };
        let archived = !sidebar.archived;
        let title = sidebar.display_title().to_owned();
        let focus = self.rows[index].key();

        match metadata::set_archived(&self.zed_db, &sidebar.thread_id, archived, false) {
            Ok(backup) => {
                if let Some(row) = self.rows[index].sidebar.as_mut() {
                    row.archived = archived;
                }
                let action = if archived { "archived" } else { "unarchived" };
                self.status = Some(Status::Info(format!(
                    "{action} {title} | backup {}",
                    backup.display()
                )));
                self.refresh_with_focus(Some(focus));
            }
            Err(error) => {
                self.status = Some(Status::Error(format!("archive failed: {error}")));
            }
        }
    }

    fn export_selected(&mut self) {
        let Some(row) = self.selected_row() else {
            self.status = Some(Status::Error("no thread selected".to_owned()));
            return;
        };
        let sidebar = row.sidebar.clone();
        let root_id = row.audit.as_ref().map(|audit| audit.id.clone());
        let metadata_only = row.audit.is_none();
        let title = row.title().to_owned();

        match archive::export_thread(
            &self.threads_db,
            &self.zed_db,
            &self.archive_dir,
            sidebar.as_ref(),
            root_id.as_deref(),
        ) {
            Ok(outcome) => {
                let note = if metadata_only {
                    " (sidebar metadata only)"
                } else {
                    ""
                };
                self.status = Some(Status::Info(format!(
                    "exported {title}{note}: {} thread(s), {} sidebar row(s) to {}",
                    outcome.threads,
                    outcome.sidebar_threads,
                    outcome.bundle_dir.display()
                )));
            }
            Err(error) => {
                self.status = Some(Status::Error(format!("export failed: {error}")));
            }
        }
    }

    fn start_delete(&mut self) {
        let Some(index) = self.selected_index() else {
            self.status = Some(Status::Error("no thread selected".to_owned()));
            return;
        };
        let row = &self.rows[index];
        let sidebar = row.sidebar.clone();
        let root_id = row.audit.as_ref().map(|audit| audit.id.clone());
        if sidebar.is_none() && root_id.is_none() {
            self.status = Some(Status::Error("nothing to delete".to_owned()));
            return;
        }

        let thread_ids = match root_id.as_deref() {
            Some(root_id) => match db::open_read_only(&self.threads_db)
                .and_then(|connection| db::thread_tree_ids(&connection, root_id))
            {
                Ok(ids) => ids,
                Err(error) => {
                    self.status = Some(Status::Error(format!("load thread tree: {error}")));
                    return;
                }
            },
            None => Vec::new(),
        };

        let sidebar_rows = match metadata::open_read_only(&self.zed_db)
            .and_then(|connection| metadata::list_sidebar_threads(&connection))
        {
            Ok(rows) => rows
                .iter()
                .filter(|candidate| {
                    sidebar
                        .as_ref()
                        .map(|selected| selected.thread_id == candidate.thread_id)
                        .unwrap_or(false)
                        || candidate
                            .session_id
                            .as_ref()
                            .map(|session| thread_ids.iter().any(|id| id == session))
                            .unwrap_or(false)
                })
                .count(),
            Err(error) => {
                self.status = Some(Status::Error(format!("load sidebar rows: {error}")));
                return;
            }
        };

        if thread_ids.is_empty() && sidebar_rows == 0 {
            self.status = Some(Status::Error("nothing to delete".to_owned()));
            return;
        }

        self.pending_delete = Some(PendingDelete {
            sidebar,
            root_id,
            title: row.title().to_owned(),
            thread_rows: thread_ids.len(),
            sidebar_rows,
            compressed_bytes: row.compressed_bytes(),
            decoded_bytes: row.decoded_bytes(),
        });
        self.mode = Mode::ConfirmDelete;
    }

    fn apply_delete(&mut self) {
        let Some(pending) = self.pending_delete.take() else {
            return;
        };
        match archive::delete_thread(
            &self.threads_db,
            &self.zed_db,
            pending.sidebar.as_ref(),
            pending.root_id.as_deref(),
            false,
        ) {
            Ok(outcome) => {
                self.status = Some(Status::Info(format!(
                    "deleted {}: {} thread row(s), {} sidebar row(s)",
                    pending.title, outcome.threads, outcome.sidebar_threads
                )));
                self.mode = Mode::Browse;
                self.reload(None);
            }
            Err(error) => {
                self.status = Some(Status::Error(format!("delete failed: {error}")));
                self.pending_delete = Some(pending);
            }
        }
    }

    fn open_import_picker(&mut self) {
        match archive::list_bundles(&self.archive_dir) {
            Ok(entries) if entries.is_empty() => {
                self.status = Some(Status::Info(format!(
                    "no bundles in {}",
                    self.archive_dir.display()
                )));
            }
            Ok(entries) => {
                self.import_picker = Some(ImportPicker {
                    entries,
                    selected: 0,
                });
                self.mode = Mode::ImportPicker;
            }
            Err(error) => {
                self.status = Some(Status::Error(format!("list bundles: {error}")));
            }
        }
    }

    fn apply_import(&mut self) {
        let Some(pending) = self.pending_import.take() else {
            return;
        };
        match archive::import_bundle(&self.threads_db, &self.zed_db, &pending.entry.path, false) {
            Ok(outcome) => {
                self.status = Some(Status::Info(format!(
                    "imported {}: {} thread row(s), {} sidebar row(s)",
                    pending.entry.title(),
                    outcome.threads,
                    outcome.sidebar_threads
                )));
                self.mode = Mode::Browse;
                self.import_picker = None;
                self.reload(None);
            }
            Err(error) => {
                self.status = Some(Status::Error(format!("import failed: {error}")));
                self.pending_import = Some(pending);
            }
        }
    }

    fn start_recovery(&mut self) -> Action {
        let Some(index) = self.selected_index() else {
            self.status = Some(Status::Error("no thread selected".to_owned()));
            return Action::Continue;
        };
        let row = &self.rows[index];
        let Some(audit) = row.audit.as_ref() else {
            self.status = Some(Status::Error(
                "recovery needs a threads.db record; this is a sidebar-only thread".to_owned(),
            ));
            return Action::Continue;
        };
        if audit.error.is_some() {
            self.status = Some(Status::Error(
                "this thread could not be decoded; refusing to recover it".to_owned(),
            ));
            return Action::Continue;
        }

        let thread_id = audit.id.clone();
        let title = row.title().to_owned();
        let updated_at = row.updated_at().to_owned();
        let project_paths: Vec<String> =
            row.project_paths().into_iter().map(str::to_owned).collect();
        let path = summary_path(&self.summary_dir, &title, &thread_id);

        if let Err(error) =
            ensure_summary_template(&path, &title, &thread_id, &updated_at, &project_paths)
        {
            self.status = Some(Status::Error(format!("summary: {error}")));
            return Action::Continue;
        }

        self.summary_path = Some(path);
        Action::EditSummary
    }

    fn edit_summary(&self) -> io::Result<()> {
        let Some(path) = &self.summary_path else {
            return Err(io::Error::other("no summary file prepared"));
        };
        open_editor(path)
    }

    fn after_edit(&mut self, result: io::Result<()>) {
        if let Err(error) = result {
            self.status = Some(Status::Error(format!("editor: {error}")));
            return;
        }

        let Some(path) = self.summary_path.clone() else {
            self.status = Some(Status::Error("no summary file prepared".to_owned()));
            return;
        };
        let summary = match std::fs::read_to_string(&path) {
            Ok(summary) => summary,
            Err(error) => {
                self.status = Some(Status::Error(format!(
                    "reading {}: {error}",
                    path.display()
                )));
                return;
            }
        };
        if !summary_is_ready(&summary) {
            self.status = Some(Status::Error(format!(
                "{} still contains the placeholder",
                path.display()
            )));
            return;
        }

        let Some(row) = self.selected_row() else {
            self.status = Some(Status::Error("no thread selected".to_owned()));
            return;
        };
        let Some(audit) = row.audit.as_ref() else {
            self.status = Some(Status::Error(
                "recovery needs a threads.db record".to_owned(),
            ));
            return;
        };
        let thread_id = audit.id.clone();
        let title = row.title().to_owned();

        let connection = match db::open_read_only(&self.threads_db) {
            Ok(connection) => connection,
            Err(error) => {
                self.status = Some(Status::Error(format!("open thread store: {error}")));
                return;
            }
        };
        let record = match db::load_record(&connection, &thread_id) {
            Ok(record) => record,
            Err(error) => {
                self.status = Some(Status::Error(format!("load thread: {error}")));
                return;
            }
        };
        drop(connection);

        let prepared = match recover::prepare_recovery(&record, &summary, None, false) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.status = Some(Status::Error(format!("prepare recovery: {error}")));
                return;
            }
        };

        self.pending_recovery = Some(PendingRecovery {
            record,
            prepared,
            summary,
            title,
            summary_path: path,
        });
        self.mode = Mode::ConfirmRecovery;
        self.status = None;
    }

    fn apply_recovery(&mut self) {
        let Some(pending) = self.pending_recovery.take() else {
            return;
        };

        let options = RecoverOptions {
            thread_id: pending.record.header.id.clone(),
            summary: pending.summary.clone(),
            title: None,
            fork: false,
            dry_run: false,
            backup: None,
            allow_running: false,
        };

        match recover::write_recovery(
            &self.threads_db,
            &pending.record,
            &pending.prepared,
            &options,
        ) {
            Ok(outcome) => {
                let id = outcome.thread_id.clone();
                let backup = outcome.backup.display().to_string();
                self.status = Some(Status::Info(format!("recovered {id} | backup {backup}")));
                self.mode = Mode::Browse;
                self.reload(Some(format!("thread:{id}")));
            }
            Err(error) => {
                self.status = Some(Status::Error(format!("recovery failed: {error}")));
                self.pending_recovery = Some(pending);
            }
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let chunks = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(6),
            Constraint::Length(3),
        ])
        .split(area);

        self.draw_header(frame, chunks[0]);
        self.draw_body(frame, chunks[1]);
        self.draw_footer(frame, chunks[2]);

        match self.mode {
            Mode::Help => self.draw_help(frame, area),
            Mode::ConfirmRecovery => self.draw_recovery_confirm(frame, area),
            Mode::ConfirmDelete => self.draw_delete_confirm(frame, area),
            Mode::ImportPicker => self.draw_import_picker(frame, area),
            Mode::ConfirmImport => self.draw_import_confirm(frame, area),
            _ => {}
        }
    }

    fn draw_header(&self, frame: &mut Frame, area: Rect) {
        let title = format!("zed-thread-tui - {}", self.threads_db.display());
        let archived = self.archived_count();
        let shown = self.filtered.len();
        let summary = format!(
            "{} sidebar | {} native | {} archived | {shown} shown",
            self.sidebar_total, self.native_total, archived
        );
        let controls = format!(
            "sort: {} | view: {} | filter: {}",
            self.sort.label(),
            self.visibility.label(),
            if self.filter.is_empty() {
                "(none)"
            } else {
                self.filter.as_str()
            }
        );
        let lines = vec![
            Line::from(Span::styled(
                title,
                Style::new().add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                format!("{summary} | {controls}"),
                Style::new().fg(Color::Gray),
            )),
        ];
        frame.render_widget(Paragraph::new(lines), area);
    }

    fn draw_body(&mut self, frame: &mut Frame, area: Rect) {
        if area.width >= 110 {
            let columns =
                Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)])
                    .split(area);
            self.draw_table(frame, columns[0]);
            self.draw_detail(frame, columns[1]);
        } else {
            let rows = Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)])
                .split(area);
            self.draw_table(frame, rows[0]);
            self.draw_detail(frame, rows[1]);
        }
    }

    fn draw_table(&mut self, frame: &mut Frame, area: Rect) {
        let header = Row::new(vec![
            "Updated", "Age", "Msgs", "Tools", "Output", "Comp", "A", "Title",
        ])
        .style(Style::new().add_modifier(Modifier::BOLD));

        let rows: Vec<Row> = self
            .filtered
            .iter()
            .map(|&index| {
                let row = &self.rows[index];
                let cells = vec![
                    Cell::from(short_date(row.updated_at())),
                    Cell::from(audit::age(row.age_seconds())),
                    Cell::from(count(row.messages())),
                    Cell::from(count(row.tool_calls())),
                    Cell::from(format_bytes_option(row.tool_output_bytes())),
                    Cell::from(count(row.compactions())),
                    Cell::from(if row.archived() { "A" } else { "" }),
                    Cell::from(row.title().to_owned()),
                ];
                let style = if row.error().is_some() {
                    Style::new().fg(Color::Red)
                } else if row.archived() || row.is_subagent() {
                    Style::new().fg(Color::DarkGray)
                } else {
                    Style::new()
                };
                Row::new(cells).style(style)
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Length(11),
                Constraint::Length(5),
                Constraint::Length(5),
                Constraint::Length(6),
                Constraint::Length(8),
                Constraint::Length(4),
                Constraint::Length(2),
                Constraint::Min(12),
            ],
        )
        .header(header)
        .block(Block::bordered().title(format!(" Threads ({}) ", self.filtered.len())))
        .column_spacing(1)
        .row_highlight_style(
            Style::new()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );

        if self.filtered.is_empty() {
            self.table_state.select(None);
        } else {
            self.table_state.select(Some(self.selected));
        }
        frame.render_stateful_widget(table, area, &mut self.table_state);
    }

    fn draw_detail(&self, frame: &mut Frame, area: Rect) {
        let block = Block::bordered().title(" Thread ");
        let Some(row) = self.selected_row() else {
            frame.render_widget(Paragraph::new("No thread selected").block(block), area);
            return;
        };

        let mut lines = vec![
            line_pair("Title", row.title().to_owned()),
            line_pair(
                "Updated",
                format!("{} ({})", row.updated_at(), audit::age(row.age_seconds())),
            ),
            line_pair("Archived", if row.archived() { "yes" } else { "no" }),
            line_pair("Session", row.session_id().unwrap_or("-")),
            line_pair("Agent", row.agent_id().unwrap_or("-")),
        ];

        if let Some(sidebar) = &row.sidebar {
            lines.push(line_pair("Sidebar", sidebar.thread_id_hex()));
            lines.push(line_pair(
                "Created",
                sidebar.created_at.as_deref().unwrap_or("-"),
            ));
            lines.push(line_pair(
                "Interacted",
                sidebar.interacted_at.as_deref().unwrap_or("-"),
            ));
            if !sidebar.main_worktree_paths().is_empty() {
                lines.push(line_pair("Main worktrees", ""));
                for path in sidebar.main_worktree_paths() {
                    lines.push(Line::from(format!("  {path}")));
                }
            }
        } else {
            lines.push(Line::from(Span::styled(
                "No sidebar entry (threads.db-only row)",
                Style::new().fg(Color::Yellow),
            )));
        }

        if let Some(audit) = &row.audit {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "threads.db",
                Style::new().add_modifier(Modifier::BOLD),
            )));
            lines.push(line_pair("Model", audit.model.as_deref().unwrap_or("-")));
            lines.push(line_pair(
                "Messages",
                format!(
                    "{} (user {}, agent {})",
                    count(audit.messages),
                    count(audit.user_messages),
                    count(audit.agent_messages)
                ),
            ));
            lines.push(line_pair(
                "Tools",
                format!(
                    "{} calls, {} results, {} failed",
                    count(audit.tool_calls),
                    count(audit.tool_results),
                    count(audit.failed_tool_results)
                ),
            ));
            lines.push(line_pair(
                "Output",
                format!(
                    "{} total, {} largest",
                    format_bytes_option(audit.tool_output_bytes),
                    format_bytes_option(audit.largest_tool_output_bytes)
                ),
            ));
            if let (Some(tool), Some(message)) = (
                audit.largest_tool_output_tool.as_deref(),
                audit.largest_tool_output_message,
            ) {
                lines.push(line_pair("Largest", format!("{tool} at message {message}")));
            }
            lines.push(line_pair(
                "Compaction",
                format!(
                    "{} total, {} since",
                    count(audit.compactions),
                    count(audit.messages_since_compaction)
                ),
            ));
            lines.push(line_pair(
                "Size",
                format!(
                    "{} decoded / {} compressed",
                    format_bytes_option(audit.decoded_bytes),
                    audit::format_bytes(audit.compressed_bytes)
                ),
            ));
            if audit.subagent {
                lines.push(line_pair("Subagent", "yes"));
            }
            if let Some(error) = &audit.error {
                lines.push(Line::from(Span::styled(
                    format!("Error: {error}"),
                    Style::new().fg(Color::Red),
                )));
            }
        } else {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "No threads.db record; the conversation is owned by the agent.",
                Style::new().fg(Color::Yellow),
            )));
        }

        lines.push(Line::from(""));
        lines.push(line_pair("Projects", ""));
        if row.project_paths().is_empty() {
            lines.push(Line::from("  (none recorded)"));
        } else {
            for path in row.project_paths() {
                lines.push(Line::from(format!("  {path}")));
            }
        }

        frame.render_widget(
            Paragraph::new(lines)
                .block(block)
                .wrap(Wrap { trim: false }),
            area,
        );
    }

    fn draw_footer(&self, frame: &mut Frame, area: Rect) {
        let keys = vec![
            Line::from(Span::styled(
                "q quit | j/k move | / filter | s sort | v view | ? help",
                Style::new().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                "a archive | e export | d delete | i import | r recover | R reload",
                Style::new().fg(Color::DarkGray),
            )),
        ];
        let status = match (&self.status, self.mode) {
            (_, Mode::Filter) => Line::from(vec![
                Span::styled("Filter: ", Style::new().fg(Color::Yellow)),
                Span::raw(self.filter_input.clone()),
            ]),
            (Some(Status::Info(message)), _) => {
                Line::from(Span::styled(message.clone(), Style::new().fg(Color::Green)))
            }
            (Some(Status::Error(message)), _) => {
                Line::from(Span::styled(message.clone(), Style::new().fg(Color::Red)))
            }
            (None, _) => Line::from(""),
        };
        let mut lines = keys;
        lines.push(status);
        frame.render_widget(Paragraph::new(lines), area);
    }

    fn draw_help(&self, frame: &mut Frame, area: Rect) {
        let popup = centered_rect(78, 72, area);
        let lines = vec![
            Line::from(Span::styled(
                "Keys",
                Style::new().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("q / Esc       quit (Esc clears a filter first)"),
            Line::from("j / k         move down / up"),
            Line::from("g / G         first / last"),
            Line::from("PgDn / PgUp   page down / up"),
            Line::from("/             filter by title, id, agent, model or project path"),
            Line::from("s             cycle sort: decoded, updated, messages, tool bytes"),
            Line::from("v             view all / active / archived"),
            Line::from("a             archive or unarchive the selected thread"),
            Line::from("e             export the selected thread to a bundle"),
            Line::from("d             delete the selected thread only"),
            Line::from("i             import a bundle from the archive directory"),
            Line::from("r             recovery for a native threads.db thread"),
            Line::from("R             reload both stores"),
            Line::from("?             close this help"),
        ];
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::bordered().title(" Help "))
                .wrap(Wrap { trim: false }),
            popup,
        );
    }

    fn draw_recovery_confirm(&self, frame: &mut Frame, area: Rect) {
        let Some(pending) = &self.pending_recovery else {
            return;
        };
        let plan = &pending.prepared.plan;
        let popup = centered_rect(82, 80, area);
        let mut lines = vec![
            Line::from(Span::styled(
                "Apply this compaction summary?",
                Style::new().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            line_pair("Thread", pending.title.clone()),
            line_pair("ID", pending.record.header.id.clone()),
            line_pair("Mode", plan.mode.to_string()),
            Line::from(format!(
                "Insert at message {} of {}",
                plan.insertion_index, plan.messages_before
            )),
            line_pair("Summary", format!("{} bytes", plan.summary_bytes)),
            line_pair("File", pending.summary_path.display().to_string()),
            Line::from(""),
            Line::from(Span::styled(
                "Summary preview:",
                Style::new().add_modifier(Modifier::BOLD),
            )),
        ];
        for line in pending.summary.lines().take(14) {
            lines.push(Line::from(format!("  {line}")));
        }
        if pending.summary.lines().count() > 14 {
            lines.push(Line::from("  ..."));
        }
        lines.push(Line::from(""));
        if let Some(Status::Error(error)) = &self.status {
            lines.push(Line::from(Span::styled(
                error.clone(),
                Style::new().fg(Color::Red),
            )));
        }
        lines.push(Line::from(Span::styled(
            "y apply   n cancel",
            Style::new().fg(Color::Yellow),
        )));

        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::bordered().title(" Confirm recovery "))
                .wrap(Wrap { trim: false }),
            popup,
        );
    }

    fn draw_delete_confirm(&self, frame: &mut Frame, area: Rect) {
        let Some(pending) = &self.pending_delete else {
            return;
        };
        let popup = centered_rect(78, 55, area);
        let mut lines = vec![
            Line::from(Span::styled(
                "Delete this thread?",
                Style::new().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            line_pair("Title", pending.title.clone()),
            line_pair(
                "Threads",
                format!("{} row(s) from threads.db", pending.thread_rows),
            ),
            line_pair(
                "Sidebar",
                format!("{} row(s) from db.sqlite", pending.sidebar_rows),
            ),
            line_pair(
                "Size",
                format!(
                    "{} decoded / {} compressed",
                    audit::format_bytes(pending.decoded_bytes),
                    audit::format_bytes(pending.compressed_bytes)
                ),
            ),
            Line::from(""),
        ];
        if pending.root_id.is_none() {
            lines.push(Line::from(Span::styled(
                "Sidebar metadata only; the agent's conversation is not deleted.",
                Style::new().fg(Color::Yellow),
            )));
        }
        lines.push(Line::from(Span::styled(
            "This is not undoable. Press e first if you want a bundle.",
            Style::new().fg(Color::Yellow),
        )));
        lines.push(Line::from(""));
        if let Some(Status::Error(error)) = &self.status {
            lines.push(Line::from(Span::styled(
                error.clone(),
                Style::new().fg(Color::Red),
            )));
        }
        lines.push(Line::from(Span::styled(
            "y delete   n cancel",
            Style::new().fg(Color::Yellow),
        )));

        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::bordered().title(" Confirm delete "))
                .wrap(Wrap { trim: false }),
            popup,
        );
    }

    fn draw_import_picker(&self, frame: &mut Frame, area: Rect) {
        let Some(picker) = &self.import_picker else {
            return;
        };
        let popup = centered_rect(86, 74, area);
        let mut lines = vec![
            Line::from(Span::styled(
                format!("Bundles in {}", self.archive_dir.display()),
                Style::new().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
        ];
        if picker.entries.is_empty() {
            lines.push(Line::from("No bundles found."));
        } else {
            let start = picker.selected.saturating_sub(7);
            for (index, entry) in picker.entries.iter().enumerate().skip(start).take(15) {
                let selected = index == picker.selected;
                let marker = if selected { ">" } else { " " };
                let style = if entry.error.is_some() {
                    Style::new().fg(Color::Red)
                } else if selected {
                    Style::new().fg(Color::Yellow)
                } else {
                    Style::new()
                };
                let detail = match &entry.error {
                    Some(error) => format!("invalid: {error}"),
                    None => format!(
                        "{} | {} thread(s), {} sidebar row(s)",
                        entry.exported_at(),
                        entry.thread_count(),
                        entry.sidebar_count()
                    ),
                };
                lines.push(Line::from(Span::styled(
                    format!("{marker} {}", entry.title()),
                    style,
                )));
                lines.push(Line::from(format!("  {detail}")));
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "j/k move   Enter import   Esc close",
            Style::new().fg(Color::Yellow),
        )));

        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::bordered().title(" Import bundle "))
                .wrap(Wrap { trim: false }),
            popup,
        );
    }

    fn draw_import_confirm(&self, frame: &mut Frame, area: Rect) {
        let Some(pending) = &self.pending_import else {
            return;
        };
        let popup = centered_rect(78, 55, area);
        let mut lines = vec![
            Line::from(Span::styled(
                "Import this bundle?",
                Style::new().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            line_pair("Title", pending.entry.title().to_owned()),
            line_pair("Exported", pending.entry.exported_at().to_owned()),
            line_pair(
                "Threads",
                format!("{} row(s)", pending.entry.thread_count()),
            ),
            line_pair(
                "Sidebar",
                format!("{} row(s)", pending.entry.sidebar_count()),
            ),
            line_pair("Path", pending.entry.path.display().to_string()),
            Line::from(""),
            Line::from(Span::styled(
                "Existing ids are never overwritten.",
                Style::new().fg(Color::Yellow),
            )),
            Line::from(""),
        ];
        if let Some(Status::Error(error)) = &self.status {
            lines.push(Line::from(Span::styled(
                error.clone(),
                Style::new().fg(Color::Red),
            )));
        }
        lines.push(Line::from(Span::styled(
            "y import   n cancel",
            Style::new().fg(Color::Yellow),
        )));

        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::bordered().title(" Confirm import "))
                .wrap(Wrap { trim: false }),
            popup,
        );
    }
}

fn matches_filter(row: &ThreadRow, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let mut haystack = format!(
        "{} {} {} {}",
        row.title(),
        row.session_id().unwrap_or(""),
        row.agent_id().unwrap_or(""),
        row.model().unwrap_or("")
    );
    if let Some(sidebar) = &row.sidebar {
        haystack.push(' ');
        haystack.push_str(&sidebar.thread_id_hex());
    }
    for path in row.project_paths() {
        haystack.push(' ');
        haystack.push_str(path);
    }
    haystack.to_lowercase().contains(needle)
}

fn compare_rows(left: &ThreadRow, right: &ThreadRow, sort: Sort) -> Ordering {
    let primary = match sort {
        Sort::Decoded => right.decoded_bytes().cmp(&left.decoded_bytes()),
        Sort::Updated => right.updated_at().cmp(left.updated_at()),
        Sort::Messages => right
            .messages()
            .unwrap_or(0)
            .cmp(&left.messages().unwrap_or(0)),
        Sort::ToolBytes => right
            .tool_output_bytes()
            .unwrap_or(0)
            .cmp(&left.tool_output_bytes().unwrap_or(0)),
    };
    primary.then_with(|| right.updated_at().cmp(left.updated_at()))
}

fn short_date(value: &str) -> String {
    value
        .get(5..16)
        .map_or_else(|| value.to_owned(), |slice| slice.replace('T', " "))
}

fn count(value: Option<usize>) -> String {
    value.map_or_else(|| "-".to_owned(), |value| value.to_string())
}

fn format_bytes_option(value: Option<u64>) -> String {
    value.map_or_else(|| "-".to_owned(), audit::format_bytes)
}

fn line_pair(label: &str, value: impl Into<String>) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<14}"), Style::new().fg(Color::Gray)),
        Span::raw(value.into()),
    ])
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);
    let horizontal = Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1]);
    horizontal[1]
}

fn summary_path(dir: &Path, title: &str, id: &str) -> PathBuf {
    let slug = slugify(title);
    let short_id: String = id
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .take(8)
        .collect();
    dir.join(format!("zed-thread-recover-{slug}-{short_id}.md"))
}

fn ensure_summary_template(
    path: &Path,
    title: &str,
    id: &str,
    updated_at: &str,
    project_paths: &[String],
) -> io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }

    let mut template = format!(
        "{SUMMARY_PLACEHOLDER}\n\nThread: {title}\nID: {id}\nUpdated: {updated_at}\nProjects:\n"
    );
    for project_path in project_paths {
        template.push_str(&format!("- {project_path}\n"));
    }
    std::fs::write(path, template)
}

fn summary_is_ready(summary: &str) -> bool {
    let trimmed = summary.trim();
    !trimmed.is_empty() && !trimmed.contains(SUMMARY_PLACEHOLDER)
}

fn open_editor(path: &Path) -> io::Result<()> {
    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "vi".to_owned());
    let status = Command::new(&editor).arg(path).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "editor {editor:?} exited with {status}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::slugify;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn sample_audit(id: &str, title: &str, updated_at: &str, decoded: u64) -> ThreadAudit {
        ThreadAudit {
            id: id.to_owned(),
            title: title.to_owned(),
            summary: title.to_owned(),
            updated_at: updated_at.to_owned(),
            age_seconds: Some(60),
            compressed_bytes: decoded / 2,
            decoded_bytes: Some(decoded),
            messages: Some(4),
            user_messages: Some(1),
            agent_messages: Some(3),
            tool_calls: Some(2),
            tool_results: Some(2),
            failed_tool_results: Some(0),
            tool_output_bytes: Some(1_000),
            largest_tool_output_bytes: Some(500),
            largest_tool_output_tool: Some("read_file".to_owned()),
            largest_tool_output_message: Some(3),
            compactions: Some(0),
            messages_since_compaction: Some(4),
            model: Some("test/model".to_owned()),
            project_paths: vec!["/repo/alpha".to_owned()],
            subagent: false,
            error: None,
        }
    }

    fn sample_sidebar(
        id: u8,
        session: Option<&str>,
        title: &str,
        updated_at: &str,
        archived: bool,
    ) -> SidebarThread {
        SidebarThread {
            thread_id: vec![id; 16],
            session_id: session.map(str::to_owned),
            agent_id: Some("native_agent".to_owned()),
            title: title.to_owned(),
            updated_at: updated_at.to_owned(),
            created_at: Some("2026-10-01T00:00:00.000000+00:00".to_owned()),
            folder_paths: Some("/repo/alpha".to_owned()),
            folder_paths_order: None,
            archived,
            main_worktree_paths: None,
            main_worktree_paths_order: None,
            remote_connection: None,
            interacted_at: None,
            title_override: None,
        }
    }

    fn sample_library() -> Library {
        Library {
            rows: vec![
                ThreadRow {
                    sidebar: Some(sample_sidebar(
                        1,
                        Some("id-alpha"),
                        "Alpha thread",
                        "2026-10-17T04:00:00.000000+00:00",
                        false,
                    )),
                    audit: Some(sample_audit(
                        "id-alpha",
                        "Alpha thread",
                        "2026-10-17T04:00:00.000000+00:00",
                        1_000,
                    )),
                },
                ThreadRow {
                    sidebar: Some(sample_sidebar(
                        2,
                        Some("id-beta"),
                        "Beta thread",
                        "2026-10-17T05:00:00.000000+00:00",
                        true,
                    )),
                    audit: Some(sample_audit(
                        "id-beta",
                        "Beta thread",
                        "2026-10-17T05:00:00.000000+00:00",
                        2_000,
                    )),
                },
                ThreadRow {
                    sidebar: Some(sample_sidebar(
                        3,
                        Some("claude-1"),
                        "External thread",
                        "2026-10-17T05:00:00.000000+00:00",
                        false,
                    )),
                    audit: None,
                },
            ],
            sidebar_total: 3,
            native_total: 2,
        }
    }

    fn app() -> App {
        App::new(
            PathBuf::from("/tmp/threads.db"),
            PathBuf::from("/tmp/db.sqlite"),
            PathBuf::from(".tmp"),
            PathBuf::from("/tmp/archives"),
            sample_library(),
        )
    }

    #[test]
    fn default_sort_is_largest_decoded_first() {
        assert_eq!(app().filtered, vec![1, 0, 2]);
    }

    #[test]
    fn filter_matches_title_and_external_session() {
        let mut app = app();
        app.filter = "external".to_owned();
        app.refresh_filtered();
        assert_eq!(app.filtered, vec![2]);

        app.filter = "id-beta".to_owned();
        app.refresh_filtered();
        assert_eq!(app.filtered, vec![1]);
    }

    #[test]
    fn visibility_hides_archived_rows() {
        let mut app = app();
        app.visibility = Visibility::Active;
        app.refresh_filtered();
        assert_eq!(app.filtered, vec![0, 2]);

        app.visibility = Visibility::Archived;
        app.refresh_filtered();
        assert_eq!(app.filtered, vec![1]);
    }

    #[test]
    fn sorting_by_updated_puts_the_newest_first() {
        let mut app = app();
        app.sort = Sort::Updated;
        app.refresh_filtered();
        assert_eq!(app.filtered, vec![1, 2, 0]);
    }

    #[test]
    fn filter_input_accepts_text_and_escape_restores() {
        let mut app = app();
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        assert_eq!(app.filter, "be");

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.filter, "");
        assert_eq!(app.mode, Mode::Browse);
    }

    #[test]
    fn renders_the_table_and_detail_headlessly() {
        let mut app = app();
        let backend = TestBackend::new(140, 42);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();

        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Threads"));
        assert!(rendered.contains("Beta thread"));
        assert!(rendered.contains("External thread"));
        assert!(rendered.contains("test/model"));
    }

    #[test]
    fn placeholder_is_not_a_ready_summary() {
        let summary = format!("{SUMMARY_PLACEHOLDER}\n\nThread: x");
        assert!(!summary_is_ready(&summary));
        assert!(summary_is_ready("The task was to ..."));
    }

    #[test]
    fn slug_is_stable_and_ascii() {
        assert_eq!(
            slugify("Zed agent thread analyser recovery"),
            "zed-agent-thread-analyser-recovery"
        );
        assert_eq!(slugify("!!!"), "thread");
    }

    #[test]
    fn summary_path_uses_the_slug_and_short_id() {
        assert_eq!(
            summary_path(
                Path::new(".tmp"),
                "Zed agent thread analyser recovery",
                "51fc0d2c-3453-405e-a0e2-24deba7a04d1",
            ),
            PathBuf::from(".tmp/zed-thread-recover-zed-agent-thread-analyser-recovery-51fc0d2c.md")
        );
    }
}
