//! End-to-end tests over a real store: publish, export, verify, version, and fetch.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use skill_store::export;
use skill_store::fsx;
use skill_store::publish::{self, Options};
use skill_store::source;
use skill_store::status;
use skill_store::store::Store;
use skill_store::verify;

struct Temp {
    path: PathBuf,
}

impl Temp {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("skill-store-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("temp dir");
        Temp { path }
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn write_skill(root: &Path, name: &str, body: &str) -> PathBuf {
    let dir = root.join(name);
    fs::create_dir_all(dir.join("references")).expect("skill dir");
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: A test skill for {name}.\n---\n\n{body}\n"),
    )
    .expect("SKILL.md");
    fs::write(dir.join("references/REFERENCE.md"), "reference\n").expect("reference");
    dir
}

fn open_path(store: &Path) -> Store {
    let source =
        source::parse(&format!("path:{}#widget-screenshot@v1", store.display())).expect("source");
    Store::open(&source, true).expect("store")
}

#[test]
fn publish_then_export_is_byte_identical() {
    let temp = Temp::new("round-trip");
    let store = temp.path.join("store");
    let skill = write_skill(&temp.path, "widget-screenshot", "one");

    let report = publish::publish(
        &skill,
        &store,
        &Options {
            init: true,
            ..Default::default()
        },
    )
    .expect("publish");
    assert_eq!(report.version, "v1");
    assert_eq!(report.added, 2);
    assert!(store.join("0=ocfl_1.1").is_file());
    assert!(store.join("widget-screenshot/0=ocfl_object_1.1").is_file());
    assert!(store
        .join("widget-screenshot/v1/content/SKILL.md")
        .is_file());

    let opened = open_path(&store);
    let out = temp.path.join("export");
    let exported = export::export(&opened, "widget-screenshot", Some("v1"), &out).expect("export");
    assert_eq!(exported.files, 2);
    assert_eq!(
        fs::read_to_string(out.join("skill/SKILL.md")).expect("exported SKILL.md"),
        fs::read_to_string(skill.join("SKILL.md")).expect("source SKILL.md")
    );
    assert_eq!(
        fs::read_to_string(out.join("skill/references/REFERENCE.md")).expect("reference"),
        "reference\n"
    );

    let conf = fsx::read_flat(&out.join("export.conf")).expect("export.conf");
    assert_eq!(
        conf.get("name").map(String::as_str),
        Some("widget-screenshot")
    );
    assert_eq!(conf.get("version").map(String::as_str), Some("v1"));
    assert!(conf
        .get("tree_digest")
        .expect("tree digest")
        .starts_with("sha256:"));
    assert!(conf
        .get("inventory_digest")
        .expect("inventory digest")
        .starts_with("sha512:"));
}

#[test]
fn a_second_version_stores_only_what_changed() {
    let temp = Temp::new("version-two");
    let store = temp.path.join("store");
    let skill = write_skill(&temp.path, "widget-screenshot", "one");
    publish::publish(
        &skill,
        &store,
        &Options {
            init: true,
            ..Default::default()
        },
    )
    .expect("v1");

    fs::write(
        skill.join("SKILL.md"),
        "---\nname: widget-screenshot\ndescription: A test skill for widget-screenshot.\n---\n\ntwo\n",
    )
    .expect("change");
    let report = publish::publish(&skill, &store, &Options::default()).expect("v2");
    assert_eq!(report.version, "v2");
    assert_eq!(report.changed, 1);
    assert_eq!(report.unchanged, 1);
    assert_eq!(report.added, 0);
    assert_eq!(report.removed, 0);

    let object = store.join("widget-screenshot");
    assert!(object.join("v2/content/SKILL.md").is_file());
    assert!(
        !object.join("v2/content/references/REFERENCE.md").exists(),
        "an unchanged file must be deduplicated by digest"
    );

    let opened = open_path(&store);
    let first = temp.path.join("v1");
    export::export(&opened, "widget-screenshot", Some("v1"), &first).expect("v1 export");
    assert!(fs::read_to_string(first.join("skill/SKILL.md"))
        .expect("v1 body")
        .contains("one"));
    let second = temp.path.join("v2");
    export::export(&opened, "widget-screenshot", Some("v2"), &second).expect("v2 export");
    assert!(fs::read_to_string(second.join("skill/SKILL.md"))
        .expect("v2 body")
        .contains("two"));
}

#[test]
fn a_user_block_carries_an_address_or_is_not_written() {
    use skill_store::inventory::Inventory;

    let temp = Temp::new("user-block");
    let store = temp.path.join("store");
    let skill = write_skill(&temp.path, "widget-screenshot", "one");

    publish::publish(
        &skill,
        &store,
        &Options {
            init: true,
            ..Default::default()
        },
    )
    .expect("publish");
    let inventory = Inventory::read(&store.join("widget-screenshot")).expect("inventory");
    let version = inventory.versions.get("v1").expect("v1");
    assert!(
        version.user.is_none(),
        "a store with no repository has no address to record, and a name alone is an OCFL \
         warning (W008) — so nothing is recorded rather than something invented"
    );

    fs::write(
        skill.join("SKILL.md"),
        "---\nname: widget-screenshot\ndescription: A test skill for widget-screenshot.\n---\n\ntwo\n",
    )
    .expect("change");
    publish::publish(
        &skill,
        &store,
        &Options {
            user: Some("tester"),
            address: Some("https://example.com/tester"),
            ..Default::default()
        },
    )
    .expect("v2");
    let inventory = Inventory::read(&store.join("widget-screenshot")).expect("inventory");
    let version = inventory.versions.get("v2").expect("v2");
    let user = version.user.as_ref().expect("user block");
    assert_eq!(user.name, "tester");
    assert_eq!(user.address.as_deref(), Some("https://example.com/tester"));
}

#[test]
fn a_user_with_no_address_and_no_repository_is_refused() {
    let temp = Temp::new("user-no-address");
    let store = temp.path.join("store");
    let skill = write_skill(&temp.path, "widget-screenshot", "one");

    let error = publish::publish(
        &skill,
        &store,
        &Options {
            init: true,
            user: Some("tester"),
            ..Default::default()
        },
    )
    .expect_err("a name with no address is W008");
    assert!(error.contains("--address"), "{error}");
    assert!(
        !store.join("widget-screenshot").exists(),
        "a refused publish must not leave an object behind"
    );
}

#[test]
fn a_store_in_a_repository_records_that_repository() {
    use skill_store::inventory::Inventory;

    let temp = Temp::new("user-block-git");
    let store = temp.path.join("store");
    let skill = write_skill(&temp.path, "widget-screenshot", "one");
    git(&temp.path, &["init", "--quiet"]);
    git(
        &temp.path,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:defrag-au/widget-works.git",
        ],
    );

