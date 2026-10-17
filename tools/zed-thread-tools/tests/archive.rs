mod common;

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use zed_thread_tools::archive::{self, BundleManifest};
use zed_thread_tools::db;
use zed_thread_tools::metadata::{self, SidebarThread};
use zed_thread_tools::Error;

use common::{
    compress, create_db, create_sidebar_db, insert_sidebar_thread, insert_thread_with_parent,
    sample_messages, thread_json, TempDir,
};

struct Fixture {
    _temp: TempDir,
    threads_db: PathBuf,
    zed_db: PathBuf,
    archive_dir: PathBuf,
    sidebar: SidebarThread,
}

fn fixture() -> Fixture {
    let temp = TempDir::new("zed-thread-archive");
    let threads_db = temp.path().join("threads.db");
    let zed_db = temp.path().join("db.sqlite");
    let archive_dir = temp.path().join("archives");

    let threads = create_db(&threads_db);
    let root_json = thread_json("root thread", "2026-10-17T10:00:00Z", sample_messages(), "");
    let child_json = thread_json(
        "child thread",
        "2026-10-17T11:00:00Z",
        sample_messages(),
        "",
    );
    insert_thread_with_parent(
        &threads,
        "thread-1",
        None,
        "root thread",
        "2026-10-17T10:00:00+00:00",
        Some("/repo/project"),
        "zstd",
        &compress(&root_json),
    );
    insert_thread_with_parent(
        &threads,
        "thread-1-child",
        Some("thread-1"),
        "child thread",
        "2026-10-17T11:00:00+00:00",
        Some("/repo/project"),
        "zstd",
        &compress(&child_json),
    );
    drop(threads);

    let zed = create_sidebar_db(&zed_db);
    insert_sidebar_thread(
        &zed,
        &[7_u8; 16],
        Some("thread-1"),
        "root thread",
        "2026-10-17T10:00:00+00:00",
        false,
    );
    drop(zed);

    let connection = metadata::open_read_only(&zed_db).expect("open sidebar store");
    let sidebar = metadata::list_sidebar_threads(&connection)
        .expect("list sidebar")
        .remove(0);

    Fixture {
        _temp: temp,
        threads_db,
        zed_db,
        archive_dir,
        sidebar,
    }
}

#[test]
fn export_delete_import_round_trips_native_tree_and_sidebar() {
    let fixture = fixture();
    let original = load_thread(&fixture.threads_db, "thread-1").data;

    let exported = archive::export_thread(
        &fixture.threads_db,
        &fixture.zed_db,
        &fixture.archive_dir,
        Some(&fixture.sidebar),
        Some("thread-1"),
    )
    .expect("export");

    assert_eq!(exported.threads, 2);
    assert_eq!(exported.sidebar_threads, 1);
    assert!(exported.bundle_dir.join("manifest.json").exists());

    let manifest: BundleManifest = serde_json::from_slice(
        &std::fs::read(exported.bundle_dir.join("manifest.json")).expect("read manifest"),
    )
    .expect("parse manifest");
    assert_eq!(manifest.format_version, archive::BUNDLE_FORMAT_VERSION);
    assert_eq!(manifest.threads.len(), 2);
    assert!(manifest
        .threads
        .iter()
        .any(|thread| thread.parent_id.as_deref() == Some("thread-1")));

    let deleted = archive::delete_thread(
        &fixture.threads_db,
        &fixture.zed_db,
        Some(&fixture.sidebar),
        Some("thread-1"),
        true,
    )
    .expect("delete");
    assert_eq!(deleted.threads, 2);
    assert_eq!(deleted.sidebar_threads, 1);
    assert_eq!(count_threads(&fixture.threads_db), 0);
    assert_eq!(count_sidebar(&fixture.zed_db), 0);

    let imported = archive::import_bundle(
        &fixture.threads_db,
        &fixture.zed_db,
        &exported.bundle_dir,
        true,
    )
    .expect("import");
    assert_eq!(imported.threads, 2);
    assert_eq!(imported.sidebar_threads, 1);
    assert_eq!(count_threads(&fixture.threads_db), 2);
    assert_eq!(count_sidebar(&fixture.zed_db), 1);
    assert_eq!(load_thread(&fixture.threads_db, "thread-1").data, original);

    match archive::import_bundle(
        &fixture.threads_db,
        &fixture.zed_db,
        &exported.bundle_dir,
        true,
    ) {
        Err(Error::ThreadConflict(id)) => assert_eq!(id, "thread-1"),
        other => panic!("expected ThreadConflict, got {other:?}"),
    }
}

