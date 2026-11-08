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
    pub largest_tool_output_tool: Option<String>,
    pub largest_tool_output_message: Option<usize>,
    pub compactions: Option<usize>,
    pub messages_since_compaction: Option<usize>,
    pub model: Option<String>,
    pub project_paths: Vec<String>,
    pub subagent: bool,
    pub error: Option<String>,
}

/// One thread's largest tool outputs, attributed to their tool and message.
/// The request argument is shown; the result body is never read.
#[derive(Debug, Clone, Serialize)]
pub struct ThreadExplanation {
    pub id: String,
    pub title: String,
    pub updated_at: String,
    pub messages: usize,
    pub tool_results: usize,
    pub outputs: Vec<ToolOutputDetail>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolOutputDetail {
    pub message: usize,
    pub tool: String,
    pub bytes: u64,
    pub is_error: bool,
    pub argument: Option<String>,
}

/// Explain where a thread's bytes went. `thread` is an id from the audit table
/// or a unique substring of its title.
///
/// `limit` truncates the output list; `0` explains every result.
pub fn explain(db_path: &Path, thread: &str, limit: usize) -> Result<ThreadExplanation, Error> {
    let connection = db::open_read_only(db_path)?;
    let headers = db::list_headers(&connection)?;
    let header = resolve_thread(&headers, thread)?;

    let record = db::load_record(&connection, &header.id)?;
    let decoded = db::decode_thread(&record.header.data_type, &record.data)?;
    let data: ThreadData = serde_json::from_slice(&decoded)?;

    let mut uses: HashMap<&str, &ToolUse> = HashMap::new();
    for message in &data.messages {
        if let Message::Agent(agent) = message {
            for part in &agent.content {
                if let Some(tool_use) = &part.tool_use {
                    if let Some(id) = tool_use.id.as_deref() {
                        uses.insert(id, tool_use);
                    }
                }
            }
        }
    }

    let mut outputs = Vec::new();
    let mut tool_results = 0;

    for (index, message) in data.messages.iter().enumerate() {
        let Message::Agent(agent) = message else {
            continue;
        };

        for (id, result) in &agent.tool_results {
            tool_results += 1;
            let bytes = result_bytes(&result.content);
            let tool_use = uses.get(id.as_str()).copied();
            let tool = result
                .tool_name
                .clone()
                .or_else(|| tool_use.and_then(|tool_use| tool_use.name.clone()))
                .unwrap_or_else(|| "?".to_owned());
            outputs.push(ToolOutputDetail {
                message: index,
                tool,
                bytes,
                is_error: result.is_error,
                argument: tool_use.and_then(tool_argument),
            });
        }
    }

    outputs.sort_by(|left, right| {
        right
            .bytes
            .cmp(&left.bytes)
            .then_with(|| left.message.cmp(&right.message))
            .then_with(|| left.tool.cmp(&right.tool))
    });
    if limit > 0 {
        outputs.truncate(limit);
    }

    Ok(ThreadExplanation {
        id: header.id.clone(),
        title: data.title.unwrap_or_else(|| header.summary.clone()),
        updated_at: header.updated_at.clone(),
        messages: data.messages.len(),
        tool_results,
        outputs,
    })
}

/// Render the explanation. Like the audit table, this is the only place it
/// prints, so `--json` and the human view cannot disagree.
/// Resolve a thread by id, or by a unique substring of its title. An ambiguous
/// substring is an error rather than a guess.
fn resolve_thread<'a>(
    headers: &'a [ThreadHeader],
    thread: &str,
) -> Result<&'a ThreadHeader, Error> {
    if let Some(header) = headers.iter().find(|header| header.id == thread) {
        return Ok(header);
    }

    let needle = thread.to_lowercase();
    let matches: Vec<&ThreadHeader> = headers
        .iter()
        .filter(|header| header.summary.to_lowercase().contains(&needle))
        .collect();
    match matches.as_slice() {
        [only] => Ok(only),
        [] => Err(Error::ThreadNotFound(thread.to_owned())),
        _ => Err(Error::InvalidThread(format!(
            "{thread:?} matches {} threads; pass the id from the audit table",
            matches.len()
        ))),
    }
}

