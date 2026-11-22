//! A bounded, text-only continuation digest of one thread.
//!
//! The problem this solves is not context length: Zed's compaction is token-based, while a
//! provider can reject a request on bytes. A thread that has read dozens of renders carries tens
//! of megabytes of base64 images, so its next request is refused before the model sees it — and
//! `/compact` cannot rescue it, because compaction is itself a request over the same body.
//!
//! A digest keeps the shapes a new thread needs to continue: what the user asked for, which files
//! were touched, what the agent did, and the results that decided the next step. Images become a
//! one-line placeholder naming the file and its size; thinking is dropped. The activity section is
//! budgeted head-and-tail, so the goal and the most recent state survive and the middle goes.
//!
//! The old thread is still authoritative — this is a reading, not a rewrite.

use std::collections::{BTreeMap, BTreeSet};

use serde::de::IgnoredAny;
use serde::Deserialize;
use serde_json::value::RawValue;

use crate::db::{decode_thread, ThreadRecord, DB_THREAD_VERSION};
use crate::Error;

/// The activity budget when the caller does not name one.
pub const DEFAULT_BUDGET_BYTES: usize = 64 * 1024;
/// The ceiling `--budget-kb` cannot raise.
pub const MAX_BUDGET_BYTES: usize = 256 * 1024;

const USER_CLIP: usize = 1_200;
const AGENT_CLIP: usize = 800;
const RESULT_CLIP: usize = 240;
const SUMMARY_CLIP: usize = 4_000;
const ARG_CLIP: usize = 160;
const MAX_FILES: usize = 60;
const MAX_CANDIDATES: usize = 10;

#[derive(Debug, Clone, Copy)]
pub struct DigestOptions {
    /// Bytes of agent activity the digest may carry. Images and user direction are separate.
    pub budget_bytes: usize,
}