    publish::publish(
        &skill,
        &store,
        &Options {
            init: true,
            ..Default::default()
        },
    )
    .expect("publish");

    let inventory = Inventory::read(&store.join("widget-screenshot")).expect("inventory");
    let version = inventory.versions.get("v1").expect("v1");
    let user = version.user.as_ref().expect("user block");
    assert_eq!(
        user.address.as_deref(),
        Some("https://github.com/defrag-au/widget-works"),
        "the address is the repository the store lives in, normalised to a URL"
    );
    assert!(
        !user.name.trim().is_empty(),
        "OCFL wants a name as well as an address"
    );
}

#[test]
fn publishing_identical_content_is_refused() {
    let temp = Temp::new("unchanged");
    let store = temp.path.join("store");
    let skill = write_skill(&temp.path, "widget-screenshot", "one");
    publish::publish(
        &skill,
        &store,
        &Options {
            init: true,
            ..Default::default()
        },
    )
    .expect("v1");
    let error = publish::publish(&skill, &store, &Options::default()).expect_err("no-op");
    assert!(error.contains("nothing to publish"), "{error}");
}

#[test]
fn verify_accepts_a_fresh_store_and_catches_tampering() {
    let temp = Temp::new("verify");
    let store = temp.path.join("store");
    let skill = write_skill(&temp.path, "widget-screenshot", "one");
    publish::publish(
        &skill,
        &store,
        &Options {
            init: true,
            ..Default::default()
        },
    )
    .expect("publish");

    let opened = open_path(&store);
    let report = verify::verify(&opened, Some("widget-screenshot"), None).expect("verify");
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.files, 2);

    fs::write(
        store.join("widget-screenshot/v1/content/SKILL.md"),
        "tampered\n",
    )
    .expect("tamper");
    let report = verify::verify(&opened, Some("widget-screenshot"), None).expect("verify");
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.contains("digest is")),
        "{:?}",
        report.errors
    );
}

