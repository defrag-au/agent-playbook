//! End-to-end tests over a real store: publish, export, verify, version, and fetch.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use skill_store::export;
use skill_store::fsx;
use skill_store::publish::{self, Options};
use skill_store::source;
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
