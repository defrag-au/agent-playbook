//! The output contract, asserted from outside the binary.
//!
//! The same invariants `at-peek` asserts, plus the two that only exist for a tool that spawns
//! something: that the one program it runs is git and nothing else, and that a repository cannot
//! make git run a program of *its* choosing while this tool reads it.
//!
//! Those last two are the reason this suite drives a real repository rather than a directory of
//! files: a `filter` attribute and a `diff.<driver>.textconv` are only dangerous in combination with
//! a config that names a command, and the only honest way to check that neither runs is to configure
//! one that would leave evidence.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_at-recall");

/// A repository, a place for scripts that must not run, and both cleaned up afterwards.
struct Repo {
    dir: PathBuf,
    props: PathBuf,
}

impl Repo {
    fn new(name: &str) -> Repo {
        let dir = std::env::temp_dir().join(format!("at-recall-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let props = dir.join("props");
        fs::create_dir_all(&props).expect("props directory");
        fs::create_dir_all(dir.join("repo")).expect("repository directory");
        let repo = Repo {
            dir: dir.join("repo"),
            props,
        };
        repo.git(&["init", "-q", "-b", "main", "."]);
        repo
    }

    fn path(&self) -> &Path {
        &self.dir
    }

    fn write(&self, rel: &str, contents: &str) -> PathBuf {
        let path = self.dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture parent");
        }
        fs::write(&path, contents).expect("write fixture");
        path
    }

    /// A script that records that it ran, and an absolute path to it — for a config value that
    /// would run it if the tool failed to neutralise the thing that names it.
    fn script(&self, name: &str) -> String {
        let marker = self.props.join(format!("{name}-ran"));
        let path = self.props.join(format!("{name}.sh"));
        fs::write(
            &path,
            format!("#!/bin/sh\ntouch '{}'\ncat\n", marker.display()),
        )
        .expect("write script");
        let mut permissions = fs::metadata(&path).expect("script metadata").permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
        fs::set_permissions(&path, permissions).expect("script permissions");
        path.to_string_lossy().into_owned()
    }

    fn ran(&self, name: &str) -> bool {
        self.props.join(format!("{name}-ran")).exists()
    }

    /// Clear the evidence a fixture's own git commands leave behind. `git add` runs a clean filter
    /// — that is what a clean filter is for — so a test that starts from a marker left by the setup
    /// proves nothing about the tool.
    fn forget(&self, name: &str) {
        let _ = fs::remove_file(self.props.join(format!("{name}-ran")));
    }

    /// A git command against this repository, in an environment this test controls: no global or
    /// system config, a fixed identity and a fixed date, so a fixture is the same on every machine
    /// and the output can be asserted exactly.
    fn git_output(&self, args: &[&str]) -> Output {
        let mut command = Command::new("git");
        command.arg("-C").arg(&self.dir).args(args);
        neutral(&mut command);
        command.output().expect("run git")
    }

    fn git(&self, args: &[&str]) -> String {
        let output = self.git_output(args);
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn configure(&self, key: &str, value: &str) {
        self.git(&["config", key, value]);
    }

    fn commit(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
        let _ = fs::remove_dir_all(&self.props);
    }
}

/// The identity and the config sources, pinned. A test that inherited the machine's git config
/// would pass or fail depending on how the machine is set up — including, on a machine with
/// `commit.gpgsign`, by hanging on a passphrase prompt.
fn neutral(command: &mut Command) {
    command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_NAME", "fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .env("GIT_AUTHOR_DATE", "2001-02-03T04:05:06Z")
        .env("GIT_COMMITTER_DATE", "2001-02-03T04:05:06Z");
}

fn at_recall(root: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(BIN);
    command.args(args).arg("--root").arg(root);
    neutral(&mut command);
    command.output().expect("run at-recall")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn code(out: &Output) -> i32 {
    out.status.code().expect("an exit code")
}

fn content_lines(text: &str) -> usize {
    text.lines().filter(|line| !line.starts_with('#')).count()
}

/// The exits an answer printed, as `(command, why)`.
fn exits(out: &Output) -> Vec<(String, String)> {
    stdout(out)
        .lines()
        .filter_map(|line| line.strip_prefix("# next: "))
        .map(|line| match line.split_once(" · ") {
            Some((command, why)) => (command.to_string(), why.to_string()),
            None => (line.to_string(), String::new()),
        })
        .collect()
}

/// Run one of the tool's own exits exactly as it was printed, through a shell — because an exit is
/// a command line, quoting included, and the property under test is that the printed text runs.
fn run_printed(command: &str) -> Output {
    let directory = Path::new(BIN).parent().expect("binary directory");
    let path = format!(
        "{}:{}",
        directory.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut shell = Command::new("sh");
    shell.arg("-c").arg(command).env("PATH", path);
    neutral(&mut shell);
    shell.output().expect("run an exit")
}

fn snapshot(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).expect("read_dir") {
            let entry = entry.expect("entry");
            let path = entry.path();
            let meta = entry.metadata().expect("metadata");
            let rel = path
                .strip_prefix(root)
                .expect("relative")
                .display()
                .to_string();
            if meta.is_dir() {
                out.push(format!("dir  {rel}"));
                walk(root, &path, out);
            } else {
                let modified = meta.modified().map(at_core::contract::iso8601_utc);
                out.push(format!("file {rel} {} {modified:?}", meta.len()));
            }
        }
    }

    let mut entries = Vec::new();
    walk(dir, dir, &mut entries);
    entries.sort();
    entries
}

/// A repository with one commit, a tracked file and a modification to it.
fn dirty(name: &str) -> Repo {
    let repo = Repo::new(name);
    repo.write("src/a.rs", "one\ntwo\nthree\n");
    repo.write("src/b.rs", "alpha\nbeta\n");
    repo.commit("first");
    repo.write("src/a.rs", "one\nTWO\nthree\nfour\n");
    repo
}

#[test]
fn state_names_the_branch_the_head_and_the_changed_paths() {
    let repo = dirty("state-basic");
    let out = at_recall(repo.path(), &["state"]);

    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.starts_with("# at-recall state · repo · main · "),
        "{text}"
    );
    // The row, without the padding: a label column that changes width is not a failure.
    assert!(text.contains("2001-02-03  fixture  first"), "{text}");
    assert!(text.contains("\nhead "), "{text}");
    assert!(text.contains("upstream   none"), "{text}");
    assert!(text.contains("pending    none"), "{text}");
    assert!(text.contains(" M  src/a.rs"), "{text}");
    // The count is a bound, and a bound is no longer the last line: the exits follow it.
    assert!(text.contains("# 1 path\n"), "{text}");
}

#[test]
fn state_lists_an_untracked_file_and_says_a_directory_is_collapsed() {
    let repo = dirty("state-untracked");
    repo.write("notes.md", "scratch\n");
    repo.write("build/one.o", "x\n");
    repo.write("build/two.o", "y\n");

    let text = stdout(&at_recall(repo.path(), &["state"]));
    assert!(text.contains("??  notes.md"), "{text}");
    assert!(text.contains("??  build/"), "{text}");
    assert!(!text.contains("build/one.o"), "{text}");
    assert!(
        text.contains("# caveat: 1 untracked directory shown collapsed"),
        "{text}"
    );
}

#[test]
fn state_reports_an_operation_in_progress() {
    let repo = dirty("state-merge");
    let head = repo.git(&["rev-parse", "HEAD"]).trim().to_string();
    fs::write(repo.path().join(".git/MERGE_HEAD"), format!("{head}\n")).expect("merge head");

    let text = stdout(&at_recall(repo.path(), &["state"]));
    assert!(text.contains("pending    merge"), "{text}");

    fs::remove_file(repo.path().join(".git/MERGE_HEAD")).expect("remove merge head");
    let text = stdout(&at_recall(repo.path(), &["state"]));
    assert!(text.contains("pending    none"), "{text}");
}

#[test]
fn a_clean_tree_still_answers_with_the_branch() {
    let repo = Repo::new("state-clean");
    repo.write("src/a.rs", "one\n");
    repo.commit("first");

    let out = at_recall(repo.path(), &["state"]);
    assert_eq!(code(&out), 0, "the branch and HEAD are the answer here");
    let text = stdout(&out);
    assert!(text.contains("2001-02-03  fixture  first"), "{text}");
    assert!(text.ends_with("# 0 paths\n"), "{text}");
}

#[test]
fn state_answers_for_the_worktree_when_given_a_subdirectory() {
    let repo = dirty("state-subdirectory");
    let nested = repo.path().join("src");

    let text = stdout(&at_recall(&nested, &["state"]));
    assert!(
        text.contains("· repo ·"),
        "the header names the worktree: {text}"
    );
    assert!(
        text.contains(" M  src/a.rs"),
        "paths stay top-relative: {text}"
    );
}

#[test]
fn diff_defaults_to_the_working_tree_against_head() {
    let repo = dirty("diff-basic");
    let out = at_recall(repo.path(), &["diff"]);

    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.starts_with("# at-recall diff · repo · working tree against HEAD\n"),
        "{text}"
    );
    assert!(text.contains("+2 -1  src/a.rs"), "{text}");
    assert!(text.contains("# 1 file, +2 -1\n"), "{text}");
}