#[test]
fn verify_catches_an_edited_inventory() {
    let temp = Temp::new("sidecar");
    let store = temp.path.join("store");
    let skill = write_skill(&temp.path, "widget-screenshot", "one");
    publish::publish(
        &skill,
        &store,
        &Options {
            init: true,
            ..Default::default()
        },
    )
    .expect("publish");

    let inventory = store.join("widget-screenshot/inventory.json");
    let mut body = fs::read_to_string(&inventory).expect("inventory");
    body.push(' ');
    fs::write(&inventory, body).expect("edit inventory");

    let opened = open_path(&store);
    let report = verify::verify(&opened, Some("widget-screenshot"), None).expect("verify");
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.contains("inventory digest does not match")),
        "{:?}",
        report.errors
    );
}

#[test]
fn a_repository_root_resolves_a_dot_skill_store() {
    let temp = Temp::new("dot-store");
    let repo = temp.path.join("container-compose");
    fs::create_dir_all(&repo).expect("repo");
    let store = repo.join(".skill-store");
    let skill = write_skill(&temp.path, "container-compose", "one");
    publish::publish(
        &skill,
        &store,
        &Options {
            init: true,
            ..Default::default()
        },
    )
    .expect("publish");

    let source =
        source::parse(&format!("path:{}#container-compose@v1", repo.display())).expect("source");
    let opened = Store::open(&source, true).expect("store");
    assert_eq!(opened.root, store);

    let out = temp.path.join("export");
    export::export(&opened, "container-compose", Some("v1"), &out).expect("export");
    assert!(out.join("skill/SKILL.md").is_file());
}

#[test]
fn a_name_that_does_not_match_its_directory_is_refused() {
    let temp = Temp::new("name-mismatch");
    let dir = temp.path.join("wrong-name");
    fs::create_dir_all(&dir).expect("dir");
    fs::write(
        dir.join("SKILL.md"),
        "---\nname: right-name\ndescription: x\n---\n\nbody\n",
    )
    .expect("SKILL.md");
    let error = publish::publish(
        &dir,
        &temp.path.join("store"),
        &Options {
            init: true,
            ..Default::default()
        },
    )
    .expect_err("mismatch");
    assert!(error.contains("must match the directory name"), "{error}");
}

#[test]
fn a_git_source_is_fetched_and_can_be_pinned() {
    let temp = Temp::new("git");
    let repo = temp.path.join("repo");
    fs::create_dir_all(&repo).expect("repo");
    let store = repo.join(".skill-store");
    let skill = write_skill(&temp.path, "widget-screenshot", "one");
    publish::publish(
        &skill,
        &store,
        &Options {
            init: true,
            ..Default::default()
        },
    )
    .expect("publish");

    git(&repo, &["init", "--quiet"]);
    git(&repo, &["add", "."]);
    git(
        &repo,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            // A fixture commit must not inherit the machine's signing configuration: an agent
            // shell has no pinentry, so a globally enabled `commit.gpgsign` fails the fetch test
            // for a reason that has nothing to do with the code under test.
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "one",
        ],
    );
    let first = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();

    let cache = temp.path.join("cache");
    std::env::set_var("SKILL_STORE_CACHE", &cache);
    let url = format!("git+file://{}", repo.display());
    let source = source::parse(&format!("{url}?rev={first}#widget-screenshot@v1")).expect("source");
    let opened = Store::open(&source, false).expect("fetch");
    assert_eq!(opened.rev.as_deref(), Some(first.as_str()));
    assert!(opened.root.ends_with(".skill-store"));
    assert!(opened.root.starts_with(&cache));
    let out = temp.path.join("export");
    export::export(&opened, "widget-screenshot", Some("v1"), &out).expect("export");
    assert!(fs::read_to_string(out.join("skill/SKILL.md"))
        .expect("body")
        .contains("one"));

    // The default `.skill-store/` resolution works from the repo root, too.
    let source = source::parse(&format!("{url}#widget-screenshot@v1")).expect("source");
    let opened = Store::open(&source, true).expect("cached");
    assert!(opened.root.ends_with(".skill-store"));
    assert!(opened.root.starts_with(&cache));
}

