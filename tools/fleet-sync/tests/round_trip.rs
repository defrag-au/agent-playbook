//! A payload built from a source checkout, applied to a satellite, and the refusals that keep
//! it from touching a writer or a dirty tree.
//!
//! Every repository here is a real git repository in a temporary directory. No network, no SSH.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use at_core::contract::Exit;
use at_reach::fleet::Fleet;
use fleet_sync::bundle;
use fleet_sync::target::{self, Clean};

struct Fixture {
    root: PathBuf,
    primary: PathBuf,
    air: PathBuf,
    fleet: Fleet,
    payload: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Fixture {
        let root = temp_root(label);
        let primary = root.join("primary");
        let air = root.join("air");
        let fleet_root = root.join("fleet");
        let payload = root.join("payload");

        let source = primary.join("org/repo");
        fs::create_dir_all(&source).expect("source repo");
        git(&source, &["init", "-q", "-b", "main"]);
        fs::write(source.join(".gitignore"), "target/\n.direnv/\n.env\n").expect("gitignore");
        fs::write(source.join("README.md"), "first\n").expect("README");
        commit(&source, "first");

        fs::create_dir_all(air.join("org")).expect("air root");
        fs::create_dir_all(fleet_root.join("machines")).expect("machines");
        fs::write(
            fleet_root.join("machines/air.md"),
            profile("air", "none", &air),
        )
        .expect("air profile");
        fs::write(
            fleet_root.join("machines/admin.md"),
            profile("admin", "nix-config", &air),
        )
        .expect("admin profile");

        let fleet = Fleet::at(&fleet_root, "--fleet-root").expect("fleet");

        Fixture {
            root,
            primary,
            air,
            fleet,
            payload,
        }
    }

    fn source(&self) -> PathBuf {
        self.primary.join("org/repo")
    }

    fn target(&self) -> PathBuf {
        self.air.join("org/repo")
    }

    fn bundle(&self) -> PathBuf {
        bundle::run("air", &self.payload, Some(&self.primary), None, &self.fleet).expect("bundle");
        self.payload.clone()
    }