impl Default for DigestOptions {
    fn default() -> Self {
        Self {
            budget_bytes: DEFAULT_BUDGET_BYTES,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DigestStats {
    pub messages: usize,
    pub user_messages: usize,
    pub tool_calls: usize,
    pub images: usize,
    pub image_bytes: usize,
    pub thinking: usize,
    pub activity_lines: usize,
    pub activity_shown: usize,
    pub omitted_activity: usize,
    pub unreadable_messages: usize,
}

#[derive(Debug, Clone)]
pub struct Digest {
    pub markdown: String,
    pub stats: DigestStats,
}

/// Build the digest for one stored thread. The bytes are decoded and parsed; nothing is written.
pub fn digest(record: &ThreadRecord, options: &DigestOptions) -> Result<Digest, Error> {
    let decoded = decode_thread(&record.header.data_type, &record.data)?;
    let text = std::str::from_utf8(&decoded)
        .map_err(|error| Error::InvalidThread(format!("thread JSON is not UTF-8: {error}")))?;
    let document: Document = serde_json::from_str(text)?;

    if let Some(version) = document.version.as_deref() {
        if version != DB_THREAD_VERSION {
            return Err(Error::UnsupportedVersion(version.to_owned()));
        }
    }

    let mut stats = DigestStats {
        messages: document.messages.len(),
        ..DigestStats::default()
    };

    let mut user: Vec<String> = Vec::new();
    let mut summaries: Vec<String> = Vec::new();
    let mut activity: Vec<String> = Vec::new();
    let mut files: BTreeMap<String, FileUse> = BTreeMap::new();

    for (index, raw) in document.messages.iter().enumerate() {
        let shape: MessageShape = match serde_json::from_str(raw.get()) {
            Ok(shape) => shape,
            Err(_) => {
                stats.unreadable_messages += 1;
                continue;
            }
        };

        if let Some(message) = shape.user {
            for item in &message.content {
                if let Some(image) = item.image.as_ref() {
                    stats.images += 1;
                    stats.image_bytes += image.source.len();
                }
                if let Some(text) = item.text.as_deref() {
                    stats.user_messages += 1;
                    user.push(format!("- [m{index}] {}", clip(&one_line(text), USER_CLIP)));
                }
            }
            continue;
        }

        if let Some(summary) = shape.compaction.and_then(|info| info.summary) {
            let text = clip(&one_line(&summary), SUMMARY_CLIP);
            if !text.is_empty() {
                summaries.push(format!("- [m{index}] {text}"));
            }
            continue;
        }

        let Some(message) = shape.agent else {
            continue;
        };

        for item in &message.content {
            if item.thinking.is_some() {
                stats.thinking += 1;
                continue;
            }

            if let Some(text) = item.text.as_deref() {
                let text = clip(&one_line(text), AGENT_CLIP);
                if !text.is_empty() {
                    activity.push(format!("**Agent [m{index}]:** {text}"));
                }
                continue;
            }

            let Some(call) = item.tool_use.as_ref() else {
                continue;
            };
            stats.tool_calls += 1;

            let argument = argument(call);
            if let Some(path) = file_path(call) {
                let use_ = files.entry(path).or_default();
                use_.count += 1;
                use_.tools.insert(call.name.clone());
            }

            let line = match message.tool_results.get(&call.id) {
                None => format!("- [m{index}] `{}` {argument} → (no result)", call.name),
                Some(result) => {
                    let images: Vec<&ImageData> = result
                        .content
                        .iter()
                        .filter_map(|item| item.image.as_ref())
                        .collect();
                    if !images.is_empty() {
                        let bytes: usize = images.iter().map(|image| image.source.len()).sum();
                        stats.images += images.len();
                        stats.image_bytes += bytes;
                        format!(
                            "- [m{index}] `{}` {argument} → [image omitted, {} kB]",
                            call.name,
                            kilobytes(bytes)
                        )
                    } else {
                        let text = result
                            .content
                            .iter()
                            .filter_map(|item| item.text.as_deref())
                            .map(one_line)
                            .collect::<Vec<_>>()
                            .join(" ");
                        let text = clip(&text, RESULT_CLIP);
                        let text = if text.is_empty() {
                            "(no text)".to_owned()
                        } else {
                            text
                        };
                        if result.is_error {
                            format!("- [m{index}] `{}` {argument} → ERROR: {text}", call.name)
                        } else {
                            format!("- [m{index}] `{}` {argument} → {text}", call.name)
                        }
                    }
                }
            };
            activity.push(line);
        }
    }

    stats.activity_lines = activity.len();
    let (head, tail, omitted) = split(&activity, options.budget_bytes);
    stats.activity_shown = head.len() + tail.len();
    stats.omitted_activity = omitted;

    let mut markdown = String::new();
    markdown.push_str("## Goal and user direction\n\n");
    if user.is_empty() {
        markdown.push_str("_No user messages._\n");
    } else {
        for line in &user {
            markdown.push_str(line);
            markdown.push('\n');
        }
    }
    markdown.push('\n');

    if !summaries.is_empty() {
        markdown.push_str("## Earlier compactions\n\n");
        for line in &summaries {
            markdown.push_str(line);
            markdown.push('\n');
        }
        markdown.push('\n');
    }

    markdown.push_str("## Files touched\n\n");
    if files.is_empty() {
        markdown.push_str("_None._\n");
    } else {
        let mut ranked: Vec<(&String, &FileUse)> = files.iter().collect();
        ranked.sort_by(|left, right| {
            right
                .1
                .count
                .cmp(&left.1.count)
                .then_with(|| left.0.cmp(right.0))
        });
        for (path, use_) in ranked.iter().take(MAX_FILES) {
            let tools = use_.tools.iter().cloned().collect::<Vec<_>>().join(", ");
            markdown.push_str(&format!("- {}× {} ({tools})\n", use_.count, path));
        }
        if ranked.len() > MAX_FILES {
            markdown.push_str(&format!(
                "- … {} more paths not shown\n",
                ranked.len() - MAX_FILES
            ));
        }
    }
    markdown.push('\n');

    markdown.push_str("## Agent activity\n\n");
    for line in &head {
        markdown.push_str(line);
        markdown.push('\n');
    }
    if omitted > 0 {
        markdown.push_str(&format!(
            "\n_… {omitted} activity lines from the middle omitted …_\n\n"
        ));
    }
    for line in &tail {
        markdown.push_str(line);
        markdown.push('\n');
    }
    if head.is_empty() && tail.is_empty() {
        markdown.push_str("_No agent activity._\n");
    }

    Ok(Digest { markdown, stats })
}

/// The threads whose id or title contains `query`, newest first. An exact id wins outright.
pub fn candidates(
    headers: &[crate::db::ThreadHeader],
    query: &str,
) -> Vec<crate::db::ThreadHeader> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }

    if let Some(exact) = headers.iter().find(|header| header.id == query) {
        return vec![exact.clone()];
    }

    let needle = query.to_lowercase();
    headers
        .iter()
        .filter(|header| header.summary.to_lowercase().contains(&needle))
        .cloned()
        .collect()
}

/// One activity block, cut to the budget with the head and tail kept.
fn split(blocks: &[String], budget: usize) -> (Vec<&str>, Vec<&str>, usize) {
    let total: usize = blocks.iter().map(|block| block.len() + 1).sum();
    if total <= budget {
        return (blocks.iter().map(String::as_str).collect(), Vec::new(), 0);
    }

    let tail_budget = budget * 6 / 10;
    let mut tail_start = blocks.len();
    let mut used = 0;
    while tail_start > 0 {
        let length = blocks[tail_start - 1].len() + 1;
        if used + length > tail_budget {
            break;
        }
        used += length;
        tail_start -= 1;
    }

    let head_budget = budget.saturating_sub(used);
    let mut head_end = 0;
    let mut head_used = 0;
    while head_end < tail_start {
        let length = blocks[head_end].len() + 1;
        if head_used + length > head_budget {
            break;
        }
        head_used += length;
        head_end += 1;
    }

    let omitted = tail_start.saturating_sub(head_end);
    (
        blocks[..head_end].iter().map(String::as_str).collect(),
        blocks[tail_start..].iter().map(String::as_str).collect(),
        omitted,
    )
}

fn argument(call: &ToolUse) -> String {
    let Some(raw) = call.input.as_deref() else {
        return "(no argument)".to_owned();
    };

    let shape: ArgShape = serde_json::from_str(raw.get()).unwrap_or_default();
    let inner = shape
        .value
        .as_deref()
        .and_then(|value| serde_json::from_str::<ArgShape>(value.get()).ok());

    let argued = [inner.as_ref(), Some(&shape)]
        .into_iter()
        .flatten()
        .find_map(|shape| {
            shape
                .path
                .clone()
                .or_else(|| shape.command.clone())
                .or_else(|| shape.query.clone())
                .or_else(|| shape.url.clone())
                .or_else(|| shape.pattern.clone())
        });

    match argued {
        Some(value) => clip(&one_line(&value), ARG_CLIP),
        None => clip(&one_line(raw.get()), ARG_CLIP),
    }
}

fn file_path(call: &ToolUse) -> Option<String> {
    if !matches!(
        call.name.as_str(),
        "read_file" | "edit_file" | "write_file" | "delete_path"
    ) {
        return None;
    }

    let raw = call.input.as_deref()?;
    let shape: ArgShape = serde_json::from_str(raw.get()).ok()?;
    let inner = shape
        .value
        .as_deref()
        .and_then(|value| serde_json::from_str::<ArgShape>(value.get()).ok());
    let path = inner
        .as_ref()
        .and_then(|shape| shape.path.clone())
        .or(shape.path)?;

    // A render is named where it was read, in the activity placeholder and in the image count. It
    // is not a file the work was done in, and a visual pass would otherwise bury every source path
    // under forty contact sheets.
    if is_image(&path) {
        return None;
    }
    Some(path)
}

fn is_image(path: &str) -> bool {
    let path = path.to_ascii_lowercase();
    [
        ".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp", ".tif", ".tiff",
    ]
    .iter()
    .any(|extension| path.ends_with(extension))
}

fn one_line(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for character in text.chars() {
        if character.is_whitespace() {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
        }
        space = false;
        out.push(character);
    }
    out
}

fn clip(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(limit).collect();
    out.push_str(" …");
    out
}

fn kilobytes(bytes: usize) -> usize {
    bytes / 1024
}

/// The number of candidate lines a caller may be shown when a title is ambiguous.
pub const CANDIDATE_LIMIT: usize = MAX_CANDIDATES;

#[derive(Debug, Clone, Default)]
struct FileUse {
    count: usize,
    tools: BTreeSet<String>,
}

/// The parts of a `DbThread` this module reads. Every other field is ignored, because the store is
/// an external schema: a field Zed adds must not turn a digest into a parse failure.
#[derive(Deserialize)]
struct Document {
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    messages: Vec<Box<RawValue>>,
}

#[derive(Deserialize)]
struct MessageShape {
    #[serde(rename = "User", default)]
    user: Option<UserMessage>,
    #[serde(rename = "Agent", default)]
    agent: Option<AgentMessage>,
    #[serde(rename = "Compaction", default)]
    compaction: Option<CompactionMessage>,
}

#[derive(Deserialize)]
struct UserMessage {
    #[serde(default)]
    content: Vec<ContentItem>,
}

#[derive(Deserialize)]
struct AgentMessage {
    #[serde(default)]
    content: Vec<ContentItem>,
    #[serde(default)]
    tool_results: BTreeMap<String, ToolResult>,
}

#[derive(Deserialize)]
struct CompactionMessage {
    #[serde(rename = "Summary", default)]
    summary: Option<String>,
}

#[derive(Deserialize)]
struct ContentItem {
    #[serde(rename = "Text", default)]
    text: Option<String>,
    #[serde(rename = "Thinking", default)]
    thinking: Option<IgnoredAny>,
    #[serde(rename = "ToolUse", default)]
    tool_use: Option<ToolUse>,
    #[serde(rename = "Image", default)]
    image: Option<ImageData>,
}

#[derive(Deserialize)]
struct ToolUse {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    input: Option<Box<RawValue>>,
}

#[derive(Deserialize)]
struct ToolResult {
    #[serde(default)]
    is_error: bool,
    #[serde(default)]
    content: Vec<ContentItem>,
}

#[derive(Deserialize)]
struct ImageData {
    #[serde(default)]
    source: String,
}

#[derive(Deserialize, Default)]
struct ArgShape {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    pattern: Option<String>,
    #[serde(default)]
    value: Option<Box<RawValue>>,
}

#[cfg(test)]
mod tests {
    use super::{candidates, digest, split, DigestOptions};
    use crate::db::{ThreadHeader, ThreadRecord};