#[test]
fn a_plain_directory_exports_as_a_working_copy() {
    let temp = Temp::new("working");
    let skill = write_skill(&temp.path, "widget-screenshot", "one");
    let out = temp.path.join("export");
    let source = format!("path:{}", skill.display());

    let report = export::export_working(&source, &skill, Some("widget-screenshot"), &out)
        .expect("working export");
    assert_eq!(report.version, "working");
    assert_eq!(report.files, 2);
    assert!(report.tree_digest.starts_with("sha256:"));

    let conf = fsx::read_flat(&out.join("export.conf")).expect("export.conf");
    assert_eq!(conf.get("version").map(String::as_str), Some("working"));
    assert_eq!(
        conf.get("source").map(String::as_str),
        Some(source.as_str())
    );
    assert!(
        !conf.contains_key("inventory_digest"),
        "a working copy has no inventory to digest"
    );
    assert_eq!(
        fs::read_to_string(out.join("skill/SKILL.md")).expect("exported"),
        fs::read_to_string(skill.join("SKILL.md")).expect("source")
    );
    assert_eq!(
        fs::read_to_string(out.join("skill/references/REFERENCE.md")).expect("reference"),
        "reference\n"
    );
}

#[test]
fn a_working_copy_names_its_selector_or_is_refused() {
    let temp = Temp::new("working-selector");
    let skill = write_skill(&temp.path, "widget-screenshot", "one");
    let source = format!("path:{}", skill.display());

    let error = export::export_working(
        &source,
        &skill,
        Some("inspect-code"),
        &temp.path.join("export"),
    )
    .expect_err("selector mismatch");
    assert!(error.contains("does not match the selector"), "{error}");
}

#[test]
fn a_working_copy_digest_follows_the_edit() {
    let temp = Temp::new("working-digest");
    let skill = write_skill(&temp.path, "widget-screenshot", "one");
    let first = export::export_working(
        &format!("path:{}", skill.display()),
        &skill,
        None,
        &temp.path.join("first"),
    )
    .expect("first");

    fs::write(skill.join("references/REFERENCE.md"), "edited\n").expect("edit");
    let second = export::export_working(
        &format!("path:{}", skill.display()),
        &skill,
        None,
        &temp.path.join("second"),
    )
    .expect("second");

    assert_ne!(
        first.tree_digest, second.tree_digest,
        "the digest has to track the working tree, or a consumer cannot see drift"
    );
}

#[test]
fn status_sees_drift_in_both_directions() {
    let temp = Temp::new("status");
    let store = temp.path.join("store");
    let worktree = temp.path.join("skills");
    fs::create_dir_all(&worktree).expect("worktree");
    fs::write(worktree.join("README.md"), "not a skill\n").expect("readme");
    let skill = write_skill(&worktree, "widget-screenshot", "one");

    publish::publish(
        &skill,
        &store,
        &Options {
            init: true,
            ..Default::default()
        },
    )
    .expect("publish");

    let readings = status::compare(&store, &worktree).expect("status");
    assert_eq!(readings.len(), 1, "a top-level file is not a skill");
    assert_eq!(readings[0].state, status::State::Current);

    // An edit, an addition and a deletion that were never published.
    fs::write(
        skill.join("SKILL.md"),
        "---\nname: widget-screenshot\ndescription: A test skill.\n---\n\ntwo\n",
    )
    .expect("edit");
    fs::write(skill.join("references/NEW.md"), "new\n").expect("add");
    fs::remove_file(skill.join("references/REFERENCE.md")).expect("remove");

    let readings = status::compare(&store, &worktree).expect("status");
    match &readings[0].state {
        status::State::Ahead(difference) => {
            assert_eq!(difference.changed, 1);
            assert_eq!(difference.added, 1);
            assert_eq!(difference.removed, 1);
        }
        other => panic!("expected ahead, got {other:?}"),
    }

    // A directory nobody can consume.
    let unsynced = worktree.join("fleet-ops");
    fs::create_dir_all(&unsynced).expect("unsynced");
    fs::write(
        unsynced.join("SKILL.md"),
        "---\nname: fleet-ops\ndescription: x\n---\n\nbody\n",
    )
    .expect("skill");
    let readings = status::compare(&store, &worktree).expect("status");
    assert!(readings
        .iter()
        .any(|reading| reading.name == "fleet-ops" && reading.state == status::State::Missing));

    // Publishing clears it, so the check is actionable rather than terminal.
    publish::publish(&skill, &store, &Options::default()).expect("publish v2");
    let readings = status::compare(&store, &worktree).expect("status");
    assert!(readings
        .iter()
        .any(|reading| reading.name == "widget-screenshot"
            && reading.state == status::State::Current));

    // And an object nobody can edit.
    fs::remove_dir_all(&skill).expect("remove directory");
    let readings = status::compare(&store, &worktree).expect("status");
    assert!(readings
        .iter()
        .any(|reading| reading.name == "widget-screenshot"
            && reading.state == status::State::StoreOnly));
}

