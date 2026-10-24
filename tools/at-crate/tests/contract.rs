//! The output contract for `at-crate`, asserted from outside the binary.
//!
//! The fixture is a cargo home and a project, both built from scratch: registry copies of a
//! crate, a `Cargo.lock` that names one of them, and build-script output under `target`. Every
//! test gets its own directory, so nothing is shared and nothing depends on the order tests run
//! in.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_at-crate");

const LOCK: &str = r#"version = 3

[[package]]
name = "headless_chrome"
version = "1.0.22"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "abc"

[[package]]
name = "project"
version = "0.1.0"
"#;

struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        let dir = std::env::temp_dir().join(format!("at-crate-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("fixture directory");
        let fixture = Fixture { dir };
        fixture.write("project/Cargo.toml", "[package]\nname = \"project\"\n");
        fixture.write("project/Cargo.lock", LOCK);

        let registry = "cargo-home/registry/src/index.crates.io-test";
        let crate_root = format!("{registry}/headless_chrome-1.0.22");
        fixture.write(
            &format!("{crate_root}/Cargo.toml"),
            "[package]\nname = \"headless_chrome\"\nversion = \"1.0.22\"\n",
        );
        fixture.write(
            &format!("{crate_root}/src/lib.rs"),
            "pub fn alpha() {}\npub fn beta() {}\n",
        );
        fixture.write(
            &format!("{crate_root}/src/browser/mod.rs"),
            "pub fn gamma() {}\n",
        );
        fixture.write(&format!("{crate_root}/examples/demo.rs"), "fn main() {}\n");
        fixture.write(&format!("{crate_root}/json/protocol.json"), "{}\n");
        fixture.write(
            &format!("{registry}/headless_chrome-0.9.0/Cargo.toml"),
            "[package]\nname = \"headless_chrome\"\nversion = \"0.9.0\"\n",
        );
        fixture.write(
            &format!("{registry}/only_installed-2.0.0/Cargo.toml"),
            "[package]\nname = \"only_installed\"\nversion = \"2.0.0\"\n",
        );
        fixture.write(
            &format!("{registry}/only_installed-2.0.0/src/lib.rs"),
            "pub fn only() {}\n",
        );

        fixture.write(
            "project/target/debug/build/headless_chrome-0123456789abcdef/out/protocol.rs",
            "pub struct Response {\n}\n",
        );
        fixture
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

    fn cargo_home(&self) -> PathBuf {
        self.dir.join("cargo-home")
    }

    fn project(&self) -> PathBuf {
        self.dir.join("project")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// Run a verb with the context flags that verb accepts. `installed` has no `--project`; `out` has
/// no `--cargo-home`; the others take both.
fn at_crate(fixture: &Fixture, verb: &str, args: &[&str]) -> Output {
    let mut command = Command::new(BIN);
    command.arg(verb).args(args);
    if matches!(verb, "where" | "installed" | "files" | "slice" | "search") {
        command.arg("--cargo-home").arg(fixture.cargo_home());
    }
    if matches!(verb, "where" | "files" | "slice" | "search" | "out") {
        command.arg("--project").arg(fixture.project());
    }
    command
        .env_remove("CARGO_HOME")
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("run at-crate")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn content_lines(text: &str) -> usize {
    text.lines().filter(|line| !line.starts_with('#')).count()
}

/// The exits an answer printed, as commands.
fn printed_exits(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.strip_prefix("# next: "))
        .map(|line| match line.split_once(" · ") {
            Some((command, _)) => command.to_string(),
            None => line.to_string(),
        })
        .collect()
}

/// Run one of the tool's own exits exactly as it was printed, through a shell. The printed command
/// carries the fixture's absolute `--cargo-home` and `--project`, so it must resolve the same store.
fn run_printed(command: &str) -> Output {
    let directory = Path::new(BIN).parent().expect("binary directory");
    let path = format!(
        "{}:{}",
        directory.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new("sh")
        .arg("-c")
        .arg(command)
        .env("PATH", path)
        .env_remove("CARGO_HOME")
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("run an exit")
}

/// Every path and every file's bytes, so a write of any kind changes the snapshot.
fn snapshot(root: &Path) -> Vec<(String, bool, Vec<u8>)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, bool, Vec<u8>)>) {
        let mut entries: Vec<PathBuf> = fs::read_dir(dir)
            .expect("read fixture")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .collect();
        entries.sort();
        for path in entries {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            if path.is_dir() {
                out.push((rel, true, Vec::new()));
                walk(root, &path, out);
            } else {
                out.push((rel, false, fs::read(&path).expect("read fixture file")));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out
}

#[test]
fn where_resolves_the_lock_version_and_names_the_source() {
    let fixture = Fixture::new("where");
    let out = at_crate(&fixture, "where", &["headless_chrome"]);
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(text.contains("headless_chrome 1.0.22"), "{text}");
    assert!(
        text.contains("index.crates.io-test/headless_chrome-1.0.22"),
        "{text}"
    );
    assert!(text.contains("resolved from"), "{text}");
    assert!(text.contains("build output:"), "{text}");
    assert!(text.contains("at-crate files headless_chrome"), "{text}");
}

#[test]
fn where_falls_back_to_the_only_installed_copy_and_says_so() {
    let fixture = Fixture::new("fallback");
    let out = at_crate(&fixture, "where", &["only-installed"]);
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(text.contains("only_installed 2.0.0"), "{text}");
    assert!(text.contains("not in"), "{text}");
    assert!(text.contains("only extracted copy"), "{text}");
    assert!(
        text.contains("resolved `only-installed` to `only_installed`"),
        "{text}"
    );
}

#[test]
fn two_installed_versions_are_an_error_naming_both() {
    let fixture = Fixture::new("ambiguous");
    // Remove the lock entry so the store has to answer, and it has two answers.
    fixture.write(
        "project/Cargo.lock",
        "version = 3\n[[package]]\nname = \"project\"\nversion = \"0.1.0\"\n",
    );
    let out = at_crate(&fixture, "where", &["headless_chrome"]);
    let text = stderr(&out);

    assert_eq!(out.status.code(), Some(2), "stdout: {}", stdout(&out));
    assert!(text.contains("1.0.22"), "{text}");
    assert!(text.contains("0.9.0"), "{text}");
    assert!(text.contains("--version <ver>"), "{text}");
}

#[test]
fn an_exact_version_overrides_the_lock_without_pretending_otherwise() {
    let fixture = Fixture::new("version-flag");
    let out = at_crate(
        &fixture,
        "where",
        &["headless_chrome", "--version", "0.9.0"],
    );
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(text.contains("headless_chrome 0.9.0"), "{text}");
    assert!(text.contains("not the lock's choice"), "{text}");
}

#[test]
fn a_path_package_answers_without_inventing_a_directory() {
    let fixture = Fixture::new("path");
    let out = at_crate(&fixture, "where", &["project"]);
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(text.contains("path package"), "{text}");
    assert!(content_lines(&text) == 0, "no invented path: {text}");
}

#[test]
fn installed_lists_versions_and_says_how_many_it_showed() {
    let fixture = Fixture::new("installed");
    let out = at_crate(&fixture, "installed", &["headless_chrome", "--limit", "1"]);
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(content_lines(&text), 1, "{text}");
    assert!(text.contains("headless_chrome 1.0.22"), "{text}");
    assert!(text.contains("1 of 2 extracted copies"), "{text}");
    assert!(text.contains("--limit 1"), "{text}");
}

#[test]
fn files_is_sorted_and_states_the_depth_it_cut() {
    let fixture = Fixture::new("files");
    let out = at_crate(&fixture, "files", &["headless_chrome", "--depth", "1"]);
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(text.contains("Cargo.toml"), "{text}");
    assert!(text.contains("src/"), "{text}");
    assert!(!text.contains("src/lib.rs"), "depth 1 descends: {text}");
    assert!(text.contains("directories not descended"), "{text}");
    assert!(text.contains("--depth 2"), "{text}");
}

#[test]
fn files_skips_vcs_and_cargo_metadata() {
    let fixture = Fixture::new("files-skips");
    let root = "cargo-home/registry/src/index.crates.io-test/headless_chrome-1.0.22";
    fixture.write(&format!("{root}/.git/HEAD"), "ref: refs/heads/main\n");
    fixture.write(&format!("{root}/.cargo/config.toml"), "[net]\n");
    let out = at_crate(&fixture, "files", &["headless_chrome", "--depth", "1"]);
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(!text.contains(".git/"), "{text}");
    assert!(!text.contains(".cargo/"), "{text}");
    assert!(text.contains("skipped by rule: .cargo, .git"), "{text}");
}

#[test]
fn slice_numbers_the_lines_and_offers_the_rest() {
    let fixture = Fixture::new("slice");
    let out = at_crate(
        &fixture,
        "slice",
        &["headless_chrome/src/browser/mod.rs:1-1", "--limit", "1"],
    );
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(text.contains("1  pub fn gamma() {}"), "{text}");
    assert!(text.contains("whole file"), "{text}");
}

#[test]
fn a_limit_cut_states_the_requested_range_and_the_continuation() {
    let fixture = Fixture::new("slice-limit");
    let out = at_crate(
        &fixture,
        "slice",
        &["headless_chrome/src/lib.rs:1-2", "--limit", "1"],
    );
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(content_lines(&text), 1, "{text}");
    assert!(text.contains("1 of 2 requested lines"), "{text}");
    assert!(
        text.contains("at-crate slice headless_chrome/src/lib.rs:2-2"),
        "{text}"
    );
}

#[test]
fn a_symlink_out_of_the_crate_is_refused() {
    #[cfg(unix)]
    {
        let fixture = Fixture::new("escape");
        let crate_root = fixture
            .cargo_home()
            .join("registry/src/index.crates.io-test/headless_chrome-1.0.22");
        std::os::unix::fs::symlink(
            fixture.project().join("Cargo.toml"),
            crate_root.join("escape.rs"),
        )
        .expect("symlink");

        let out = at_crate(&fixture, "slice", &["headless_chrome/escape.rs:1-2"]);
        assert_eq!(out.status.code(), Some(4), "stderr: {}", stderr(&out));
        assert!(
            stderr(&out).contains("resolves outside"),
            "{}",
            stderr(&out)
        );
    }
}

#[test]
fn a_secret_shaped_path_is_refused_and_can_be_forced() {
    let fixture = Fixture::new("secret");
    fixture.write(
        "cargo-home/registry/src/index.crates.io-test/headless_chrome-1.0.22/.env",
        "TOKEN=placeholder\n",
    );

    let refused = at_crate(&fixture, "slice", &["headless_chrome/.env"]);
    assert_eq!(
        refused.status.code(),
        Some(4),
        "stderr: {}",
        stderr(&refused)
    );
    assert!(stderr(&refused).contains("--include-secret-paths"));

    let forced = at_crate(
        &fixture,
        "slice",
        &["headless_chrome/.env", "--include-secret-paths"],
    );
    assert_eq!(forced.status.code(), Some(0), "stderr: {}", stderr(&forced));
    assert!(stdout(&forced).contains("TOKEN=placeholder"));
}

#[test]
fn search_counts_every_match_it_considered() {
    let fixture = Fixture::new("search");
    let out = at_crate(
        &fixture,
        "search",
        &["pub fn", "headless_chrome", "--limit", "2"],
    );
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(content_lines(&text), 2, "{text}");
    assert!(text.contains("2 of 3 matches"), "{text}");
    assert!(text.contains("--limit 3"), "{text}");
    assert!(text.contains("headless_chrome/src/lib.rs:1"), "{text}");
}

#[test]
fn search_count_is_per_file_and_bounded() {
    let fixture = Fixture::new("search-count");
    let out = at_crate(
        &fixture,
        "search",
        &["pub fn", "headless_chrome", "--count", "--limit", "1"],
    );
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(content_lines(&text), 1, "{text}");
    assert!(text.contains("1 of 2 files shown"), "{text}");
    assert!(text.contains("3 matches in 2 files"), "{text}");
}

#[test]
fn search_skips_a_secret_shaped_file_and_says_so() {
    let fixture = Fixture::new("search-secret");
    fixture.write(
        "cargo-home/registry/src/index.crates.io-test/headless_chrome-1.0.22/.env",
        "TOKEN=placeholder\n",
    );
    let out = at_crate(&fixture, "search", &["TOKEN", "headless_chrome"]);
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr(&out));
    assert!(text.contains("1 secret-shaped"), "{text}");
    assert!(!text.contains("TOKEN=placeholder"), "{text}");
}

#[test]
fn out_finds_a_nested_target_build_dir_and_offers_the_generated_file() {
    let fixture = Fixture::new("out");
    fixture.write(
        "project/target/aarch64-apple-darwin/debug/build/headless_chrome-fedcba9876543210/out/generated.rs",
        "pub fn generated() {}\n",
    );
    let out = at_crate(&fixture, "out", &["headless_chrome"]);
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(
        text.contains("target/debug/build/headless_chrome-0123456789abcdef/out"),
        "{text}"
    );
    assert!(
        text.contains("aarch64-apple-darwin/debug/build/headless_chrome-fedcba9876543210/out"),
        "{text}"
    );
    assert!(text.contains("2 of 2 build directories"), "{text}");
    assert!(text.contains("--build-dir <path>"), "{text}");
    assert!(
        printed_exits(&text).is_empty(),
        "ambiguous output offers no exit: {text}"
    );
}

#[test]
fn build_output_is_read_with_the_build_flag() {
    let fixture = Fixture::new("build");
    let out = at_crate(
        &fixture,
        "slice",
        &["headless_chrome/protocol.rs:1-2", "--build"],
    );
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(text.contains("1  pub struct Response {"), "{text}");
    assert!(
        text.contains("target/debug/build/headless_chrome-0123456789abcdef/out"),
        "{text}"
    );
}

#[test]
fn two_build_directories_are_an_error_naming_the_flag_that_chooses() {
    let fixture = Fixture::new("build-ambiguous");
    fixture.write(
        "project/target/aarch64-apple-darwin/debug/build/headless_chrome-fedcba9876543210/out/protocol.rs",
        "pub fn other() {}\n",
    );
    let out = at_crate(
        &fixture,
        "slice",
        &["headless_chrome/protocol.rs:1-2", "--build"],
    );

    assert_eq!(out.status.code(), Some(2), "stdout: {}", stdout(&out));
    assert!(
        stderr(&out).contains("--build-dir <path>"),
        "{}",
        stderr(&out)
    );

    let build_dir = fixture.path().join(
        "project/target/aarch64-apple-darwin/debug/build/headless_chrome-fedcba9876543210/out",
    );
    let chosen = at_crate(
        &fixture,
        "slice",
        &[
            "headless_chrome/protocol.rs:1-1",
            "--build",
            "--build-dir",
            build_dir.to_str().expect("utf-8 path"),
        ],
    );
    assert_eq!(chosen.status.code(), Some(0), "stderr: {}", stderr(&chosen));
    assert!(
        stdout(&chosen).contains("pub fn other() {}"),
        "{}",
        stdout(&chosen)
    );
}

#[test]
fn search_build_searches_every_out_directory_and_says_how_many() {
    let fixture = Fixture::new("search-build");
    fixture.write(
        "project/target/aarch64-apple-darwin/debug/build/headless_chrome-fedcba9876543210/out/protocol.rs",
        "pub struct Response {\n}\n",
    );
    let out = at_crate(
        &fixture,
        "search",
        &["Response", "headless_chrome", "--build"],
    );
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(text.contains("2 build directories searched"), "{text}");
    assert!(text.contains("--build-dir <path> narrows to one"), "{text}");
    assert!(text.contains("protocol.rs:1"), "{text}");
}

#[test]
fn an_explicit_cargo_home_and_project_are_echoed_in_every_exit() {
    let fixture = Fixture::new("exit-context");
    let out = at_crate(&fixture, "where", &["headless_chrome"]);
    let text = stdout(&out);

    let exits = printed_exits(&text);
    assert!(!exits.is_empty(), "{text}");
    for command in exits {
        // `out` does not read a cargo home, so its exit carries only the project; every other
        // verb in this answer accepts both flags.
        if !command.starts_with("at-crate out") {
            assert!(command.contains("--cargo-home"), "{command}");
        }
        assert!(command.contains("--project"), "{command}");
        let run = run_printed(&command);
        assert!(
            run.status.success(),
            "printed exit failed: {command}\nstdout: {}\nstderr: {}",
            stdout(&run),
            stderr(&run)
        );
    }
}

#[test]
fn every_printed_exit_is_a_command_that_runs() {
    let fixture = Fixture::new("exits");
    for (verb, args) in [
        ("where", vec!["headless_chrome"]),
        ("files", vec!["headless_chrome", "--depth", "1"]),
        ("installed", vec!["headless_chrome"]),
        (
            "slice",
            vec!["headless_chrome/src/lib.rs:1-2", "--limit", "1"],
        ),
        ("search", vec!["pub fn", "headless_chrome", "--limit", "2"]),
        ("out", vec!["headless_chrome"]),
    ] {
        let out = at_crate(&fixture, verb, &args);
        for command in printed_exits(&stdout(&out)) {
            let run = run_printed(&command);
            assert!(
                run.status.success(),
                "printed exit failed for {verb}: {command}\nstdout: {}\nstderr: {}",
                stdout(&run),
                stderr(&run)
            );
        }
    }
}

#[test]
fn unknown_flag_is_a_usage_error() {
    let fixture = Fixture::new("unknown-flag");
    let out = at_crate(&fixture, "search", &["--wat"]);
    assert_eq!(out.status.code(), Some(2), "stderr: {}", stderr(&out));
    assert!(stderr(&out).contains("unknown flag `--wat`"));
}

#[test]
fn help_lists_every_verb_in_the_catalogue() {
    let fixture = Fixture::new("help");
    let out = at_crate(&fixture, "help", &[]);
    let text = stdout(&out);
    for verb in ["where", "installed", "files", "slice", "search", "out"] {
        assert!(text.contains(verb), "help omits {verb}: {text}");
    }
}

#[test]
fn no_write_api_in_the_source() {
    // The guarantee is structural: no write call, no subprocess and no socket in the source. The
    // environment reads this tool does make (`CARGO_HOME`, `CARGO_TARGET_DIR`, `HOME`) name
    // locations to read, so `std::env::var` is deliberately not in this list.
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
    ];

    let tools = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tools")
        .to_path_buf();
    let mut checked = 0;
    for source in [
        tools.join("at-crate").join("src"),
        tools.join("at-core").join("src"),
    ] {
        for file in source_files(&source) {
            let text = fs::read_to_string(&file).expect("read source");
            for (index, line) in text.lines().enumerate() {
                for token in FORBIDDEN {
                    assert!(
                        !line.contains(token),
                        "{}:{} contains `{token}` — the toolkit promises no write path and no \
                         subprocess",
                        file.display(),
                        index + 1
                    );
                }
            }
            checked += 1;
        }
    }
    assert!(checked >= 12, "the scan found only {checked} source files");
}

#[test]
fn no_command_writes() {
    let fixture = Fixture::new("writes");
    let before = snapshot(fixture.path());
    for (verb, args) in [
        ("where", vec!["headless_chrome"]),
        ("where", vec!["only-installed"]),
        ("installed", vec![]),
        ("files", vec!["headless_chrome"]),
        ("slice", vec!["headless_chrome/src/lib.rs:1-2"]),
        ("slice", vec!["headless_chrome/../escape.rs:1-2"]),
        ("search", vec!["pub fn", "headless_chrome"]),
        ("search", vec!["pub fn", "headless_chrome", "--count"]),
        ("search", vec!["pub fn", "headless_chrome", "--files-only"]),
        ("out", vec!["headless_chrome"]),
    ] {
        let _ = at_crate(&fixture, verb, &args);
    }
    let after = snapshot(fixture.path());
    assert_eq!(
        before, after,
        "a read-only tool that writes is not read-only"
    );
}

fn source_files(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let mut entries: Vec<PathBuf> = fs::read_dir(dir)
            .expect("read source")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}
