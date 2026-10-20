mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::DateTime;
use serde_json::value::RawValue;
use zed_thread_tools::db;
use zed_thread_tools::recover::{prepare_recovery, write_recovery, RecoverOptions};
use zed_thread_tools::{Error, ThreadRecord};

use common::{compress, create_db, insert_thread, sample_messages, thread_json, TempDir};

#[test]
fn appends_before_a_pending_user_message_and_preserves_messages() {
    let (temp, _db_path, record) = fixture("", &messages_with_pending_user());
    let original_messages = decode_messages(&record.data);
    let original_raw: Vec<&str> = original_messages
        .iter()
        .map(|message| message.get())
        .collect();

    let prepared = prepare_recovery(&record, "test summary", None, false).expect("prepare");

    assert_eq!(prepared.plan.messages_before, 3);
    assert_eq!(prepared.plan.messages_after, 4);
    assert_eq!(prepared.plan.insertion_index, 2);

    let fields = decode_fields(&prepared.new_data);
    let messages = messages_from(&fields);
    assert_eq!(messages.len(), 4);
    assert_eq!(
        messages[2].get(),
        r#"{"Compaction":{"Summary":"test summary"}}"#
    );
    assert_eq!(messages[3].get(), original_raw[2]);

    let mut without_compaction = messages
        .iter()
        .map(|message| message.get())
        .collect::<Vec<_>>();
    without_compaction.remove(prepared.plan.insertion_index);
    assert_eq!(without_compaction, original_raw);

    drop(temp);
}

#[test]
fn appends_at_the_end_when_the_last_message_is_an_agent_message() {
    let (_temp, _db_path, record) = fixture("", sample_messages());
    let prepared = prepare_recovery(&record, "test summary", None, false).expect("prepare");

    assert_eq!(prepared.plan.messages_before, 2);
    assert_eq!(prepared.plan.messages_after, 3);
    assert_eq!(prepared.plan.insertion_index, 2);

    let messages = messages_from(&decode_fields(&prepared.new_data));
    assert_eq!(
        messages[2].get(),
        r#"{"Compaction":{"Summary":"test summary"}}"#
    );
}