    fn record(messages: &str) -> ThreadRecord {
        let json = format!(r#"{{"version":"0.3.0","messages":[{messages}]}}"#);
        ThreadRecord {
            header: ThreadHeader {
                id: "8b0d".to_owned(),
                summary: "Exoplanet review".to_owned(),
                updated_at: "2026-11-22T07:23:54Z".to_owned(),
                data_type: "json".to_owned(),
                compressed_bytes: json.len() as u64,
                parent_id: None,
                worktree_branch: None,
                folder_paths: Some("unify-space".to_owned()),
                folder_paths_order: None,
                created_at: None,
            },
            data: json.into_bytes(),
        }
    }

    #[test]
    fn images_become_placeholders_and_keep_their_size() {
        let image = "iVBORw0KGgo".repeat(1_000);
        let json = record(&format!(
            r#"{{"Agent":{{"content":[{{"ToolUse":{{"id":"c1","name":"read_file","input":{{"type":"json","value":{{"path":"unify-space/.tmp/globe.png"}}}}}}}}],"tool_results":{{"c1":{{"is_error":false,"content":[{{"Image":{{"source":"{image}"}}}}]}}}}}}}}"#
        ));
        let digest = digest(&json, &DigestOptions::default()).expect("digest");
        assert!(digest.markdown.contains("unify-space/.tmp/globe.png"));
        assert!(digest.markdown.contains("[image omitted"));
        assert!(!digest.markdown.contains("iVBORw0KGgo"));
        assert_eq!(digest.stats.images, 1);
        assert!(digest.stats.image_bytes >= 10_000);
    }

    #[test]
    fn root_images_from_a_user_message_stay_out_but_are_counted() {
        let image = "iVBORw0KGgo".repeat(1_000);
        let json = record(&format!(
            r#"{{"User":{{"content":[{{"Image":{{"source":"{image}"}}}}]}}}}"#
        ));
        let digest = digest(&json, &DigestOptions::default()).expect("digest");
        assert!(!digest.markdown.contains("iVBORw0KGgo"));
        assert!(!digest.markdown.contains("[image omitted"));
        assert_eq!(digest.stats.images, 1);
    }

    #[test]
    fn thinking_is_dropped_and_counted() {
        let json = record(
            r#"{"Agent":{"content":[{"Thinking":{"text":"secret reasoning"}},{"Text":"visible"}]}}"#,
        );
        let digest = digest(&json, &DigestOptions::default()).expect("digest");
        assert!(!digest.markdown.contains("secret reasoning"));
        assert!(digest.markdown.contains("visible"));
        assert_eq!(digest.stats.thinking, 1);
    }

    #[test]
    fn an_error_result_is_named_as_one() {
        let json = record(
            r#"{"Agent":{"content":[{"ToolUse":{"id":"c1","name":"terminal","input":{"type":"json","value":{"command":"cargo test"}}}}],"tool_results":{"c1":{"is_error":true,"content":[{"Text":"error: nope"}]}}}}"#,
        );
        let digest = digest(&json, &DigestOptions::default()).expect("digest");
        assert!(digest
            .markdown
            .contains("`terminal` cargo test → ERROR: error: nope"));
    }

    #[test]
    fn a_budgeted_thread_keeps_the_head_and_the_tail_and_says_what_went() {
        let mut messages = Vec::new();
        for index in 0..80 {
            messages.push(format!(
                r#"{{"Agent":{{"content":[{{"Text":"step {index} {}"}}]}}}}"#,
                "x".repeat(400)
            ));
        }
        let json = record(&messages.join(","));
        let digest = digest(
            &json,
            &DigestOptions {
                budget_bytes: 4 * 1024,
            },
        )
        .expect("digest");
        assert!(digest.markdown.contains("step 0"));
        assert!(digest.markdown.contains("step 79"));
        assert!(digest
            .markdown
            .contains("activity lines from the middle omitted"));
        assert!(digest.stats.omitted_activity > 0);
    }

    #[test]
    fn compaction_summaries_are_kept() {
        let json = record(r#"{"Compaction":{"Summary":"the plan so far"}}"#);
        let digest = digest(&json, &DigestOptions::default()).expect("digest");
        assert!(digest.markdown.contains("the plan so far"));
    }

    #[test]
    fn an_unknown_message_kind_is_ignored_not_fatal() {
        let json = record(r#"{"Future":{"x":1}}"#);
        let digest = digest(&json, &DigestOptions::default()).expect("digest");
        assert_eq!(digest.stats.unreadable_messages, 0);
        assert_eq!(digest.stats.messages, 1);
    }

    #[test]
    fn split_keeps_both_ends() {
        let blocks: Vec<String> = (0..10).map(|index| format!("line {index}")).collect();
        let (head, tail, omitted) = split(&blocks, 40);
        assert_eq!(head.first().copied(), Some("line 0"));
        assert_eq!(tail.last().copied(), Some("line 9"));
        assert!(omitted > 0);
        assert_eq!(head.len() + tail.len() + omitted, blocks.len());
    }

    #[test]
    fn a_title_resolves_by_fragment_but_an_id_wins() {
        let headers = vec![
            ThreadHeader {
                id: "aaaa".to_owned(),
                summary: "Exoplanet Set Rendering".to_owned(),
                updated_at: String::new(),
                data_type: String::new(),
                compressed_bytes: 0,
                parent_id: None,
                worktree_branch: None,
                folder_paths: None,
                folder_paths_order: None,
                created_at: None,
            },
            ThreadHeader {
                id: "bbbb".to_owned(),
                summary: "Exoplanet catalogue".to_owned(),
                updated_at: String::new(),
                data_type: String::new(),
                compressed_bytes: 0,
                parent_id: None,
                worktree_branch: None,
                folder_paths: None,
                folder_paths_order: None,
                created_at: None,
            },
        ];
        assert_eq!(candidates(&headers, "exoplanet").len(), 2);
        assert_eq!(candidates(&headers, "aaaa").len(), 1);
        assert_eq!(candidates(&headers, "catalogue").len(), 1);
        assert!(candidates(&headers, "nothing").is_empty());
    }
}
