//! A decoded, budgeted snapshot of Zed agent threads for an external reader.
//!
//! `archive` exports the stored blobs unchanged so an import round trips
//! byte-for-byte. This module does the opposite: it decodes whole thread trees
//! to the JSON Zed stores and writes one file per thread beside a typed
//! manifest. Selection is by tree, so a snapshot never has a dangling
//! `parent_id`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, Utc};
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};

use crate::audit::format_bytes;
use crate::db::{self, ThreadHeader};
use crate::util::safe_id;
use crate::{Error, VERSION};

pub const SNAPSHOT_FORMAT_VERSION: u32 = 1;

/// `--max-mb` is decimal megabytes, as its name says.
pub const MB: u64 = 1_000_000;

#[derive(Debug, Clone)]
pub struct ExportOptions {
    /// Decoded thread bytes to include. `0` includes every tree.
    pub max_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotManifest {
    pub format_version: u32,
    pub tool_version: String,
    pub exported_at: String,
    pub source_database: String,
    pub selection: SnapshotSelection,
    pub threads: Vec<SnapshotThread>,
    pub skipped: Vec<SkippedTree>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotSelection {
    pub max_bytes: u64,
    pub included_bytes: u64,
    pub order: String,
    pub candidate_trees: usize,
    pub included_trees: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotThread {
    pub id: String,
    pub parent_id: Option<String>,
    pub root_id: String,
    pub title: String,
    pub summary: String,
    pub updated_at: String,
    pub created_at: Option<String>,
    pub model: Option<String>,
    pub project_paths: Vec<String>,
    pub messages: usize,
    pub version: Option<String>,
    pub decoded_bytes: u64,
    pub file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkippedTree {
    pub root_id: String,
    pub title: String,
    pub updated_at: String,
    pub decoded_bytes: u64,
    pub reason: SkipReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    OverBudget,
    Undecodable,
}

#[derive(Debug, Clone)]
pub struct ExportOutcome {
    pub output_dir: PathBuf,
    pub threads: usize,
    pub trees: usize,
    pub bytes: u64,
    pub skipped_over_budget: usize,
    pub skipped_undecodable: usize,
}

/// The parts of Zed's `DbThread` the manifest describes. Every other field is
/// ignored here and written through untouched in the thread file.
#[derive(Deserialize)]
struct SnapshotDocument {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    messages: Vec<IgnoredAny>,
    #[serde(default)]
    model: Option<SnapshotModel>,
}

#[derive(Deserialize)]
struct SnapshotModel {
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

impl SnapshotModel {
    fn display(&self) -> Option<String> {
        match (self.provider.as_deref(), self.model.as_deref()) {
            (Some(provider), Some(model)) => Some(format!("{provider}/{model}")),
            (Some(provider), None) => Some(provider.to_owned()),
            (None, Some(model)) => Some(model.to_owned()),
            (None, None) => None,
        }
    }
}

struct PreparedThread {
    header: ThreadHeader,
    decoded: Vec<u8>,
    document: SnapshotDocument,
}

/// Write decoded top-level thread trees, newest `updated_at` first, until the
/// next whole tree would exceed `options.max_bytes`.
pub fn export(
    db_path: &Path,
    output_dir: &Path,
    options: &ExportOptions,
) -> Result<ExportOutcome, Error> {
    if output_dir.exists() {
        return Err(Error::OutputExists(output_dir.to_path_buf()));
    }

    let connection = db::open_read_only(db_path)?;
    let headers = db::list_headers(&connection)?;
    if headers.is_empty() {
        return Err(Error::NothingToDo);
    }

    let by_id: HashMap<&str, &ThreadHeader> = headers
        .iter()
        .map(|header| (header.id.as_str(), header))
        .collect();
    let mut roots: Vec<&ThreadHeader> = headers
        .iter()
        .filter(|header| match header.parent_id.as_deref() {
            Some(parent) => !by_id.contains_key(parent),
            None => true,
        })
        .collect();
    roots.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.id.cmp(&right.id))
    });

    if let Err(error) = std::fs::create_dir_all(output_dir.join("threads")) {
        let _ = std::fs::remove_dir_all(output_dir);
        return Err(error.into());
    }

    match write_snapshot(&connection, db_path, &roots, output_dir, options) {
        Ok(outcome) => Ok(outcome),
        Err(error) => {
            let _ = std::fs::remove_dir_all(output_dir);
            Err(error)
        }
    }
}

fn write_snapshot(
    connection: &rusqlite::Connection,
    db_path: &Path,
    roots: &[&ThreadHeader],
    output_dir: &Path,
    options: &ExportOptions,
) -> Result<ExportOutcome, Error> {
    let mut threads = Vec::new();
    let mut skipped = Vec::new();
    let mut included_bytes = 0_u64;
    let mut included_trees = 0_usize;
    let mut smallest_over_budget: Option<(u64, String)> = None;
    let mut first_undecodable: Option<String> = None;

    for root in roots {
        let tree_ids = db::thread_tree_ids(connection, &root.id)?;
        let mut tree = Vec::with_capacity(tree_ids.len());
        let mut tree_bytes = 0_u64;
        let mut failure = None;

        for id in &tree_ids {
            let record = match db::load_record(connection, id) {
                Ok(record) => record,
                Err(error) => {
                    failure = Some(error.to_string());
                    break;
                }
            };
            let decoded = match db::decode_thread(&record.header.data_type, &record.data) {
                Ok(decoded) => decoded,
                Err(error) => {
                    failure = Some(format!("{}: {error}", record.header.id));
                    break;
                }
            };
            let document = match serde_json::from_slice::<SnapshotDocument>(&decoded) {
                Ok(document) => document,
                Err(error) => {
                    failure = Some(format!("{}: {error}", record.header.id));
                    break;
                }
            };
            tree_bytes = tree_bytes.saturating_add(decoded.len() as u64);
            tree.push(PreparedThread {
                header: record.header,
                decoded,
                document,
            });
        }

        if let Some(error) = failure {
            first_undecodable.get_or_insert_with(|| error.clone());
            skipped.push(SkippedTree {
                root_id: root.id.clone(),
                title: root.summary.clone(),
                updated_at: root.updated_at.clone(),
                decoded_bytes: tree_bytes,
                reason: SkipReason::Undecodable,
                error: Some(error),
            });
            continue;
        }

        if options.max_bytes > 0 && included_bytes.saturating_add(tree_bytes) > options.max_bytes {
            if smallest_over_budget
                .as_ref()
                .is_none_or(|(smallest, _)| tree_bytes < *smallest)
            {
                smallest_over_budget = Some((tree_bytes, root.summary.clone()));
            }
            skipped.push(SkippedTree {
                root_id: root.id.clone(),
                title: root.summary.clone(),
                updated_at: root.updated_at.clone(),
                decoded_bytes: tree_bytes,
                reason: SkipReason::OverBudget,
                error: None,
            });
            continue;
        }

        tree.sort_by(|left, right| {
            right
                .header
                .updated_at
                .cmp(&left.header.updated_at)
                .then_with(|| left.header.id.cmp(&right.header.id))
        });

        for thread in tree {
            if !safe_id(&thread.header.id) {
                return Err(Error::InvalidThread(format!(
                    "thread id {:?} is not a safe file name",
                    thread.header.id
                )));
            }

            let file = format!("threads/{}.json", thread.header.id);
            std::fs::write(output_dir.join(&file), &thread.decoded)?;

            threads.push(SnapshotThread {
                id: thread.header.id.clone(),
                parent_id: thread.header.parent_id.clone(),
                root_id: root.id.clone(),
                title: thread
                    .document
                    .title
                    .clone()
                    .unwrap_or_else(|| thread.header.summary.clone()),
                summary: thread.header.summary.clone(),
                updated_at: thread.header.updated_at.clone(),
                created_at: thread.header.created_at.clone(),
                model: thread
                    .document
                    .model
                    .as_ref()
                    .and_then(SnapshotModel::display),
                project_paths: thread.header.project_paths(),
                messages: thread.document.messages.len(),
                version: thread.document.version.clone(),
                decoded_bytes: thread.decoded.len() as u64,
                file,
            });
        }

        included_bytes = included_bytes.saturating_add(tree_bytes);
        included_trees += 1;
    }

    if included_trees == 0 {
        if let Some((smallest, title)) = smallest_over_budget {
            return Err(Error::BudgetTooSmall {
                budget: options.max_bytes,
                smallest,
                title,
            });
        }
        return Err(Error::InvalidThread(
            first_undecodable.unwrap_or_else(|| "no thread tree could be read".to_owned()),
        ));
    }

    let manifest = SnapshotManifest {
        format_version: SNAPSHOT_FORMAT_VERSION,
        tool_version: VERSION.to_owned(),
        exported_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        source_database: db_path.display().to_string(),
        selection: SnapshotSelection {
            max_bytes: options.max_bytes,
            included_bytes,
            order: "updated_at descending".to_owned(),
            candidate_trees: roots.len(),
            included_trees,
        },
        threads,
        skipped,
    };
    std::fs::write(
        output_dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    std::fs::write(output_dir.join("README.md"), readme(&manifest))?;

    Ok(ExportOutcome {
        output_dir: output_dir.to_path_buf(),
        threads: manifest.threads.len(),
        trees: manifest.selection.included_trees,
        bytes: manifest.selection.included_bytes,
        skipped_over_budget: manifest
            .skipped
            .iter()
            .filter(|tree| tree.reason == SkipReason::OverBudget)
            .count(),
        skipped_undecodable: manifest
            .skipped
            .iter()
            .filter(|tree| tree.reason == SkipReason::Undecodable)
            .count(),
    })
}

/// A short, self-contained note for whoever receives the snapshot. It describes
/// the format from the code above; no message content is copied into it.
fn readme(manifest: &SnapshotManifest) -> String {
    let budget = if manifest.selection.max_bytes == 0 {
        "No size budget was set.".to_owned()
    } else {
        format!(
            "The budget was {} ({} decoded bytes).",
            format_bytes(manifest.selection.max_bytes),
            manifest.selection.max_bytes
        )
    };

    let mut body = String::new();
    body.push_str("# Zed agent thread snapshot\n\n");
    body.push_str(&format!(
        "Exported {} from `{}` by `zed-thread-export` {}.\n\n",
        manifest.exported_at, manifest.source_database, manifest.tool_version
    ));
    body.push_str("## Files\n\n");
    body.push_str(
        "- `manifest.json` — one entry per exported thread, plus the trees that were not included.\n\
         - `threads/<thread-id>.json` — one decoded Zed `DbThread` document per thread.\n\n",
    );
    body.push_str(
        "A thread with a non-null `parent_id` is a subagent conversation. Whole trees are \
         exported, so every `parent_id` in the manifest resolves inside this snapshot.\n\n",
    );
    body.push_str("## Selection\n\n");
    body.push_str(&format!(
        "The {} threads come from {} top-level trees, newest `updated_at` first, totalling {} \
         decoded bytes. A tree is included whole or not at all, and a tree that would take the \
         total past the budget is listed under `skipped`. {budget}\n\n",
        manifest.threads.len(),
        manifest.selection.included_trees,
        format_bytes(manifest.selection.included_bytes),
    ));
    body.push_str("## The thread document\n\n");
    body.push_str(
        "Each file is the JSON Zed stores in `threads.db` before compressing it, decoded \
         unchanged. The fields a reader is most likely to need:\n\n",
    );
    body.push_str(concat!(
        "- `title`, `model` (with `provider` and `model`), `updated_at`, `version`\n",
        "- `messages` — an array of externally tagged variants: `{\"User\": {...}}`,\n",
        "  `{\"Agent\": {...}}`, `\"Resume\"`, or `{\"Compaction\": {...}}`\n",
        "- an `Agent` message has `content`, an array of `{\"Text\": ...}`, `{\"Thinking\": ...}`,\n",
        "  `{\"ToolUse\": {...}}` and `{\"Image\": ...}` parts, and `tool_results`, a map keyed\n",
        "  by the tool-use id its `ToolUse` carries\n",
        "- `cumulative_token_usage` and `request_token_usage`\n\n",
    ));
    body.push_str(
        "Unknown fields are preserved because the document is the stored JSON, not a model of \
         it.\n\n",
    );
    body.push_str("## Privacy\n\n");
    body.push_str(
        "The documents are unredacted. They contain the prompts, tool output, absolute local \
         paths and any file contents read during the session.\n",
    );
    body
}