pub fn print_explanation(explanation: &ThreadExplanation) {
    println!("# zed-thread-audit --explain · {}", explanation.title);
    println!(
        "# {} · {} messages · {} tool results · updated {}",
        explanation.id,
        explanation.messages,
        explanation.tool_results,
        date_part(&explanation.updated_at)
    );
    println!("# largest outputs first · request arguments only · result bodies are never printed");
    println!();

    for output in &explanation.outputs {
        println!(
            "{:>10}  message {:<5} {:<14} {}{}",
            format_bytes(output.bytes),
            output.message,
            output.tool,
            output.argument.as_deref().unwrap_or("-"),
            if output.is_error { "  [error]" } else { "" },
        );
    }
}

// --- behaviour -------------------------------------------------------------

/// What an agent *did* in a thread, counted rather than sized.
///
/// Unlike [`ThreadExplanation`], this reads result bodies: a followed exit exists
/// only in the text a tool printed. It never stores or prints a body — the
/// report carries counts, message indices and request arguments.
#[derive(Debug, Clone, Serialize)]
pub struct BehaviourReport {
    pub id: String,
    pub title: String,
    pub updated_at: String,
    pub messages: usize,
    pub tool_calls: usize,
    pub tool_results: usize,
    pub dead_ends: DeadEnds,
    pub repetitions: Repetitions,
    pub habits: Habits,
    pub exits: Exits,
}