#[test]
fn diff_takes_several_paths_in_one_invocation() {
    let repo = dirty("diff-many-paths");
    repo.write("src/b.rs", "alpha\nBETA\n");

    let text = stdout(&at_recall(repo.path(), &["diff", "src/a.rs", "src/b.rs"]));
    assert!(text.contains("+2 -1  src/a.rs"), "{text}");
    assert!(text.contains("+1 -1  src/b.rs"), "{text}");
    assert!(text.contains("# 2 files, +3 -2\n"), "{text}");

    let one = stdout(&at_recall(repo.path(), &["diff", "src/b.rs"]));
    assert!(!one.contains("src/a.rs"), "{one}");
}

#[test]
fn diff_takes_a_revision_before_the_paths() {
    let repo = Repo::new("diff-rev");
    repo.write("src/a.rs", "one\n");
    repo.commit("first");
    repo.write("src/a.rs", "one\ntwo\n");
    repo.commit("second");
    repo.write("src/a.rs", "one\ntwo\nthree\n");

    let text = stdout(&at_recall(repo.path(), &["diff", "HEAD~1", "src/a.rs"]));
    assert!(text.contains("· working tree against HEAD~1"), "{text}");
    assert!(text.contains("+2 -0  src/a.rs"), "{text}");

    let range = stdout(&at_recall(
        repo.path(),
        &["diff", "HEAD~1..HEAD", "src/a.rs"],
    ));
    assert!(range.contains("· HEAD~1..HEAD"), "{range}");
    assert!(range.contains("+1 -0  src/a.rs"), "{range}");
}