    fn apply(&self, payload: &Path, clean: Clean) -> at_core::contract::Outcome {
        target::apply(
            "air",
            payload,
            Some(clean),
            false,
            false,
            false,
            &self.fleet,
        )
        .expect("apply")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn temp_root(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("fleet-sync-{label}-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&root).expect("temp root");
    root
}

fn profile(id: &str, writes: &str, repo_root: &Path) -> String {
    format!(
        "id: {id}\n\
         role: client\n\
         os: darwin\n\
         arch: aarch64\n\
         ssh: local\n\
         sudo: none\n\
         repo_root: {}\n\
         repos: org/repo\n\
         rebuild: true\n\
         writes: {writes}\n\
         sync: bundle\n\
         sync_clean: artifacts\n\
         notes: test fixture\n",
        repo_root.display()
    )
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn commit(dir: &Path, message: &str) {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", message]);
}

fn head(dir: &Path) -> String {
    git(dir, &["rev-parse", "HEAD"])
}

#[test]
fn a_fresh_satellite_is_cloned_and_matches_the_commit() {
    let fixture = Fixture::new("clone");
    let payload = fixture.bundle();

    let outcome = fixture.apply(&payload, Clean::Artifacts);
    assert_eq!(outcome.exit, Exit::Results);

    let target = fixture.target();
    assert!(target.is_dir());
    assert_eq!(head(&target), head(&fixture.source()));
    assert_eq!(
        git(&target, &["status", "--porcelain", "--untracked-files=no"]),
        ""
    );
}

#[test]
fn an_update_removes_artifacts_and_keeps_env() {
    let fixture = Fixture::new("update");
    let payload = fixture.bundle();
    fixture.apply(&payload, Clean::Artifacts);

    let target = fixture.target();
    fs::create_dir_all(target.join("target")).expect("artifact dir");
    fs::write(target.join("target/artifact.bin"), "built").expect("artifact");
    fs::write(target.join(".env"), "local secret").expect("env");
    fs::write(target.join("notes.txt"), "untracked").expect("notes");

    let source = fixture.source();
    fs::write(source.join("second.txt"), "second\n").expect("second");
    commit(&source, "second");
    let second = head(&source);

    let payload = fixture.bundle();
    let outcome = fixture.apply(&payload, Clean::Artifacts);
    assert_eq!(outcome.exit, Exit::Results);

    assert_eq!(head(&target), second);
    assert!(!target.join("target").exists());
    assert!(target.join(".env").exists());
    assert!(target.join("notes.txt").exists());
}

#[test]
fn a_reset_removes_untracked_files_but_protects_env() {
    let fixture = Fixture::new("reset");
    let payload = fixture.bundle();
    fixture.apply(&payload, Clean::Artifacts);

    let target = fixture.target();
    fs::write(target.join(".env"), "local secret").expect("env");
    fs::write(target.join("notes.txt"), "untracked").expect("notes");

    let payload = fixture.bundle();
    let outcome = fixture.apply(&payload, Clean::Reset);
    assert_eq!(outcome.exit, Exit::Results);

    assert!(!target.join("notes.txt").exists());
    assert!(target.join(".env").exists());
}

#[test]
fn a_dirty_checkout_is_blocked_until_forced() {
    let fixture = Fixture::new("dirty");
    let payload = fixture.bundle();
    fixture.apply(&payload, Clean::Artifacts);

    let target = fixture.target();
    fs::write(target.join("README.md"), "local edit\n").expect("edit");

    let payload = fixture.bundle();
    let outcome = fixture.apply(&payload, Clean::Artifacts);
    assert_eq!(outcome.exit, Exit::Refused);
    assert!(outcome.report.render().contains("refused:"));
    assert_eq!(
        fs::read_to_string(target.join("README.md")).expect("read"),
        "local edit\n"
    );

    let outcome = target::apply(
        "air",
        &payload,
        Some(Clean::Artifacts),
        true,
        false,
        false,
        &fixture.fleet,
    )
    .expect("forced apply");
    assert_eq!(outcome.exit, Exit::Results);
    assert_eq!(
        fs::read_to_string(target.join("README.md")).expect("read"),
        "first\n"
    );
}

#[test]
fn a_payload_that_cannot_fit_is_blocked() {
    let fixture = Fixture::new("space");
    let payload = fixture.bundle();

    let manifest = payload.join("manifest");
    let raw = fs::read_to_string(&manifest).expect("manifest");
    let raw = raw
        .lines()
        .map(|line| match line.strip_prefix("worktree_bytes: ") {
            Some(_) => format!("worktree_bytes: {}", u64::MAX),
            None => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&manifest, raw).expect("manifest rewrite");

    let outcome = fixture.apply(&payload, Clean::Artifacts);
    assert_eq!(outcome.exit, Exit::Refused);
    assert!(outcome.report.render().contains("needs"));
    assert!(!fixture.target().exists());
}

#[test]
fn a_writer_is_refused_without_allow_writer() {
    let fixture = Fixture::new("writer");
    let payload = fixture.bundle();

    let error = match target::apply(
        "admin",
        &payload,
        Some(Clean::Artifacts),
        false,
        false,
        false,
        &fixture.fleet,
    ) {
        Ok(_) => panic!("a writer must be refused"),
        Err(error) => error,
    };
    assert_eq!(error.exit, Exit::Refused);
    assert!(error.message.contains("source of truth"));
}

#[test]
fn a_payload_for_another_host_is_refused() {
    let fixture = Fixture::new("wrong-host");
    let payload = fixture.bundle();

    let error = match target::apply(
        "admin",
        &payload,
        Some(Clean::Artifacts),
        false,
        true,
        false,
        &fixture.fleet,
    ) {
        Ok(_) => panic!("a payload for another host must be refused"),
        Err(error) => error,
    };
    assert_eq!(error.exit, Exit::Refused);
    assert!(error.message.contains("built for `air`"));
}