#[test]
fn publish_changed_publishes_only_what_moved_and_is_idempotent() {
    let temp = Temp::new("publish-changed");
    let store = temp.path.join("store");
    let worktree = temp.path.join("skills");
    fs::create_dir_all(&worktree).expect("worktree");
    fs::write(worktree.join("README.md"), "not a skill\n").expect("readme");
    let changed_skill = write_skill(&worktree, "widget-screenshot", "one");
    let fresh_skill = write_skill(&worktree, "inspect-code", "body");

    publish::publish(
        &changed_skill,
        &store,
        &Options {
            init: true,
            ..Default::default()
        },
    )
    .expect("v1");

    fs::write(
        changed_skill.join("SKILL.md"),
        "---\nname: widget-screenshot\ndescription: A test skill for widget-screenshot.\n---\n\ntwo\n",
    )
    .expect("edit");
    let changed = publish::publish_changed(&store, &worktree, &Options::default()).expect("loop");
    let published: Vec<(&str, &str)> = changed
        .published
        .iter()
        .map(|report| (report.id.as_str(), report.version.as_str()))
        .collect();
    assert_eq!(
        published,
        vec![
            ("urn:skill:inspect-code", "v1"),
            ("urn:skill:widget-screenshot", "v2"),
        ],
        "a directory with no object is published, and the edited one gets one new version"
    );
    assert!(changed.blocked.is_empty());

    let readings = status::compare(&store, &worktree).expect("status");
    assert!(
        readings
            .iter()
            .all(|reading| reading.state == status::State::Current),
        "{readings:?}"
    );

    let again = publish::publish_changed(&store, &worktree, &Options::default()).expect("loop");
    assert!(
        again.published.is_empty() && again.blocked.is_empty(),
        "running it with nothing to do publishes nothing"
    );
    assert!(fresh_skill.is_dir());
}

#[test]
fn publish_changed_reports_an_object_nobody_can_edit() {
    let temp = Temp::new("publish-changed-blocked");
    let store = temp.path.join("store");
    let worktree = temp.path.join("skills");
    fs::create_dir_all(&worktree).expect("worktree");
    let skill = write_skill(&worktree, "widget-screenshot", "one");
    publish::publish(
        &skill,
        &store,
        &Options {
            init: true,
            ..Default::default()
        },
    )
    .expect("v1");
    fs::remove_dir_all(&skill).expect("remove directory");

    let changed = publish::publish_changed(&store, &worktree, &Options::default()).expect("loop");
    assert!(changed.published.is_empty());
    assert_eq!(changed.blocked, vec!["widget-screenshot".to_string()]);
}

#[test]
fn publish_changed_refuses_an_explicit_id() {
    let temp = Temp::new("publish-changed-id");
    let store = temp.path.join("store");
    let worktree = temp.path.join("skills");
    fs::create_dir_all(&worktree).expect("worktree");

    let error = publish::publish_changed(
        &store,
        &worktree,
        &Options {
            id: Some("urn:skill:other"),
            init: true,
            ..Default::default()
        },
    )
    .expect_err("one id cannot name several skills");
    assert!(error.contains("--id"), "{error}");
    assert!(
        !store.join("0=ocfl_1.1").exists(),
        "a refused call must not leave a store behind"
    );
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-C", dir.to_str().expect("utf-8")])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}