#[test]
fn diff_reports_a_rename_with_both_of_its_names() {
    let repo = Repo::new("diff-rename");
    repo.write("src/old.rs", "one\ntwo\n");
    repo.commit("first");
    repo.git(&["mv", "src/old.rs", "src/new.rs"]);

    let text = stdout(&at_recall(repo.path(), &["diff"]));
    assert!(text.contains("src/{old.rs => new.rs}"), "{text}");
}

#[test]
fn diff_says_an_untracked_path_is_untracked_rather_than_showing_nothing() {
    let repo = dirty("diff-untracked");
    repo.write("notes.md", "scratch\n");

    let out = at_recall(repo.path(), &["diff", "notes.md"]);
    assert_eq!(
        code(&out),
        1,
        "nothing differs, and that is a fact not a failure"
    );
    let text = stdout(&out);
    assert!(text.contains("# no differences"), "{text}");
    assert!(text.contains("notes.md is untracked"), "{text}");
}

#[test]
fn diff_refuses_a_path_it_cannot_find() {
    let repo = dirty("diff-missing");
    let out = at_recall(repo.path(), &["diff", "src/typo.rs"]);

    assert_eq!(code(&out), 3);
    assert!(
        stderr(&out).contains("src/typo.rs is neither a revision nor a path"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn diff_refuses_a_range_that_does_not_resolve() {
    let repo = dirty("diff-bad-range");
    let out = at_recall(repo.path(), &["diff", "HEAD~9..HEAD"]);

    assert_eq!(code(&out), 3);
    assert!(
        stderr(&out).contains("is not a revision range"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_reflog_reference_is_refused_by_name() {
    // The reflog is a history of where the branch has *been*, which is not what this tool reads.
    // Making it inexpressible is the point; saying so is what makes it usable.
    let repo = dirty("diff-reflog");
    let out = at_recall(repo.path(), &["diff", "HEAD@{1}"]);

    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("reflog"), "{}", stderr(&out));
}

#[test]
fn a_revision_that_starts_with_a_dash_is_never_an_option() {
    let repo = dirty("diff-dash");
    let out = at_recall(repo.path(), &["diff", "--upload-pack=x"]);

    // Refused as an unknown flag before git ever sees it: the grammar is closed, so there is no
    // pass-through for an argument that a later git would read as an option.
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("--upload-pack"), "{}", stderr(&out));
}

#[test]
fn patch_adds_the_hunks_to_the_table() {
    let repo = dirty("diff-patch");
    let out = at_recall(repo.path(), &["diff", "--patch"]);

    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("+2 -1  src/a.rs"), "{text}");
    assert!(text.contains("diff --git a/src/a.rs b/src/a.rs"), "{text}");
    assert!(text.contains("+TWO"), "{text}");
    assert!(text.contains("+four"), "{text}");
}

#[test]
fn patch_withholds_a_secret_shaped_file_and_names_it() {
    let repo = Repo::new("diff-secret");
    repo.write("src/a.rs", "one\n");
    repo.write(".env", "TOKEN=placeholder-not-a-secret\n");
    repo.write("deploy.key", "not-a-real-key\n");
    repo.commit("first");
    repo.write("src/a.rs", "one\ntwo\n");
    repo.write(
        ".env",
        "TOKEN=placeholder-not-a-secret\nSECOND=also-placeholder\n",
    );
    repo.write("deploy.key", "not-a-real-key\nmore\n");

    let out = at_recall(repo.path(), &["diff", "--patch"]);
    let text = stdout(&out);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(
        text.contains("# 2 secret-shaped paths withheld from the hunks: .env, deploy.key"),
        "{text}"
    );
    assert!(
        !text.contains("SECOND=also-placeholder"),
        "the hunk of a secret-shaped path must not be printed: {text}"
    );
    // The table still names them: a file's name and its line counts are metadata, and a reader who
    // cannot see that a file changed cannot ask about it.
    assert!(text.contains("+1 -0  .env"), "{text}");
    // The flag that reads them is offered the way every other continuation is: as a command.
    let offered = exits(&out);
    assert!(
        offered
            .iter()
            .any(|(command, _)| command.contains("--include-secret-paths")),
        "{offered:?}"
    );

    let forced = stdout(&at_recall(
        repo.path(),
        &["diff", "--patch", "--include-secret-paths"],
    ));
    assert!(forced.contains("SECOND=also-placeholder"), "{forced}");
}

#[test]
fn every_printed_exit_is_a_command_that_runs() {
    // An exit that names a flag the parser refuses, or a path it cannot read, is worse than no exit:
    // the reader trusts it, runs it, and eats the failure. This runs each one as printed.
    let repo = Repo::new("exits-run");
    repo.write("src/a.rs", "one\n");
    repo.write("src/b.rs", "alpha\n");
    repo.write(".env", "TOKEN=placeholder-not-a-secret\n");
    repo.commit("first");
    repo.write("src/a.rs", "one\ntwo\nthree\n");
    repo.write("src/b.rs", "alpha\nBETA\n");
    repo.write(
        ".env",
        "TOKEN=placeholder-not-a-secret\nSECOND=placeholder\n",
    );
    repo.write("notes.md", "scratch\n");

    let mut checked = 0;
    for args in [
        vec!["state"],
        vec!["state", "--limit", "1"],
        vec!["diff"],
        vec!["diff", "--limit", "1"],
        vec!["diff", "--patch"],
        vec!["diff", "--patch", "--limit", "4"],
        vec!["diff", "src/a.rs", "--patch", "--limit", "2"],
    ] {
        let printed = exits(&at_recall(repo.path(), &args));
        assert!(
            !printed.is_empty(),
            "{args:?} printed no exit, so this test is not looking"
        );
        for (command, why) in printed {
            assert!(command.starts_with("at-recall "), "{command}");
            assert!(!why.is_empty(), "{command} has no reason attached");
            let ran = run_printed(&command);
            assert_eq!(
                code(&ran),
                0,
                "`{command}` asked again did not answer: {} {}",
                stdout(&ran),
                stderr(&ran)
            );
            checked += 1;
        }
    }
    assert!(checked >= 6, "only {checked} exits were exercised");
}

#[test]
fn the_exits_are_the_questions_this_answer_implies() {
    let repo = Repo::new("exits-shape");
    repo.write("src/a.rs", "one\n");
    repo.commit("first");

    // A clean tree implies nothing, and says so by staying silent.
    assert!(exits(&at_recall(repo.path(), &["state"])).is_empty());

    // Untracked is the one kind of change a diff cannot show, so state does not offer one.
    repo.write("notes.md", "scratch\n");
    assert!(!stdout(&at_recall(repo.path(), &["state"])).contains("# next:"));

    // A tracked change does.
    repo.write("src/a.rs", "one\ntwo\n");
    let state = exits(&at_recall(repo.path(), &["state"]));
    assert_eq!(state.len(), 1, "{state:?}");
    assert!(state[0].0.contains("at-recall diff"), "{state:?}");

    // A cut answer is widened, and the widen asks the same question: no `--patch` dropped, no
    // limit that would clamp.
    let cut = exits(&at_recall(repo.path(), &["state", "--limit", "1"]));
    assert!(cut[0].0.contains("--limit 2"), "{cut:?}");
    assert_eq!(cut[0].1, "all 2 paths");

    let table = exits(&at_recall(repo.path(), &["diff"]));
    assert_eq!(table.len(), 1, "{table:?}");
    assert!(table[0].0.contains("--patch"), "{table:?}");
    assert_eq!(table[0].1, "the hunks");

    let patched = exits(&at_recall(
        repo.path(),
        &["diff", "--patch", "--limit", "3"],
    ));
    assert_eq!(patched.len(), 1, "{patched:?}");
    assert!(patched[0].0.contains("--patch"), "{patched:?}");
    assert!(patched[0].0.contains("--limit"), "{patched:?}");
}

#[test]
fn an_exit_keeps_the_tree_the_caller_named() {
    // Every invocation in this suite passes `--root`, so an exit that dropped it would answer about
    // the test runner's own directory — and would do the same to anyone working in a second tree.
    let repo = dirty("exits-root");
    let printed = exits(&at_recall(repo.path(), &["diff"]));

    assert!(printed[0].0.contains("--root"), "{}", printed[0].0);
    assert_eq!(code(&run_printed(&printed[0].0)), 0);
}

#[test]
fn every_truncation_is_announced() {
    let repo = dirty("diff-limited");
    repo.write("src/b.rs", "alpha\nBETA\n");
    repo.write("src/c.rs", "alpha\nGAMMA\n");
    repo.write("src/a.rs", "one\nTWO\nthree\nfour\n");
    repo.git(&["add", "-A"]);
    repo.write("src/c.rs", "alpha\nGAMMA\nDELTA\n");

    let out = at_recall(repo.path(), &["diff", "--limit", "1"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert_eq!(content_lines(&text), 1, "{text}");
    assert!(text.contains("shown · --limit 1"), "{text}");
    assert!(text.contains("--limit 1 reached"), "{text}");

    let patched = at_recall(repo.path(), &["diff", "--patch", "--limit", "2"]);
    let text = stdout(&patched);
    assert!(content_lines(&text) <= 2, "{text}");
    assert!(text.contains("2 of "), "{text}");
}

#[test]
fn a_limit_above_the_ceiling_is_clamped_and_announced() {
    let repo = dirty("diff-clamped");
    let text = stdout(&at_recall(repo.path(), &["diff", "--limit", "99999"]));

    assert!(text.contains("# --limit 99999 clamped to 2000"), "{text}");
}

#[test]
fn a_line_wider_than_the_cap_is_cut_and_flagged() {
    let repo = Repo::new("diff-wide");
    repo.write("src/wide.rs", "short\n");
    repo.commit("first");
    repo.write("src/wide.rs", &format!("short\n{}\n", "x".repeat(900)));

    let text = stdout(&at_recall(repo.path(), &["diff", "--patch"]));
    // 900 characters plus the diff's own `+` prefix, cut at 500: the cap is on what the reader
    // receives, not on the file's line.
    assert!(text.contains("...(+401 characters)"), "{text}");
    assert!(
        text.contains("# caveat: 1 line(s) wider than 500 characters, truncated"),
        "{text}"
    );
}

#[test]
fn no_command_writes() {
    // The snapshot includes `.git`, which is the stronger claim: a read that refreshes the index
    // has written to the repository it was asked only to read. `--no-optional-locks` is what stops
    // git doing that, and this is the test that would notice its removal.
    let repo = Repo::new("writes");
    repo.write("src/a.rs", "one\n");
    repo.write(".env", "TOKEN=placeholder-not-a-secret\n");
    repo.commit("first");
    repo.write("src/a.rs", "one\ntwo\n");

    let before = snapshot(repo.path());
    for args in [
        vec!["state"],
        vec!["state", "--limit", "1"],
        vec!["diff"],
        vec!["diff", "--patch"],
        vec!["diff", "src/a.rs"],
        vec!["diff", "HEAD..HEAD"],
        vec!["diff", "--limit", "1"],
    ] {
        let _ = at_recall(repo.path(), &args);
    }
    let after = snapshot(repo.path());

    assert_eq!(
        before, after,
        "a read-only tool that writes is not read-only"
    );
}

#[test]
fn a_repository_filter_is_refused_rather_than_run() {
    // `clean` runs when git converts a working-tree file into the blob it would commit, and a
    // repository chooses it. A tool that can be allowlisted once cannot run it, and cannot diff
    // around it either — raw bytes are not the file git would show.
    let repo = Repo::new("filter");
    let script = repo.script("filter");
    repo.write("a.foo", "one\n");
    repo.commit("first");
    repo.configure("filter.hostile.clean", &script);
    repo.configure("filter.hostile.required", "false");
    fs::write(repo.path().join(".gitattributes"), "*.foo filter=hostile\n").expect("attributes");
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "attributes"]);
    repo.forget("filter");
    repo.write("a.foo", "one\ntwo\n");

    let out = at_recall(repo.path(), &["diff"]);
    assert_eq!(code(&out), 4, "{} {}", stdout(&out), stderr(&out));
    assert!(stderr(&out).contains("filter=hostile"), "{}", stderr(&out));
    assert!(
        stdout(&out).is_empty(),
        "the refusal has to come before anything is printed: {}",
        stdout(&out)
    );
    assert!(
        !repo.ran("filter"),
        "the tool ran a program the repository named"
    );

    // The worktree is what cannot be read safely. History still can.
    let history = at_recall(repo.path(), &["diff", "HEAD~1..HEAD"]);
    assert_eq!(code(&history), 0, "{}", stderr(&history));
    assert!(
        stdout(&history).contains(".gitattributes"),
        "{}",
        stdout(&history)
    );
}

#[test]
fn a_repositorys_diff_driver_and_pager_are_not_run() {
    let repo = Repo::new("driver");
    let script = repo.script("driver");
    let pager = repo.script("pager");
    repo.configure("core.pager", &pager);
    repo.configure("diff.external", &script);
    repo.configure("diff.textconv.textconv", &script);
    repo.write("changed.rs", "one\n");
    repo.commit("first");
    repo.write("changed.rs", "one\ntwo\n");
    fs::write(repo.path().join(".gitattributes"), "*.rs diff=textconv\n").expect("attributes");

    let text = stdout(&at_recall(repo.path(), &["diff", "--patch"]));
    assert!(text.contains("+two"), "{text}");
    assert!(
        !repo.ran("driver"),
        "the tool ran the repository's diff driver"
    );
    assert!(!repo.ran("pager"), "the tool ran the repository's pager");

    let state = stdout(&at_recall(repo.path(), &["state"]));
    assert!(state.contains(" M  changed.rs"), "{state}");
    assert!(
        !repo.ran("driver"),
        "the tool ran the repository's diff driver"
    );
}

#[test]
fn output_is_byte_stable() {
    // `check` in the playbook diffs rendered output, and a reader diffs two transcripts. Either way
    // nondeterminism reads as a change that did not happen.
    let repo = dirty("stable");
    repo.write("notes.md", "scratch\n");

    for args in [vec!["state"], vec!["diff"], vec!["diff", "--patch"]] {
        let first = at_recall(repo.path(), &args);
        let second = at_recall(repo.path(), &args);
        assert_eq!(
            stdout(&first),
            stdout(&second),
            "{args:?} is not byte-stable"
        );
        assert_eq!(
            code(&first),
            code(&second),
            "{args:?} changed its exit code"
        );
    }
}

#[test]
fn a_directory_that_is_not_a_worktree_is_an_environment_error() {
    let repo = Repo::new("not-a-repo");
    fs::remove_dir_all(repo.path().join(".git")).expect("remove the repository");
    let out = at_recall(repo.path(), &["state"]);

    assert_eq!(code(&out), 3);
    assert!(
        stderr(&out).contains("is not a git worktree"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn state_takes_no_path_and_says_so() {
    let repo = dirty("state-path");
    let out = at_recall(repo.path(), &["state", "src/a.rs"]);

    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("takes no path"), "{}", stderr(&out));
}

#[test]
fn unknown_verbs_and_flags_say_what_is_available() {
    let repo = dirty("unknown");
    let verb = at_recall(repo.path(), &["log"]);
    assert_eq!(code(&verb), 2);
    assert!(stderr(&verb).contains("unknown verb"), "{}", stderr(&verb));

    let flag = at_recall(repo.path(), &["diff", "--nope"]);
    assert_eq!(code(&flag), 2);
    assert!(stderr(&flag).contains("unknown flag"), "{}", stderr(&flag));
}

#[test]
fn help_lists_every_verb_in_the_catalogue() {
    let repo = dirty("help");
    let text = stdout(&at_recall(repo.path(), &["--help"]));

    for verb in at_recall::catalogue::VERBS {
        assert!(text.contains(verb.name), "{} is not in the help", verb.name);
        assert!(text.contains(verb.usage), "{} has no usage line", verb.name);
    }
    assert!(text.contains("Exit codes"), "{text}");
}

#[test]
fn only_the_catalogue_can_reach_git() {
    // The guarantee is structural: `Noun` is the closed set of git words, and every invocation
    // spells one of them. A string built anywhere else would be a command this tool could run that
    // no test looked at.
    let source = source_of("git.rs");
    assert!(
        source.contains("Command::new(GIT)"),
        "the one process must be spawned through the constant"
    );
    assert!(
        source.contains("noun.as_str()"),
        "the git word must come from the enum"
    );
}

#[test]
fn no_mutating_noun_in_the_enum() {
    use at_recall::git::{Noun, MUTATING};

    for noun in Noun::all() {
        assert!(
            !MUTATING.contains(&noun.as_str()),
            "`{}` is a mutating git command and must not be in the enum",
            noun.as_str()
        );
    }
    // A list that has decayed into an empty array would pass the loop above.
    for word in ["commit", "push", "reset", "checkout", "reflog"] {
        assert!(MUTATING.contains(&word), "`{word}` left the list");
    }
}

#[test]
fn no_write_api_in_the_source() {
    // `git.rs` is the one exception and it is a narrow one: the single `Command::new(GIT)`, whose
    // program is a constant. Everything else in the crate must have no write path at all.
    const FORBIDDEN: &[&str] = &[
        "fs::write",
        "File::create",
        "OpenOptions",
        "create_dir",
        "remove_file",
        "remove_dir",
        "fs::rename",
        "fs::copy",
        "process::Command",
        "Command::new",
        "std::net",
        "TcpStream",
        "UdpSocket",
        "std::env::var",
    ];

    let mut checked = 0;
    for file in source_files() {
        let text = fs::read_to_string(&file).expect("read source");
        let name = file
            .file_name()
            .expect("a name")
            .to_string_lossy()
            .into_owned();
        for (index, line) in text.lines().enumerate() {
            // `git.rs` is the one exception, and it is narrow: the single `Command::new(GIT)`,
            // whose program is a constant rather than anything assembled from input.
            let spawn = ["process::Command", "Command::new"]
                .iter()
                .any(|token| line.contains(token));
            for token in FORBIDDEN {
                if name == "git.rs"
                    && spawn
                    && (*token == "process::Command" || *token == "Command::new")
                {
                    continue;
                }
                assert!(
                    !line.contains(token),
                    "{name}:{} contains `{token}` · the toolkit spawns one process and writes nothing",
                    index + 1
                );
            }
        }
        checked += 1;
    }
    assert!(
        checked >= 5,
        "the scan found only {checked} source files; it is not looking"
    );
}

fn source_files() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("read source directory") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                out.push(path);
            }
        }
    }

    let mut files = Vec::new();
    walk(&source_dir(), &mut files);
    files.sort();
    files
}

fn source_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn source_of(name: &str) -> String {
    fs::read_to_string(source_dir().join(name)).expect("read source")
}