/// Results that told the agent nothing: an error, or no text at all.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DeadEnds {
    pub errors: usize,
    /// Results with no text content. Not filtered by whether a fact was
    /// expected, because nothing in the trace says what was expected.
    pub empty: usize,
    pub sites: Vec<CallSite>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CallSite {
    pub message: usize,
    pub tool: String,
    pub argument: Option<String>,
    pub kind: DeadEndKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeadEndKind {
    Error,
    Empty,
}

/// Calls whose request had already been made earlier in the thread.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Repetitions {
    pub repeated: usize,
    pub sites: Vec<RepeatSite>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepeatSite {
    pub message: usize,
    pub first_message: usize,
    pub tool: String,
    pub argument: String,
}

/// Shell habits the toolkit exists to make unnecessary.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Habits {
    pub pipe_head: usize,
    pub pipe_tail: usize,
    pub separator_echo: usize,
    pub sites: Vec<HabitSite>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Habit {
    PipeHead,
    PipeTail,
    SeparatorEcho,
}

impl Habit {
    fn label(self) -> &'static str {
        match self {
            Self::PipeHead => "| head",
            Self::PipeTail => "| tail",
            Self::SeparatorEcho => "echo ===",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HabitSite {
    pub message: usize,
    pub habit: Habit,
    pub command: String,
}

/// `# next:` footers printed by the inspection tools, and whether a later call
/// ran one as printed.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Exits {
    pub printed: usize,
    pub followed: usize,
    pub sites: Vec<ExitSite>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExitSite {
    pub message: usize,
    pub command: String,
    pub followed: bool,
    /// The call immediately after the one that printed this exit. Without it,
    /// "not followed" cannot be told apart from "answered a better way".
    pub next: Option<NextCall>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NextCall {
    pub message: usize,
    pub tool: String,
    pub argument: Option<String>,
}

/// One tool call, in message order, with its result paired by `tool_use_id`.
struct Call<'a> {
    message: usize,
    tool: String,
    argument: Option<String>,
    input: ToolInput,
    result: Option<&'a ToolResult>,
}

/// Count what happened in one thread. `limit` truncates each site list; `0`
/// lists every site. Counts are always complete.
pub fn behaviour(db_path: &Path, thread: &str, limit: usize) -> Result<BehaviourReport, Error> {
    let connection = db::open_read_only(db_path)?;
    let headers = db::list_headers(&connection)?;
    let header = resolve_thread(&headers, thread)?;

    let record = db::load_record(&connection, &header.id)?;
    let decoded = db::decode_thread(&record.header.data_type, &record.data)?;
    let data: ThreadData = serde_json::from_slice(&decoded)?;

    let (calls, tool_results) = collect_calls(&data);

    let mut report = BehaviourReport {
        id: header.id.clone(),
        title: data.title.clone().unwrap_or_else(|| header.summary.clone()),
        updated_at: header.updated_at.clone(),
        messages: data.messages.len(),
        tool_calls: calls.len(),
        tool_results,
        dead_ends: DeadEnds::default(),
        repetitions: Repetitions::default(),
        habits: Habits::default(),
        exits: Exits::default(),
    };

    collect_dead_ends(&calls, &mut report.dead_ends);
    collect_repetitions(&calls, &mut report.repetitions);
    collect_habits(&calls, &mut report.habits);
    collect_exits(&calls, &mut report.exits);

    if limit > 0 {
        report.dead_ends.sites.truncate(limit);
        report.repetitions.sites.truncate(limit);
        report.habits.sites.truncate(limit);
        report.exits.sites.truncate(limit);
    }

    Ok(report)
}

/// Render the behaviour report. Like the other views, this is the only place it
/// prints, so `--json` and the human report cannot disagree.
pub fn print_behaviour(report: &BehaviourReport) {
    println!("# zed-thread-audit --behaviour · {}", report.title);
    println!(
        "# {} · {} messages · {} tool calls · {} results · updated {}",
        report.id,
        report.messages,
        report.tool_calls,
        report.tool_results,
        date_part(&report.updated_at)
    );
    println!(
        "# counts, message indices and request arguments only · result bodies are never printed"
    );
    println!();

    println!(
        "dead ends      {:>3} errors · {:>3} empty",
        report.dead_ends.errors, report.dead_ends.empty
    );
    for site in &report.dead_ends.sites {
        println!(
            "  message {:<5} {:<14} {:<8} {}",
            site.message,
            site.tool,
            match site.kind {
                DeadEndKind::Error => "[error]",
                DeadEndKind::Empty => "[empty]",
            },
            truncate(site.argument.as_deref().unwrap_or("-"), 100)
        );
    }

    println!(
        "repetitions    {:>3} calls repeated an earlier argument",
        report.repetitions.repeated
    );
    for site in &report.repetitions.sites {
        println!(
            "  message {:<5} {:<14} {}   first at message {}",
            site.message,
            site.tool,
            truncate(&site.argument, 80),
            site.first_message
        );
    }

    println!(
        "habits         {:>3} | head · {:>3} | tail · {:>3} echo ===",
        report.habits.pipe_head, report.habits.pipe_tail, report.habits.separator_echo
    );
    for site in &report.habits.sites {
        println!(
            "  message {:<5} {:<14} {}",
            site.message,
            site.habit.label(),
            truncate(&site.command, 100)
        );
    }

    let ratio = match (report.exits.printed, report.exits.followed) {
        (0, _) => String::new(),
        (printed, followed) => format!(" ({}%)", followed * 100 / printed),
    };
    println!(
        "exits          {:>3} printed · {:>3} followed{}",
        report.exits.printed, report.exits.followed, ratio
    );
    for site in &report.exits.sites {
        println!(
            "  message {:<5} {:<13} {}",
            site.message,
            if site.followed {
                "followed"
            } else {
                "not followed"
            },
            truncate(&site.command, 80)
        );
        if let Some(next) = &site.next {
            println!(
                "        next: {:<12} {}",
                next.tool,
                truncate(next.argument.as_deref().unwrap_or("-"), 100)
            );
        }
    }
}

fn collect_calls(data: &ThreadData) -> (Vec<Call<'_>>, usize) {
    let mut calls: Vec<Call<'_>> = Vec::new();
    let mut index_by_id: HashMap<&str, usize> = HashMap::new();

    for (message_index, message) in data.messages.iter().enumerate() {
        let Message::Agent(agent) = message else {
            continue;
        };
        for part in &agent.content {
            let Some(tool_use) = &part.tool_use else {
                continue;
            };
            if let Some(id) = tool_use.id.as_deref() {
                index_by_id.insert(id, calls.len());
            }
            calls.push(Call {
                message: message_index,
                tool: tool_use.name.clone().unwrap_or_else(|| "?".to_owned()),
                argument: tool_argument(tool_use),
                input: tool_input(tool_use),
                result: None,
            });
        }
    }

    let mut tool_results = 0;
    for message in &data.messages {
        let Message::Agent(agent) = message else {
            continue;
        };
        for (id, result) in &agent.tool_results {
            tool_results += 1;
            if let Some(index) = index_by_id.get(id.as_str()).copied() {
                calls[index].result = Some(result);
            }
        }
    }

    (calls, tool_results)
}

fn collect_dead_ends(calls: &[Call<'_>], dead_ends: &mut DeadEnds) {
    for call in calls {
        let Some(result) = call.result else {
            continue;
        };
        if result.is_error {
            dead_ends.errors += 1;
            dead_ends.sites.push(call_site(call, DeadEndKind::Error));
        } else if !is_path_write(&call.tool) && result_text(&result.content).is_none() {
            // A successful write returns little by design. Only an observation
            // that returned nothing is a dead end.
            dead_ends.empty += 1;
            dead_ends.sites.push(call_site(call, DeadEndKind::Empty));
        }
    }
}

fn call_site(call: &Call<'_>, kind: DeadEndKind) -> CallSite {
    CallSite {
        message: call.message,
        tool: call.tool.clone(),
        argument: call.argument.clone(),
        kind,
    }
}

fn collect_repetitions(calls: &[Call<'_>], repetitions: &mut Repetitions) {
    let mut first_seen: HashMap<(&str, &str), usize> = HashMap::new();

    for (index, call) in calls.iter().enumerate() {
        // A write repeats its path on purpose. It is not a question whose answer
        // was already in the transcript.
        if is_path_write(&call.tool) {
            continue;
        }

        let Some(argument) = call
            .argument
            .as_deref()
            .filter(|argument| !argument.is_empty())
        else {
            continue;
        };
        let key = (call.tool.as_str(), argument);

        let Some(first) = first_seen.get(&key).copied() else {
            first_seen.insert(key, index);
            continue;
        };

        if changed_between(calls, first, index, call) {
            first_seen.insert(key, index);
            continue;
        }

        repetitions.repeated += 1;
        repetitions.sites.push(RepeatSite {
            message: call.message,
            first_message: calls[first].message,
            tool: call.tool.clone(),
            argument: argument.to_owned(),
        });
    }
}

/// A repeat after a write is a re-read, not a re-derivation. A path-carrying
/// read is narrowed to a write of the same path; a shell observation carries no
/// path to pair on, so any intervening write is taken as a possible change.
fn changed_between(calls: &[Call<'_>], first: usize, current: usize, call: &Call<'_>) -> bool {
    let between = &calls[first + 1..current];

    if is_path_read(&call.tool) {
        if let Some(path) = call.input.path.as_deref() {
            return between.iter().any(|other| {
                is_path_write(&other.tool) && other.input.path.as_deref() == Some(path)
            });
        }
    }

    between.iter().any(|other| is_path_write(&other.tool))
}

fn is_path_read(tool: &str) -> bool {
    matches!(tool, "read_file" | "list_directory" | "find_path" | "grep")
}

fn is_path_write(tool: &str) -> bool {
    matches!(tool, "edit_file" | "write_file" | "delete_path")
}

fn collect_habits(calls: &[Call<'_>], habits: &mut Habits) {
    for call in calls {
        let Some(command) = call.input.command.as_deref() else {
            continue;
        };

        if pipe_targets(command, "head") {
            habits.pipe_head += 1;
            habits
                .sites
                .push(habit_site(call, Habit::PipeHead, command));
        }
        if pipe_targets(command, "tail") {
            habits.pipe_tail += 1;
            habits
                .sites
                .push(habit_site(call, Habit::PipeTail, command));
        }
        if separator_echo(command) {
            habits.separator_echo += 1;
            habits
                .sites
                .push(habit_site(call, Habit::SeparatorEcho, command));
        }
    }
}

fn habit_site(call: &Call<'_>, habit: Habit, command: &str) -> HabitSite {
    HabitSite {
        message: call.message,
        habit,
        command: command.to_owned(),
    }
}

/// True when the command pipes into `program`, matched on a word boundary so
/// `header` does not read as `head`.
fn pipe_targets(command: &str, program: &str) -> bool {
    let mut rest = command;
    while let Some(index) = rest.find('|') {
        let after = rest[index + 1..].trim_start();
        if let Some(remainder) = after.strip_prefix(program) {
            let boundary = remainder.chars().next().is_none_or(|character| {
                character.is_whitespace() || matches!(character, '-' | ';' | '|' | '&')
            });
            if boundary {
                return true;
            }
        }
        rest = &rest[index + 1..];
    }
    false
}

fn separator_echo(command: &str) -> bool {
    [
        "echo \"===",
        "echo '===",
        "echo ===",
        "echo \"---",
        "echo '---",
        "echo ---",
    ]
    .iter()
    .any(|pattern| command.contains(pattern))
}

fn collect_exits(calls: &[Call<'_>], exits: &mut Exits) {
    for (index, call) in calls.iter().enumerate() {
        let Some(result) = call.result else {
            continue;
        };
        let Some(text) = result_text(&result.content) else {
            continue;
        };

        for command in exits_in(&text) {
            let followed = calls[index + 1..]
                .iter()
                .any(|later| ran_as_printed(later, &command));
            exits.printed += 1;
            if followed {
                exits.followed += 1;
            }
            exits.sites.push(ExitSite {
                message: call.message,
                command,
                followed,
                next: calls.get(index + 1).map(next_call),
            });
        }
    }
}

fn next_call(call: &Call<'_>) -> NextCall {
    NextCall {
        message: call.message,
        tool: call.tool.clone(),
        argument: call.argument.clone(),
    }
}

/// An exit is followed when a later call ran its command. Matching is on the
/// command text, not the tool name: the point of an exit is that any tool that
/// can run it is enough.
fn ran_as_printed(call: &Call<'_>, exit: &str) -> bool {
    call.input
        .command
        .as_deref()
        .is_some_and(|command| command.contains(exit))
        || call
            .argument
            .as_deref()
            .is_some_and(|argument| argument.contains(exit))
}

/// A printed footer, with its gloss removed. The format is
/// `# next: <command> · <what it answers>`.
fn exits_in(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("# next:"))
        .map(|rest| rest.split(" · ").next().unwrap_or(rest).trim())
        .filter(|command| !command.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The text a result carried, or `None` when it carried none. Both stored
/// content shapes are handled; images and other non-text parts are absent here.
fn result_text(content: &ToolResultContent) -> Option<String> {
    match content {
        ToolResultContent::One(item) => item
            .text
            .as_deref()
            .filter(|text| !text.is_empty())
            .map(str::to_owned),
        ToolResultContent::Many(items) => {
            let text: String = items
                .iter()
                .filter_map(|item| item.text.as_deref())
                .collect::<Vec<&str>>()
                .join("\n");
            (!text.is_empty()).then_some(text)
        }
        ToolResultContent::Text(text) => (!text.is_empty()).then(|| text.clone()),
    }
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
            largest_tool_output_tool: None,
            largest_tool_output_message: None,
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
    let mut largest_tool_output: Option<ToolOutputCandidate> = None;
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
                for (tool_use_id, result) in &agent_message.tool_results {
                    tool_results += 1;
                    if result.is_error {
                        failed_tool_results += 1;
                    }
                    let bytes = result_bytes(&result.content);
                    tool_output_bytes += bytes;

                    if bytes > 0 {
                        let replace = largest_tool_output.as_ref().is_none_or(|current| {
                            bytes > current.bytes
                                || (bytes == current.bytes
                                    && (index, tool_use_id)
                                        < (current.message, &current.tool_use_id))
                        });
                        if replace {
                            largest_tool_output = Some(ToolOutputCandidate {
                                bytes,
                                message: index,
                                tool_name: result.tool_name.clone().filter(|name| !name.is_empty()),
                                tool_use_id: tool_use_id.clone(),
                            });
                        }
                    }
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
    row.largest_tool_output_bytes = Some(
        largest_tool_output
            .as_ref()
            .map_or(0, |output| output.bytes),
    );
    row.largest_tool_output_tool = largest_tool_output
        .as_ref()
        .and_then(|output| output.tool_name.clone());
    row.largest_tool_output_message = largest_tool_output.map(|output| output.message);
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
        "{:<19} {:>6} {:>5} {:>6} {:>10} {:>10} {:<18} {:>5} {:>5} {:>10} {:<24} Title",
        "Updated",
        "Age",
        "Msgs",
        "Tools",
        "Tool bytes",
        "Largest",
        "Top tool",
        "Comp",
        "Since",
        "Decoded",
        "Model"
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
            "{:<19} {:>6} {:>5} {:>6} {:>10} {:>10} {:<18} {:>5} {:>5} {:>10} {:<24} {}",
            date_part(&thread.updated_at),
            age(thread.age_seconds),
            thread.messages.unwrap_or(0),
            thread.tool_calls.unwrap_or(0),
            format_bytes(thread.tool_output_bytes.unwrap_or(0)),
            format_bytes(thread.largest_tool_output_bytes.unwrap_or(0)),
            tool_attribution(thread),
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

fn result_bytes(content: &ToolResultContent) -> u64 {
    let bytes = match content {
        ToolResultContent::One(item) => item.text.as_deref().map_or(0, str::len),
        ToolResultContent::Many(items) => items
            .iter()
            .filter_map(|item| item.text.as_deref())
            .map(str::len)
            .sum(),
        ToolResultContent::Text(text) => text.len(),
    };
    bytes as u64
}

/// The structured request fields, parsed from `raw_input` (which is where most
/// tools put them) and falling back to the `input` field.
fn tool_input(tool_use: &ToolUse) -> ToolInput {
    tool_use
        .raw_input
        .as_deref()
        .and_then(|raw| serde_json::from_str::<ToolInput>(raw).ok())
        .filter(|input| !input.is_empty())
        .or_else(|| tool_use.input.clone())
        .unwrap_or_default()
}

/// The request side of a call, as one readable line.
fn tool_argument(tool_use: &ToolUse) -> Option<String> {
    let parsed = tool_input(tool_use);

    let argument = match tool_use.name.as_deref().unwrap_or_default() {
        "fetch" => parsed.url.clone(),
        "read_file" => {
            parsed
                .path
                .as_ref()
                .map(|path| match (parsed.start_line, parsed.end_line) {
                    (Some(start), Some(end)) => format!("{path}:{start}-{end}"),
                    (Some(start), None) => format!("{path}:{start}-"),
                    _ => path.clone(),
                })
        }
        "list_directory" | "find_path" => parsed.path.clone(),
        "grep" => parsed
            .pattern
            .clone()
            .map(|pattern| match parsed.path.as_deref() {
                Some(path) => format!("{pattern} in {path}"),
                None => pattern,
            }),
        "terminal" => parsed.command.clone(),
        "web_search" => parsed.query.clone(),
        _ => parsed
            .url
            .clone()
            .or_else(|| parsed.path.clone())
            .or_else(|| parsed.command.clone())
            .or_else(|| parsed.query.clone()),
    };

    argument
        .map(|argument| truncate(&argument, 160))
        .or_else(|| {
            tool_use
                .raw_input
                .as_deref()
                .filter(|raw| !raw.is_empty() && *raw != "{}")
                .map(|raw| truncate(raw, 160))
        })
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ToolInput {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    pattern: Option<String>,
    #[serde(default)]
    start_line: Option<u64>,
    #[serde(default)]
    end_line: Option<u64>,
}

impl ToolInput {
    fn is_empty(&self) -> bool {
        self.url.is_none()
            && self.path.is_none()
            && self.command.is_none()
            && self.query.is_none()
            && self.pattern.is_none()
            && self.start_line.is_none()
            && self.end_line.is_none()
    }
}

fn date_part(value: &str) -> String {
    value.get(..16).unwrap_or(value).replace('T', " ")
}

/// Where the largest tool output sits, as `tool@message-index`. The audit never
/// reads the output itself; the index is what lets a reader open that message.
fn tool_attribution(thread: &ThreadAudit) -> String {
    match (
        thread.largest_tool_output_tool.as_deref(),
        thread.largest_tool_output_message,
    ) {
        (Some(tool), Some(message)) => truncate(&format!("{tool}@{message}"), 18),
        (None, Some(message)) => truncate(&format!("tool@{message}"), 18),
        _ => "-".to_owned(),
    }
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

/// One tool result, kept only so the largest output can be attributed after the
/// walk. `tool_use_id` is the deterministic tiebreak when HashMap order is not.
struct ToolOutputCandidate {
    bytes: u64,
    message: usize,
    tool_name: Option<String>,
    tool_use_id: String,
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

/// The payload carries the id and name; the audit only counts the presence, and
/// `--explain` uses the id to pair a result with its request.
#[derive(Deserialize)]
struct ToolUse {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    raw_input: Option<String>,
    #[serde(default)]
    input: Option<ToolInput>,
}

#[derive(Deserialize)]
struct ToolResult {
    #[serde(default)]
    content: ToolResultContent,
    #[serde(default)]
    is_error: bool,
    #[serde(default)]
    tool_name: Option<String>,
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
