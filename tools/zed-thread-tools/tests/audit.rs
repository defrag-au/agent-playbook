mod common;

use chrono::NaiveDate;
use zed_thread_tools::audit::{audit, AuditOptions};
use zed_thread_tools::{AuditReport, ThreadAudit};

use common::{
    compress, create_db, insert_thread, legacy_messages, sample_messages, thread_json, TempDir,
};

#[test]
fn counts_messages_tools_and_both_content_shapes() {
    let temp = TempDir::new("zed-thread-audit-counts");
    let db_path = temp.path().join("threads.db");
    let connection = create_db(&db_path);

    let alpha = thread_json("alpha", "2026-10-16T10:00:00Z", sample_messages(), "");
    insert_thread(
        &connection,
        "alpha",
        "alpha",
        "2026-10-16T10:00:00+00:00",
        Some("/Users/test/project-a\n/Users/test/project-b"),
        "zstd",
        &compress(&alpha),
    );

    let beta = thread_json("beta", "2026-10-17T10:00:00Z", legacy_messages(), "");
    insert_thread(
        &connection,
        "beta",
        "beta",
        "2026-10-17T10:00:00+00:00",
        Some("/Users/test/legacy"),
        "zstd",
        &compress(&beta),
    );

    let gamma_messages = with_compaction(sample_messages());
    let gamma = thread_json("gamma", "2026-10-17T11:00:00Z", &gamma_messages, "");
    insert_thread(
        &connection,
        "gamma",
        "gamma",
        "2026-10-17T11:00:00+00:00",
        Some("/Users/test/project-a"),
        "zstd",
        &compress(&gamma),
    );
    drop(connection);

    let report = audit(
        &db_path,
        &AuditOptions {
            top: 0,
            older_than: None,
            project: None,
        },
    )
    .expect("audit succeeds");

    assert_eq!(report.total_threads, 3);
    assert_eq!(report.matched_threads, 3);
    assert_eq!(report.failed_threads, 0);

    let alpha = find(&report, "alpha");
    assert_eq!(alpha.messages, Some(2));
    assert_eq!(alpha.user_messages, Some(1));
    assert_eq!(alpha.agent_messages, Some(1));
    assert_eq!(alpha.tool_calls, Some(1));
    assert_eq!(alpha.tool_results, Some(1));
    assert_eq!(alpha.failed_tool_results, Some(0));
    assert_eq!(alpha.tool_output_bytes, Some(10));
    assert_eq!(alpha.compactions, Some(0));
    assert_eq!(alpha.messages_since_compaction, Some(2));
    assert_eq!(alpha.model.as_deref(), Some("test/fake"));
    assert_eq!(alpha.project_paths.len(), 2);
    assert!(alpha.error.is_none());

    let beta = find(&report, "beta");
    assert_eq!(beta.tool_output_bytes, Some(10));
    assert_eq!(beta.messages, Some(2));
    assert!(beta.error.is_none());

    let gamma = find(&report, "gamma");
    assert_eq!(gamma.messages, Some(3));
    assert_eq!(gamma.compactions, Some(1));
    assert_eq!(gamma.messages_since_compaction, Some(0));
}

#[test]
fn filters_by_project_and_date() {
    let temp = TempDir::new("zed-thread-audit-filters");
    let db_path = temp.path().join("threads.db");
    let connection = create_db(&db_path);

    let older = thread_json("older", "2026-10-15T10:00:00Z", sample_messages(), "");
    insert_thread(
        &connection,
        "older",
        "older",
        "2026-10-15T10:00:00+00:00",
        Some("/Users/test/project-a"),
        "zstd",
        &compress(&older),
    );

    let newer = thread_json("newer", "2026-10-17T10:00:00Z", sample_messages(), "");
    insert_thread(
        &connection,
        "newer",
        "newer",
        "2026-10-17T10:00:00+00:00",
        Some("/Users/test/project-b"),
        "zstd",
        &compress(&newer),
    );
    drop(connection);

    let by_project = audit(
        &db_path,
        &AuditOptions {
            top: 0,
            older_than: None,
            project: Some("PROJECT-A".to_owned()),
        },
    )
    .expect("audit by project");
    assert_eq!(by_project.matched_threads, 1);
    assert_eq!(by_project.threads[0].id, "older");

    let cutoff = NaiveDate::from_ymd_opt(2026, 10, 17).expect("valid date");
    let by_date = audit(
        &db_path,
        &AuditOptions {
            top: 0,
            older_than: Some(cutoff),
            project: None,
        },
    )
    .expect("audit by date");
    assert_eq!(by_date.matched_threads, 1);
    assert_eq!(by_date.threads[0].id, "older");
}

#[test]
fn reports_a_bad_blob_without_failing_the_run() {
    let temp = TempDir::new("zed-thread-audit-bad-blob");
    let db_path = temp.path().join("threads.db");
    let connection = create_db(&db_path);

    let good = thread_json("good", "2026-10-17T10:00:00Z", sample_messages(), "");
    insert_thread(
        &connection,
        "good",
        "good",
        "2026-10-17T10:00:00+00:00",
        None,
        "zstd",
        &compress(&good),
    );
    insert_thread(
        &connection,
        "bad",
        "bad",
        "2026-10-17T10:01:00+00:00",
        None,
        "zstd",
        b"this is not a zstd frame",
    );
    drop(connection);

    let report = audit(
        &db_path,
        &AuditOptions {
            top: 0,
            older_than: None,
            project: None,
        },
    )
    .expect("audit succeeds despite one bad thread");

    assert_eq!(report.failed_threads, 1);
    assert!(find(&report, "good").error.is_none());
    let bad = find(&report, "bad");
    assert!(
        bad.error
            .as_deref()
            .is_some_and(|error| error.contains("decode")),
        "expected a decode error, got {:?}",
        bad.error
    );
}

#[test]
fn audit_does_not_modify_the_database() {
    let temp = TempDir::new("zed-thread-audit-read-only");
    let db_path = temp.path().join("threads.db");
    let connection = create_db(&db_path);
    let thread = thread_json("alpha", "2026-10-17T10:00:00Z", sample_messages(), "");
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

    let before = std::fs::read(&db_path).expect("read database before audit");
    audit(
        &db_path,
        &AuditOptions {
            top: 0,
            older_than: None,
            project: None,
        },
    )
    .expect("audit succeeds");
    let after = std::fs::read(&db_path).expect("read database after audit");

    assert_eq!(before, after);
}

fn find<'a>(report: &'a AuditReport, id: &str) -> &'a ThreadAudit {
    report
        .threads
        .iter()
        .find(|thread| thread.id == id)
        .unwrap_or_else(|| panic!("thread {id} missing from report"))
}

fn with_compaction(messages: &str) -> String {
    let mut messages = messages.trim_end().to_owned();
    assert!(messages.ends_with(']'));
    messages.pop();
    messages.push_str(r#",{"Compaction":{"Summary":"earlier context"}}]"#);
    messages
}
