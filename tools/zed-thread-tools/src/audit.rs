//! The read-only audit: one row of metadata per thread, and no message text.

use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};

use crate::db::{self, ThreadHeader};
use crate::Error;

#[derive(Debug, Clone)]
pub struct AuditOptions {
    /// `0` means every matching thread.
    pub top: usize,
    pub older_than: Option<NaiveDate>,
    pub project: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditReport {
    pub database: String,
    pub generated_at: String,
    pub total_threads: usize,
    pub matched_threads: usize,
    pub shown_threads: usize,
    pub failed_threads: usize,
    pub total_compressed_bytes: u64,
    pub total_decoded_bytes: u64,
    pub threads: Vec<ThreadAudit>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThreadAudit {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub updated_at: String,
    pub age_seconds: Option<i64>,
    pub compressed_bytes: u64,
    pub decoded_bytes: Option<u64>,
    pub messages: Option<usize>,
    pub user_messages: Option<usize>,
    pub agent_messages: Option<usize>,
    pub tool_calls: Option<usize>,
    pub tool_results: Option<usize>,
    pub failed_tool_results: Option<usize>,
    pub tool_output_bytes: Option<u64>,
    pub largest_tool_output_bytes: Option<u64>,
    pub compactions: Option<usize>,
    pub messages_since_compaction: Option<usize>,
    pub model: Option<String>,
    pub project_paths: Vec<String>,
    pub subagent: bool,
    pub error: Option<String>,
}

/// Audit every thread that matches the filters, loading one compressed blob at
/// a time. A thread that cannot be decoded becomes an error row rather than
/// aborting the whole run.
pub fn audit(db_path: &Path, options: &AuditOptions) -> Result<AuditReport, Error> {
    let connection = db::open_read_only(db_path)?;
    let headers = db::list_headers(&connection)?;
    let total_threads = headers.len();
    let mut matched = Vec::new();

    for header in &headers {
        if !matches_filters(header, options) {
            continue;
        }

        let mut row = ThreadAudit::from_header(header);

        match db::load_record(&connection, &header.id) {
            Ok(record) => match db::decode_thread(&record.header.data_type, &record.data) {
                Ok(decoded) => {
                    row.decoded_bytes = Some(decoded.len() as u64);
                    match serde_json::from_slice::<ThreadData>(&decoded) {
                        Ok(data) => summarize(&data, &mut row),
                        Err(error) => row.error = Some(format!("parse: {error}")),
                    }
                }
                Err(error) => row.error = Some(format!("decode: {error}")),
            },
            Err(error) => row.error = Some(format!("read: {error}")),
        }

        matched.push(row);
    }

    let failed_threads = matched.iter().filter(|row| row.error.is_some()).count();
    let total_compressed_bytes = matched.iter().map(|row| row.compressed_bytes).sum();
    let total_decoded_bytes = matched
        .iter()
        .map(|row| row.decoded_bytes.unwrap_or(0))
        .sum();

    matched.sort_by(|left, right| {
        right
            .decoded_bytes
            .unwrap_or(0)
            .cmp(&left.decoded_bytes.unwrap_or(0))
            .then_with(|| right.compressed_bytes.cmp(&left.compressed_bytes))
            .then_with(|| right.updated_at.cmp(&left.updated_at))
    });

    let shown_threads = if options.top == 0 {
        matched.len()
    } else {
        options.top.min(matched.len())
    };
    let threads = matched.into_iter().take(shown_threads).collect();

    Ok(AuditReport {
        database: db_path.display().to_string(),
        generated_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        total_threads,
        matched_threads: matched_threads(&headers, options),
        shown_threads,
        failed_threads,
        total_compressed_bytes,
        total_decoded_bytes,
        threads,
    })
}

/// The count before `top` truncation. Kept separate so the report can say
/// "showing 20 of 43" without keeping the rows it did not show.
fn matched_threads(headers: &[ThreadHeader], options: &AuditOptions) -> usize {
    headers
        .iter()
        .filter(|header| matches_filters(header, options))
        .count()
}

fn matches_filters(header: &ThreadHeader, options: &AuditOptions) -> bool {
    if let Some(cutoff) = options.older_than {
        if let Some(updated) = parse_rfc3339(&header.updated_at) {
            if updated.date_naive() >= cutoff {
                return false;
            }
        }
    }

    if let Some(project) = options.project.as_deref() {
        let needle = project.to_lowercase();
        if !header
            .project_paths()
            .iter()
            .any(|path| path.to_lowercase().contains(&needle))
        {
            return false;
        }
    }

    true
}

impl ThreadAudit {
    fn from_header(header: &ThreadHeader) -> Self {
        Self {
            id: header.id.clone(),
            title: header.summary.clone(),
            summary: header.summary.clone(),
            updated_at: header.updated_at.clone(),
            age_seconds: parse_rfc3339(&header.updated_at)
                .map(|updated| Utc::now().signed_duration_since(updated).num_seconds()),
            compressed_bytes: header.compressed_bytes,
            decoded_bytes: None,
            messages: None,
            user_messages: None,
            agent_messages: None,
            tool_calls: None,
            tool_results: None,
            failed_tool_results: None,
            tool_output_bytes: None,
            largest_tool_output_bytes: None,
            compactions: None,
            messages_since_compaction: None,
            model: None,
            project_paths: header.project_paths(),
            subagent: header.parent_id.is_some(),
            error: None,
        }
    }
}

fn summarize(data: &ThreadData, row: &mut ThreadAudit) {
    if let Some(title) = data.title.as_deref().filter(|title| !title.is_empty()) {
        row.title = title.to_owned();
    }

    if row.age_seconds.is_none() {
        row.age_seconds = data
            .updated_at
            .as_deref()
            .and_then(parse_rfc3339)
            .map(|updated| Utc::now().signed_duration_since(updated).num_seconds());
    }

    row.model = data.model.as_ref().and_then(Model::display);
    row.subagent = row.subagent || data.subagent_context.is_some();

    let mut user_messages = 0;
    let mut agent_messages = 0;
    let mut tool_calls = 0;
    let mut tool_results = 0;
    let mut failed_tool_results = 0;
    let mut tool_output_bytes = 0u64;
    let mut largest_tool_output_bytes = 0u64;
    let mut compactions = 0;
    let mut last_compaction = None;

    for (index, message) in data.messages.iter().enumerate() {
        match message {
            Message::User(_) => user_messages += 1,
            Message::Agent(agent_message) => {
                agent_messages += 1;
                for part in &agent_message.content {
                    if part.tool_use.is_some() {
                        tool_calls += 1;
                    }
                }
                for result in agent_message.tool_results.values() {
                    tool_results += 1;
                    if result.is_error {
                        failed_tool_results += 1;
                    }
                    let bytes = match &result.content {
                        ToolResultContent::One(item) => item.text.as_deref().map_or(0, str::len),
                        ToolResultContent::Many(items) => items
                            .iter()
                            .filter_map(|item| item.text.as_deref())
                            .map(str::len)
                            .sum(),
                        ToolResultContent::Text(text) => text.len(),
                    };
                    let bytes = bytes as u64;
                    tool_output_bytes += bytes;
                    largest_tool_output_bytes = largest_tool_output_bytes.max(bytes);
                }
            }
            Message::Compaction(_) => {
                compactions += 1;
                last_compaction = Some(index);
            }
            Message::Resume => {}
        }
    }

    row.messages = Some(data.messages.len());
    row.user_messages = Some(user_messages);
    row.agent_messages = Some(agent_messages);
    row.tool_calls = Some(tool_calls);
    row.tool_results = Some(tool_results);
    row.failed_tool_results = Some(failed_tool_results);
    row.tool_output_bytes = Some(tool_output_bytes);
    row.largest_tool_output_bytes = Some(largest_tool_output_bytes);
    row.compactions = Some(compactions);
    row.messages_since_compaction = Some(match last_compaction {
        Some(index) => data.messages.len().saturating_sub(index + 1),
        None => data.messages.len(),
    });
}

/// Render the human report. This is the only place the audit prints, so
/// `--json` and the table cannot disagree about what was measured.
pub fn print_report(report: &AuditReport) {
    println!("# zed-thread-audit · {}", report.database);
    println!(
        "# {} threads, {} matched, showing {} · {} failed to decode",
        report.total_threads, report.matched_threads, report.shown_threads, report.failed_threads
    );
    println!(
        "# {} compressed · {} decoded",
        format_bytes(report.total_compressed_bytes),
        format_bytes(report.total_decoded_bytes)
    );
    println!();

    println!(
        "{:<19} {:>6} {:>5} {:>6} {:>10} {:>5} {:>5} {:>10} {:<24} Title",
        "Updated", "Age", "Msgs", "Tools", "Tool bytes", "Comp", "Since", "Decoded", "Model"
    );

    for thread in &report.threads {
        if let Some(error) = &thread.error {
            println!(
                "{:<19} {:>6}  ! {}",
                date_part(&thread.updated_at),
                age(thread.age_seconds),
                error
            );
            continue;
        }

        println!(
            "{:<19} {:>6} {:>5} {:>6} {:>10} {:>5} {:>5} {:>10} {:<24} {}",
            date_part(&thread.updated_at),
            age(thread.age_seconds),
            thread.messages.unwrap_or(0),
            thread.tool_calls.unwrap_or(0),
            format_bytes(thread.tool_output_bytes.unwrap_or(0)),
            thread.compactions.unwrap_or(0),
            thread.messages_since_compaction.unwrap_or(0),
            format_bytes(thread.decoded_bytes.unwrap_or(0)),
            truncate(thread.model.as_deref().unwrap_or("-"), 24),
            truncate(&thread.title, 48),
        );
    }

    if report.matched_threads > report.shown_threads {
        println!(
            "# showing {} of {} matched threads; use --top 0 for all",
            report.shown_threads, report.matched_threads
        );
    }
}

fn parse_rfc3339(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date_time| date_time.with_timezone(&Utc))
}

fn date_part(value: &str) -> String {
    value.get(..16).unwrap_or(value).replace('T', " ")
}

pub fn age(seconds: Option<i64>) -> String {
    match seconds {
        None => "-".to_owned(),
        Some(seconds) if seconds < 60 => "now".to_owned(),
        Some(seconds) if seconds < 3_600 => format!("{}m", seconds / 60),
        Some(seconds) if seconds < 86_400 => format!("{}h", seconds / 3_600),
        Some(seconds) => format!("{}d", seconds / 86_400),
    }
}

pub fn format_bytes(bytes: u64) -> String {
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

pub fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_owned();
    }