#[test]
fn preserves_unknown_top_level_fields() {
    let (_temp, _db_path, record) = fixture(r#","future_field":{"x":1}"#, sample_messages());
    let prepared = prepare_recovery(&record, "test summary", None, false).expect("prepare");
    let fields = decode_fields(&prepared.new_data);

    assert_eq!(fields["future_field"].get(), r#"{"x":1}"#);
}

#[test]
fn refuses_an_unknown_thread_version() {
    let temp = TempDir::new("zed-thread-recover-version");
    let db_path = temp.path().join("threads.db");
    let connection = create_db(&db_path);
    let json = thread_json("test thread", "2026-10-17T10:00:00Z", sample_messages(), "")
        .replace(r#""version":"0.3.0""#, r#""version":"9.9.9""#);
    insert_thread(
        &connection,
        "thread-1",
        "test thread",
        "2026-10-17T10:00:00+00:00",
        None,
        "zstd",
        &compress(&json),
    );
    drop(connection);

    let record = load(&db_path, "thread-1");
    match prepare_recovery(&record, "test summary", None, false) {
        Err(Error::UnsupportedVersion(version)) => assert_eq!(version, "9.9.9"),
        other => panic!("expected UnsupportedVersion, got {other:?}"),
    }
}

#[test]
fn in_place_write_updates_the_row_and_makes_a_backup() {
    let (temp, db_path, record) = fixture("", sample_messages());
    let backup = temp.path().join("backup.db");
    let prepared = prepare_recovery(&record, "test summary", None, false).expect("prepare");

    let outcome = write_recovery(
        &db_path,
        &record,
        &prepared,
        &options("thread-1", "test summary", false, backup.clone()),
    )
    .expect("write recovery");

    assert!(!outcome.created);
    assert_eq!(outcome.thread_id, "thread-1");
    assert_eq!(outcome.backup, backup);
    assert!(backup.exists());

    let updated = load(&db_path, "thread-1");
    assert_ne!(updated.data, record.data);
    let updated_messages = decode_messages(&updated.data);
    assert_eq!(
        updated_messages
            .iter()
            .filter(|message| message.get().contains(r#""Compaction""#))
            .count(),
        1
    );

    let original_time = DateTime::parse_from_rfc3339(&record.header.updated_at)
        .expect("old timestamp")
        .with_timezone(&chrono::Utc);
    let updated_time = DateTime::parse_from_rfc3339(&updated.header.updated_at)
        .expect("new timestamp")
        .with_timezone(&chrono::Utc);
    assert!(updated_time > original_time);

    let backup_connection = rusqlite::Connection::open(&backup).expect("open backup");
    let count: i64 = backup_connection
        .query_row("SELECT count(*) FROM threads", [], |row| row.get(0))
        .expect("count backup threads");
    assert_eq!(count, 1);
}

#[test]
fn fork_write_leaves_the_original_untouched() {
    let (temp, db_path, record) = fixture("", sample_messages());
    let original_data = record.data.clone();
    let backup = temp.path().join("backup.db");
    let prepared = prepare_recovery(&record, "test summary", None, true).expect("prepare");

    let outcome = write_recovery(
        &db_path,
        &record,
        &prepared,
        &options("thread-1", "test summary", true, backup),
    )
    .expect("write fork");

    assert!(outcome.created);
    assert_ne!(outcome.thread_id, "thread-1");

    let unchanged = load(&db_path, "thread-1");
    assert_eq!(unchanged.data, original_data);

    let forked = load(&db_path, &outcome.thread_id);
    assert_eq!(forked.header.parent_id, None);
    assert_eq!(forked.header.folder_paths, record.header.folder_paths);
    assert!(forked.header.summary.ends_with("(recovered)"));

    let fields = decode_fields(&forked.data);
    assert_eq!(
        serde_json::from_str::<String>(fields["detailed_summary"].get()).expect("summary string"),
        "test summary"
    );
    assert!(messages_from(&fields)
        .iter()
        .any(|message| message.get().contains(r#""Compaction""#)));
}

#[test]
fn refuses_to_overwrite_an_existing_backup() {
    let (temp, db_path, record) = fixture("", sample_messages());
    let backup = temp.path().join("backup.db");
    std::fs::write(&backup, b"existing").expect("write existing backup");
    let prepared = prepare_recovery(&record, "test summary", None, false).expect("prepare");
    let before = std::fs::read(&db_path).expect("read database before write");

    match write_recovery(
        &db_path,
        &record,
        &prepared,
        &options("thread-1", "test summary", false, backup.clone()),
    ) {
        Err(Error::BackupExists(path)) => assert_eq!(path, backup),
        other => panic!("expected BackupExists, got {other:?}"),
    }

    assert_eq!(
        std::fs::read(&db_path).expect("read database after write"),
        before
    );
}

fn fixture(extra: &str, messages: &str) -> (TempDir, PathBuf, ThreadRecord) {
    let temp = TempDir::new("zed-thread-recover");
    let db_path = temp.path().join("threads.db");
    let connection = create_db(&db_path);
    let json = thread_json("test thread", "2026-10-16T10:00:00Z", messages, extra);
    insert_thread(
        &connection,
        "thread-1",
        "test thread",
        "2026-10-16T10:00:00+00:00",
        Some("/Users/test/project"),
        "zstd",
        &compress(&json),
    );
    drop(connection);
    let record = load(&db_path, "thread-1");
    (temp, db_path, record)
}

fn load(db_path: &Path, id: &str) -> ThreadRecord {
    let connection = db::open_read_only(db_path).expect("open read-only");
    db::load_record(&connection, id).expect("load thread")
}

fn options(thread_id: &str, summary: &str, fork: bool, backup: PathBuf) -> RecoverOptions {
    RecoverOptions {
        thread_id: thread_id.to_owned(),
        summary: summary.to_owned(),
        title: None,
        fork,
        dry_run: false,
        backup: Some(backup),
        allow_running: true,
    }
}

fn decode_fields(data: &[u8]) -> BTreeMap<String, Box<RawValue>> {
    let decoded = zstd::stream::decode_all(data).expect("decompress thread");
    serde_json::from_slice(&decoded).expect("parse thread fields")
}

fn messages_from(fields: &BTreeMap<String, Box<RawValue>>) -> Vec<Box<RawValue>> {
    serde_json::from_str(fields["messages"].get()).expect("parse messages")
}

fn decode_messages(data: &[u8]) -> Vec<Box<RawValue>> {
    messages_from(&decode_fields(data))
}

fn messages_with_pending_user() -> String {
    let mut messages = sample_messages().trim_end().to_owned();
    assert!(messages.ends_with(']'));
    messages.pop();
    messages.push_str(r#",{"User":{"id":"u2","content":[{"Text":"still pending"}]}}]"#);
    messages
}
