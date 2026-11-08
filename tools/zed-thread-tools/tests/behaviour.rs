mod common;

use std::path::PathBuf;

use zed_thread_tools::audit::{behaviour, DeadEndKind};

use common::{compress, create_db, insert_thread, thread_json, TempDir};

const USER: &str = r#"{"User":{"id":"u1","content":[{"Text":"go"}]}}"#;

fn agent(id: &str, name: &str, raw_input: &str, text: Option<&str>, is_error: bool) -> String {
    let raw = serde_json::to_string(raw_input).expect("encode raw input");
    let content = match text {
        Some(text) => {
            let text = serde_json::to_string(text).expect("encode result text");
            format!(r#"[{{"Text":{text}}}]"#)
        }
        None => "[]".to_owned(),
    };

    let tool_use = format!(r#"{{"id":"{id}","name":"{name}","raw_input":{raw}}}"#);
    let result = format!(
        r#"{{"tool_use_id":"{id}","tool_name":"{name}","is_error":{is_error},"content":{content},"output":null}}"#
    );

    let mut message = String::from(r#"{"Agent":{"content":[{"ToolUse":"#);
    message.push_str(&tool_use);
    message.push_str(r#"}],"tool_results":{"#);
    message.push('"');
    message.push_str(id);
    message.push_str(r#"":"#);
    message.push_str(&result);
    message.push_str(r#"},"reasoning_details":null}}"#);
    message
}

fn sample_store(prefix: &str, messages: &str) -> (TempDir, PathBuf) {
    let temp = TempDir::new(prefix);
    let db_path = temp.path().join("threads.db");
    let connection = create_db(&db_path);
    let thread = thread_json("alpha", "2026-10-17T10:00:00Z", messages, "");
    insert_thread(
        &connection,
        "alpha",
        "alpha",
        "2026-10-17T10:00:00+00:00",
        None,
        "zstd",
        &compress(&thread),
    );
    drop(connection);
    (temp, db_path)
}

#[test]
fn counts_errors_and_empty_results_apart() {
    let messages = format!(
        "[{}, {}, {}]",
        USER,
        agent(
            "t1",
            "read_file",
            r#"{"path":"/repo/a.rs"}"#,
            Some("contents"),
            true
        ),
        agent("t2", "terminal", r#"{"command":"true"}"#, None, false),
    );
    let (_temp, db_path) = sample_store("zed-thread-behaviour-dead-ends", &messages);

    let report = behaviour(&db_path, "alpha", 0).expect("behaviour succeeds");

    assert_eq!(report.tool_calls, 2);
    assert_eq!(report.tool_results, 2);
    assert_eq!(report.dead_ends.errors, 1);
    assert_eq!(report.dead_ends.empty, 1);
    assert_eq!(report.dead_ends.sites.len(), 2);
    assert_eq!(report.dead_ends.sites[0].message, 1);
    assert_eq!(report.dead_ends.sites[0].tool, "read_file");
    assert_eq!(report.dead_ends.sites[0].kind, DeadEndKind::Error);
    assert_eq!(report.dead_ends.sites[1].message, 2);
    assert_eq!(report.dead_ends.sites[1].tool, "terminal");
    assert_eq!(report.dead_ends.sites[1].kind, DeadEndKind::Empty);
}

#[test]
fn counts_repeats_but_not_a_reread_after_a_write() {
    let path = r#"{"path":"/repo/a.rs"}"#;
    let messages = format!(
        "[{}, {}, {}, {}, {}, {}]",
        USER,
        agent("t1", "read_file", path, Some("one"), false),
        agent("t2", "read_file", path, Some("two"), false),
        agent("t3", "edit_file", path, Some("ok"), false),
        agent("t4", "read_file", path, Some("three"), false),
        agent("t5", "read_file", path, Some("four"), false),
    );
    let (_temp, db_path) = sample_store("zed-thread-behaviour-repeats", &messages);

    let report = behaviour(&db_path, "alpha", 0).expect("behaviour succeeds");

    // t2 repeats t1. t4 follows an edit to the same path, so it is a re-read and
    // becomes the new first sighting. t5 repeats t4.
    assert_eq!(report.repetitions.repeated, 2);
    assert_eq!(report.repetitions.sites.len(), 2);
    assert_eq!(report.repetitions.sites[0].message, 2);
    assert_eq!(report.repetitions.sites[0].first_message, 1);
    assert_eq!(report.repetitions.sites[1].message, 5);
    assert_eq!(report.repetitions.sites[1].first_message, 4);
}

#[test]
fn counts_shell_habits_by_name() {
    let messages = format!(
        "[{}, {}, {}, {}, {}]",
        USER,
        agent(
            "t1",
            "terminal",
            r#"{"command":"cargo test 2>&1 | tail -5"}"#,
            Some("ok"),
            false
        ),
        agent(
            "t2",
            "terminal",
            r#"{"command":"rg foo | head -20"}"#,
            Some("ok"),
            false
        ),
        agent(
            "t3",
            "terminal",
            r#"{"command":"echo \"=== status ===\""}"#,
            Some("ok"),
            false
        ),
        agent(
            "t4",
            "terminal",
            r#"{"command":"cargo fmt"}"#,
            Some("ok"),
            false
        ),
    );
    let (_temp, db_path) = sample_store("zed-thread-behaviour-habits", &messages);

    let report = behaviour(&db_path, "alpha", 0).expect("behaviour succeeds");

    assert_eq!(report.habits.pipe_head, 1);
    assert_eq!(report.habits.pipe_tail, 1);
    assert_eq!(report.habits.separator_echo, 1);
    assert_eq!(report.habits.sites.len(), 3);
}

#[test]
fn counts_printed_exits_and_whether_they_were_followed() {
    let text = "# next: at-recall diff --patch · the hunks\n\
                # next: at-peek slice src/lib.rs:1-20 · the lines";
    let messages = format!(
        "[{}, {}, {}, {}]",
        USER,
        agent(
            "t1",
            "terminal",
            r#"{"command":"at-recall state"}"#,
            Some(text),
            false
        ),
        agent(
            "t2",
            "terminal",
            r#"{"command":"at-recall diff --patch"}"#,
            Some("diff"),
            false
        ),
        agent(
            "t3",
            "terminal",
            r#"{"command":"cargo fmt"}"#,
            Some("ok"),
            false
        ),
    );
    let (_temp, db_path) = sample_store("zed-thread-behaviour-exits", &messages);

    let report = behaviour(&db_path, "alpha", 0).expect("behaviour succeeds");

    assert_eq!(report.exits.printed, 2);
    assert_eq!(report.exits.followed, 1);
    assert_eq!(report.exits.sites.len(), 2);
    assert_eq!(report.exits.sites[0].command, "at-recall diff --patch");
    assert!(report.exits.sites[0].followed);
    let next = report.exits.sites[0].next.as_ref().expect("next call");
    assert_eq!(next.tool, "terminal");
    assert_eq!(next.argument.as_deref(), Some("at-recall diff --patch"));
    assert_eq!(
        report.exits.sites[1].command,
        "at-peek slice src/lib.rs:1-20"
    );
    assert!(!report.exits.sites[1].followed);
}

#[test]
fn limit_truncates_sites_but_not_counts() {
    let path = r#"{"path":"/repo/a.rs"}"#;
    let messages = format!(
        "[{}, {}, {}, {}]",
        USER,
        agent("t1", "read_file", path, Some("one"), false),
        agent("t2", "read_file", path, Some("two"), false),
        agent("t3", "read_file", path, Some("three"), false),
    );
    let (_temp, db_path) = sample_store("zed-thread-behaviour-limit", &messages);

    let report = behaviour(&db_path, "alpha", 1).expect("behaviour succeeds");

    assert_eq!(report.repetitions.repeated, 2);
    assert_eq!(report.repetitions.sites.len(), 1);
}

#[test]
fn does_not_count_repeated_edits() {
    let path = r#"{"path":"/repo/a.rs"}"#;
    let messages = format!(
        "[{}, {}, {}, {}]",
        USER,
        agent("t1", "edit_file", path, Some("ok"), false),
        agent("t2", "edit_file", path, Some("ok"), false),
        agent("t3", "edit_file", path, Some("ok"), false),
    );
    let (_temp, db_path) = sample_store("zed-thread-behaviour-edits", &messages);

    let report = behaviour(&db_path, "alpha", 0).expect("behaviour succeeds");

    assert_eq!(report.repetitions.repeated, 0);
    assert!(report.repetitions.sites.is_empty());
}

#[test]
fn does_not_count_a_shell_observation_after_an_edit() {
    let command = r#"{"command":"cargo test"}"#;
    let path = r#"{"path":"/repo/a.rs"}"#;
    let messages = format!(
        "[{}, {}, {}, {}]",
        USER,
        agent("t1", "terminal", command, Some("ok"), false),
        agent("t2", "edit_file", path, Some("ok"), false),
        agent("t3", "terminal", command, Some("ok"), false),
    );
    let (_temp, db_path) = sample_store("zed-thread-behaviour-shell-repeat", &messages);

    let report = behaviour(&db_path, "alpha", 0).expect("behaviour succeeds");

    assert_eq!(report.repetitions.repeated, 0);
}

#[test]
fn a_successful_write_is_not_an_empty_dead_end() {
    let path = r#"{"path":"/repo/a.rs"}"#;
    let messages = format!(
        "[{}, {}, {}]",
        USER,
        agent("t1", "write_file", path, None, false),
        agent("t2", "read_file", path, None, false),
    );
    let (_temp, db_path) = sample_store("zed-thread-behaviour-write-empty", &messages);

    let report = behaviour(&db_path, "alpha", 0).expect("behaviour succeeds");

    assert_eq!(report.dead_ends.errors, 0);
    assert_eq!(report.dead_ends.empty, 1);
    assert_eq!(report.dead_ends.sites[0].tool, "read_file");
    assert_eq!(report.dead_ends.sites[0].kind, DeadEndKind::Empty);
}
