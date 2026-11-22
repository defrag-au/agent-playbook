mod common;

use zed_thread_tools::export::{export, ExportOptions, SkipReason, SnapshotManifest};
use zed_thread_tools::Error;

use common::{
    compress, create_db, insert_thread, insert_thread_with_parent, sample_messages, thread_json,
    TempDir,
};

fn messages_with_text(text: &str) -> String {
    format!(r#"[{{"User":{{"id":"u1","content":[{{"Text":"{text}"}}]}}}}]"#)
}

#[test]
fn exports_a_whole_tree_with_decoded_documents() {
    let temp = TempDir::new("zed-thread-export-tree");
    let db_path = temp.path().join("threads.db");
    let connection = create_db(&db_path);

    let root_json = thread_json("root title", "2026-10-20T10:00:00Z", sample_messages(), "");
    insert_thread(
        &connection,
        "root",
        "root summary",
        "2026-10-20T10:00:00+00:00",
        Some("/repo/a"),
        "zstd",
        &compress(&root_json),
    );

    let child_json = thread_json("child title", "2026-10-20T09:00:00Z", sample_messages(), "");
    insert_thread_with_parent(
        &connection,
        "child",
        Some("root"),
        "child summary",
        "2026-10-20T09:00:00+00:00",
        Some("/repo/a"),
        "zstd",
        &compress(&child_json),
    );
    drop(connection);

    let output = temp.path().join("snapshot");
    let outcome = export(&db_path, &output, &ExportOptions { max_bytes: 0 }).expect("export");

    assert_eq!(outcome.threads, 2);
    assert_eq!(outcome.trees, 1);
    assert_eq!(outcome.bytes, (root_json.len() + child_json.len()) as u64);

    let manifest: SnapshotManifest =
        serde_json::from_slice(&std::fs::read(output.join("manifest.json")).expect("manifest"))
            .expect("parse manifest");

    assert_eq!(manifest.format_version, 1);
    assert_eq!(manifest.selection.included_trees, 1);
    assert_eq!(manifest.selection.included_bytes, outcome.bytes);
    assert_eq!(manifest.threads.len(), 2);
    assert!(manifest.skipped.is_empty());

    let root = manifest
        .threads
        .iter()
        .find(|thread| thread.id == "root")
        .expect("root in manifest");
    assert_eq!(root.parent_id, None);
    assert_eq!(root.root_id, "root");
    assert_eq!(root.title, "root title");
    assert_eq!(root.summary, "root summary");
    assert_eq!(root.model.as_deref(), Some("test/fake"));
    assert_eq!(root.project_paths, vec!["/repo/a"]);
    assert_eq!(root.messages, 2);
    assert_eq!(root.version.as_deref(), Some("0.3.0"));
    assert_eq!(root.file, "threads/root.json");
    assert_eq!(
        std::fs::read(output.join(&root.file)).expect("root document"),
        root_json.as_bytes()
    );

    let child = manifest
        .threads
        .iter()
        .find(|thread| thread.id == "child")
        .expect("child in manifest");
    assert_eq!(child.parent_id.as_deref(), Some("root"));
    assert_eq!(child.root_id, "root");

    let readme = std::fs::read_to_string(output.join("README.md")).expect("readme");
    assert!(readme.contains("unredacted"));
    assert!(readme.contains("threads/<thread-id>.json"));
}

#[test]
fn skips_a_whole_tree_over_the_budget() {
    let temp = TempDir::new("zed-thread-export-budget");
    let db_path = temp.path().join("threads.db");
    let connection = create_db(&db_path);

    let big_text = "x".repeat(50_000);
    let big_json = thread_json(
        "big",
        "2026-10-21T10:00:00Z",
        &messages_with_text(&big_text),
        "",
    );
    insert_thread(
        &connection,
        "big",
        "big",
        "2026-10-21T10:00:00+00:00",
        Some("/repo/big"),
        "zstd",
        &compress(&big_json),
    );

    let small_json = thread_json("small", "2026-10-20T10:00:00Z", sample_messages(), "");
    insert_thread(
        &connection,
        "small",
        "small",
        "2026-10-20T10:00:00+00:00",
        Some("/repo/small"),
        "zstd",
        &compress(&small_json),
    );
    drop(connection);

    let output = temp.path().join("snapshot");
    let outcome = export(
        &db_path,
        &output,
        &ExportOptions {
            max_bytes: small_json.len() as u64 + 10,
        },
    )
    .expect("export");

    assert_eq!(outcome.threads, 1);
    assert_eq!(outcome.skipped_over_budget, 1);
    assert!(output.join("threads/small.json").exists());
    assert!(!output.join("threads/big.json").exists());

    let manifest: SnapshotManifest =
        serde_json::from_slice(&std::fs::read(output.join("manifest.json")).expect("manifest"))
            .expect("parse manifest");
    assert_eq!(manifest.skipped.len(), 1);
    assert_eq!(manifest.skipped[0].root_id, "big");
    assert_eq!(manifest.skipped[0].reason, SkipReason::OverBudget);
}

#[test]
fn never_exports_part_of_a_thread_tree() {
    let temp = TempDir::new("zed-thread-export-partial-tree");
    let db_path = temp.path().join("threads.db");
    let connection = create_db(&db_path);

    let root_json = thread_json("root", "2026-10-20T10:00:00Z", sample_messages(), "");
    insert_thread(
        &connection,
        "root",
        "root",
        "2026-10-20T10:00:00+00:00",
        Some("/repo/a"),
        "zstd",
        &compress(&root_json),
    );

    let child_json = thread_json("child", "2026-10-20T09:00:00Z", sample_messages(), "");
    insert_thread_with_parent(
        &connection,
        "child",
        Some("root"),
        "child",
        "2026-10-20T09:00:00+00:00",
        Some("/repo/a"),
        "zstd",
        &compress(&child_json),
    );
    drop(connection);

    let output = temp.path().join("snapshot");
    let budget = (root_json.len() + child_json.len()) as u64 - 1;
    let error = export(&db_path, &output, &ExportOptions { max_bytes: budget })
        .expect_err("budget is too small");

    assert!(matches!(error, Error::BudgetTooSmall { .. }));
    assert!(!output.exists());
}

#[test]
fn passes_through_json_rows() {
    let temp = TempDir::new("zed-thread-export-json");
    let db_path = temp.path().join("threads.db");
    let connection = create_db(&db_path);

    let plain_json = thread_json("plain", "2026-10-20T10:00:00Z", sample_messages(), "");
    insert_thread(
        &connection,
        "plain",
        "plain",
        "2026-10-20T10:00:00+00:00",
        Some("/repo/plain"),
        "json",
        plain_json.as_bytes(),
    );
    drop(connection);

    let output = temp.path().join("snapshot");
    export(&db_path, &output, &ExportOptions { max_bytes: 0 }).expect("export");

    assert_eq!(
        std::fs::read(output.join("threads/plain.json")).expect("document"),
        plain_json.as_bytes()
    );
}

#[test]
fn refuses_an_existing_output_path() {
    let temp = TempDir::new("zed-thread-export-exists");
    let db_path = temp.path().join("threads.db");
    let connection = create_db(&db_path);
    let data = thread_json("thread", "2026-10-20T10:00:00Z", sample_messages(), "");
    insert_thread(
        &connection,
        "thread",
        "thread",
        "2026-10-20T10:00:00+00:00",
        Some("/repo/a"),
        "zstd",
        &compress(&data),
    );
    drop(connection);

    let output = temp.path().join("snapshot");
    std::fs::create_dir(&output).expect("existing directory");
    let error =
        export(&db_path, &output, &ExportOptions { max_bytes: 0 }).expect_err("output exists");

    assert!(matches!(error, Error::OutputExists(_)));
}