    let mut shortened: String = value.chars().take(max.saturating_sub(3)).collect();
    shortened.push_str("...");
    shortened
}

#[derive(Deserialize)]
struct ThreadData {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    messages: Vec<Message>,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    model: Option<Model>,
    #[serde(default)]
    subagent_context: Option<IgnoredAny>,
}

#[derive(Deserialize)]
struct Model {
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

impl Model {
    fn display(&self) -> Option<String> {
        match (self.provider.as_deref(), self.model.as_deref()) {
            (Some(provider), Some(model)) => Some(format!("{provider}/{model}")),
            (Some(provider), None) => Some(provider.to_owned()),
            (None, Some(model)) => Some(model.to_owned()),
            (None, None) => None,
        }
    }
}

#[derive(Deserialize)]
enum Message {
    User(UserMessage),
    Agent(AgentMessage),
    Resume,
    Compaction(IgnoredAny),
}

#[derive(Deserialize)]
struct UserMessage {}

#[derive(Deserialize)]
struct AgentMessage {
    #[serde(default)]
    content: Vec<ContentPart>,
    #[serde(default)]
    tool_results: HashMap<String, ToolResult>,
}

/// A content part is externally tagged: `{"Text": "..."}`, `{"ToolUse": {...}}`
/// and so on. Modelled as optional fields so a tag this tool does not know
/// about is skipped rather than causing the whole thread to fail.
#[derive(Deserialize)]
struct ContentPart {
    #[serde(rename = "Text")]
    _text: Option<IgnoredAny>,
    #[serde(rename = "Thinking")]
    _thinking: Option<IgnoredAny>,
    #[serde(rename = "ToolUse")]
    tool_use: Option<ToolUse>,
    #[serde(rename = "Image")]
    _image: Option<IgnoredAny>,
}

/// The payload is ignored; only its presence matters to the audit. The empty
/// struct accepts the map and discards its fields.
#[derive(Deserialize)]
struct ToolUse {}

#[derive(Deserialize)]
struct ToolResult {
    #[serde(default)]
    content: ToolResultContent,
    #[serde(default)]
    is_error: bool,
}

/// Zed changed this field from a single block to an array. Both shapes are
/// real stored data, not an unknown schema, so the union is explicit.
#[derive(Deserialize)]
#[serde(untagged)]
enum ToolResultContent {
    One(ContentItem),
    Many(Vec<ContentItem>),
    Text(String),
}

impl Default for ToolResultContent {
    fn default() -> Self {
        Self::Many(Vec::new())
    }
}

#[derive(Deserialize)]
struct ContentItem {
    #[serde(rename = "Text")]
    text: Option<String>,
}