#[test]
fn exporting_a_subagent_includes_its_ancestors() {
    let fixture = fixture();
    let exported = archive::export_thread(
        &fixture.threads_db,
        &fixture.zed_db,
        &fixture.archive_dir,
        None,
        Some("thread-1-child"),
    )
    .expect("export subagent");
    assert_eq!(exported.threads, 2);

    let manifest: BundleManifest = serde_json::from_slice(
        &std::fs::read(exported.bundle_dir.join("manifest.json")).expect("read manifest"),
    )
    .expect("parse manifest");
    assert_eq!(manifest.root_thread_id.as_deref(), Some("thread-1"));
    assert!(manifest
        .threads
        .iter()
        .any(|thread| thread.id == "thread-1-child"));
}

#[test]
fn sidebar_only_bundle_round_trips_metadata() {
    let fixture = fixture();
    let zed = Connection::open(&fixture.zed_db).expect("open sidebar");
    insert_sidebar_thread(
        &zed,
        &[9_u8; 16],
        Some("claude-1"),
        "external thread",
        "2026-10-17T12:00:00+00:00",
        false,
    );
    drop(zed);

    let connection = metadata::open_read_only(&fixture.zed_db).expect("open sidebar store");
    let external = metadata::list_sidebar_threads(&connection)
        .expect("list sidebar")
        .into_iter()
        .find(|sidebar| sidebar.session_id.as_deref() == Some("claude-1"))
        .expect("external sidebar row");

    let exported = archive::export_thread(
        &fixture.threads_db,
        &fixture.zed_db,
        &fixture.archive_dir,
        Some(&external),
        None,
    )
    .expect("export external");
    assert_eq!(exported.threads, 0);
    assert_eq!(exported.sidebar_threads, 1);

    let deleted = archive::delete_thread(
        &fixture.threads_db,
        &fixture.zed_db,
        Some(&external),
        None,
        true,
    )
    .expect("delete external");
    assert_eq!(deleted.threads, 0);
    assert_eq!(deleted.sidebar_threads, 1);

    archive::import_bundle(
        &fixture.threads_db,
        &fixture.zed_db,
        &exported.bundle_dir,
        true,
    )
    .expect("import external");
    let rows = metadata::list_sidebar_threads(
        &metadata::open_read_only(&fixture.zed_db).expect("open sidebar store"),
    )
    .expect("list sidebar");
    assert!(rows
        .iter()
        .any(|sidebar| sidebar.session_id.as_deref() == Some("claude-1")));
}

#[test]
fn archive_toggle_updates_the_flag_and_makes_a_backup() {
    let fixture = fixture();
    let backup = metadata::set_archived(&fixture.zed_db, &fixture.sidebar.thread_id, true, true)
        .expect("archive");
    assert!(backup.exists());

    let rows = metadata::list_sidebar_threads(
        &metadata::open_read_only(&fixture.zed_db).expect("open sidebar store"),
    )
    .expect("list sidebar");
    assert!(rows[0].archived);

    metadata::set_archived(&fixture.zed_db, &fixture.sidebar.thread_id, false, true)
        .expect("unarchive");
    let rows = metadata::list_sidebar_threads(
        &metadata::open_read_only(&fixture.zed_db).expect("open sidebar store"),
    )
    .expect("list sidebar");
    assert!(!rows[0].archived);
}

fn load_thread(threads_db: &Path, id: &str) -> zed_thread_tools::ThreadRecord {
    let connection = db::open_read_only(threads_db).expect("open read-only");
    db::load_record(&connection, id).expect("load thread")
}

fn count_threads(threads_db: &Path) -> i64 {
    Connection::open(threads_db)
        .expect("open threads db")
        .query_row("SELECT count(*) FROM threads", [], |row| row.get(0))
        .expect("count threads")
}

fn count_sidebar(zed_db: &Path) -> i64 {
    Connection::open(zed_db)
        .expect("open sidebar db")
        .query_row("SELECT count(*) FROM sidebar_threads", [], |row| row.get(0))
        .expect("count sidebar")
}
