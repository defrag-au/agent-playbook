//! The output contract, asserted from outside the binary.
//!
//! These are the invariants that make `at-peek *` safe to allowlist and cheap to read, and
//! every one of them is a property of what a *caller* sees. A unit test cannot honestly check
//! "no output exceeds the cap" or "every truncation is announced", so these drive the real
//! binary and read its real stdout.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_at-peek");

/// A directory that cleans up after itself. Every test gets its own, so nothing is shared and
/// nothing depends on the order tests run in.
struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        let dir = std::env::temp_dir().join(format!("at-peek-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("fixture directory");
        Fixture { dir }
    }

    fn path(&self) -> &Path {
        &self.dir
    }

    fn write(&self, rel: &str, contents: &str) -> PathBuf {
        self.write_bytes(rel, contents.as_bytes())
    }

    fn write_bytes(&self, rel: &str, bytes: &[u8]) -> PathBuf {
        let path = self.dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture parent");
        }
        fs::write(&path, bytes).expect("write fixture");
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn at_peek(root: &Path, verb: &str, args: &[&str]) -> Output {
    let mut command = Command::new(BIN);
    command.arg(verb).args(args).arg("--root").arg(root);
    command.output().expect("run at-peek")
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

fn numbered_lines(n: usize) -> String {
    (1..=n)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn slice_prints_the_requested_lines_with_numbers() {
    let fixture = Fixture::new("slice-basic");
    fixture.write("src/one.rs", "line one\nline two\nline three\nline four\n");

    let out = at_peek(fixture.path(), "slice", &["src/one.rs:2-3"]);
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0));
    assert!(text.contains("2  line two"), "number and text: {text}");
    assert!(text.contains("3  line three"), "{text}");
    assert!(
        !text.contains("line one"),
        "lines outside the range: {text}"
    );
}

#[test]
fn a_slice_always_states_how_much_of_the_file_it_is() {
    let fixture = Fixture::new("slice-coverage");
    fixture.write("src/one.rs", "line one\nline two\nline three\nline four\n");

    let text = stdout(&at_peek(fixture.path(), "slice", &["src/one.rs:2-3"]));

    assert!(
        text.contains("2 of 4 lines"),
        "the file is bigger than the window: {text}"
    );
    assert!(text.contains("for the rest"), "and how to widen: {text}");
}

#[test]
fn no_output_exceeds_the_cap() {
    let fixture = Fixture::new("cap");
    fixture.write("big.txt", &numbered_lines(5000));

    let out = at_peek(fixture.path(), "slice", &["big.txt"]);
    let text = stdout(&out);

    assert_eq!(
        content_lines(&text),
        200,
        "the default cap is the contract: {text}"
    );
    assert!(text.contains("200 of 5000 lines"), "{text}");
}

#[test]
fn every_truncation_is_announced() {
    let fixture = Fixture::new("truncated");
    fixture.write("big.txt", &numbered_lines(5000));

    let out = at_peek(fixture.path(), "slice", &["big.txt", "--limit", "20"]);
    let text = stdout(&out);

    assert_eq!(content_lines(&text), 20);
    assert!(text.contains("--limit 20 reached"), "{text}");
    assert!(
        text.contains("next: at-peek slice big.txt:21-5000"),
        "resume point: {text}"
    );
}

#[test]
fn a_limit_above_the_ceiling_is_clamped_and_announced() {
    let fixture = Fixture::new("clamped");
    fixture.write("big.txt", &numbered_lines(10));

    let out = at_peek(fixture.path(), "slice", &["big.txt", "--limit", "99999"]);
    let text = stdout(&out);

    assert!(
        text.contains("clamped to 2000"),
        "clamping is never silent: {text}"
    );
    assert_eq!(
        content_lines(&text),
        10,
        "the file is shorter than the clamp"
    );
}

#[test]
fn the_limit_is_shared_across_targets() {
    let fixture = Fixture::new("budget");
    fixture.write("a.txt", &numbered_lines(4));
    fixture.write("b.txt", &numbered_lines(4));
    fixture.write("c.txt", &numbered_lines(4));

    let out = at_peek(
        fixture.path(),
        "slice",
        &["a.txt", "b.txt", "c.txt", "--limit", "5"],
    );
    let text = stdout(&out);

    assert_eq!(
        content_lines(&text),
        5,
        "a batch cannot print more than the cap: {text}"
    );
    assert!(
        text.contains("next: at-peek slice b.txt:2-4"),
        "a mid-target cut resumes: {text}"
    );
    assert!(
        text.contains("1 target not shown"),
        "the untouched target is named: {text}"
    );
}

#[test]
fn a_whole_file_read_says_so() {
    let fixture = Fixture::new("whole");
    fixture.write("small.txt", "one\ntwo\n");

    let text = stdout(&at_peek(fixture.path(), "slice", &["small.txt"]));

    assert!(text.contains("2 lines · whole file"), "{text}");
}

#[test]
fn a_line_wider_than_the_cap_is_truncated_and_flagged() {
    let fixture = Fixture::new("wide");
    fixture.write("wide.txt", &format!("{}\n", "x".repeat(900)));

    let out = at_peek(fixture.path(), "slice", &["wide.txt"]);
    let text = stdout(&out);

    assert!(
        text.contains("wider than 500 characters, truncated"),
        "{text}"
    );
    assert!(
        text.contains("characters)"),
        "the marker says how much was cut: {text}"
    );
}

#[test]
fn a_binary_file_is_reported_rather_than_printed() {
    let fixture = Fixture::new("binary");
    fixture.write_bytes("blob.bin", &[0x00, 0x01, 0x02, 0xff, 0x00]);

    let out = at_peek(fixture.path(), "slice", &["blob.bin"]);
    let text = stdout(&out);

    assert_eq!(
        out.status.code(),
        Some(1),
        "read succeeded, nothing to show"
    );
    assert!(text.contains("binary file"), "{text}");
}

#[test]
fn an_empty_file_is_nothing_found_rather_than_a_failure() {
    let fixture = Fixture::new("empty");
    fixture.write("empty.txt", "");

    let out = at_peek(fixture.path(), "slice", &["empty.txt"]);

    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("is empty"), "{}", stdout(&out));
}

#[test]
fn stat_reports_a_path_without_printing_its_contents() {
    let fixture = Fixture::new("stat");
    fixture.write("src/one.rs", "a\nb\nc\n");

    let out = at_peek(fixture.path(), "stat", &["src/one.rs"]);
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0));
    assert!(text.contains("3 lines"), "{text}");
    assert!(text.contains("rust"), "{text}");
    assert!(!text.contains("a\nb"), "stat is not a read: {text}");
}

#[test]
fn unknown_flag_is_a_usage_error() {
    let fixture = Fixture::new("bad-flag");
    fixture.write("one.txt", "one\n");

    let out = at_peek(fixture.path(), "slice", &["one.txt", "--nope"]);

    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("unknown flag `--nope`"),
        "{}",
        stderr(&out)
    );
    assert!(empty_or_meta(&out.stdout), "a usage error prints no report");
}

#[test]
fn a_flag_before_the_verb_says_where_flags_go() {
    let out = Command::new(BIN)
        .arg("--limit")
        .arg("5")
        .arg("slice")
        .output()
        .expect("run");

    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("flags come after the verb"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_path_outside_the_root_is_refused() {
    let fixture = Fixture::new("containment");

    let out = at_peek(fixture.path(), "slice", &[".."]);

    assert_eq!(out.status.code(), Some(4));
    assert!(
        stderr(&out).contains("outside the root"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_missing_path_inside_the_root_is_an_environment_error() {
    let fixture = Fixture::new("missing");

    let out = at_peek(fixture.path(), "slice", &["no/such/file.rs"]);

    assert_eq!(
        out.status.code(),
        Some(3),
        "not a refusal: the path is inside the root"
    );
}

#[test]
fn a_secret_shaped_path_is_refused_and_can_be_forced() {
    let fixture = Fixture::new("secrets");
    fixture.write(".env", "TOKEN=placeholder\n");
    fixture.write(".env.example", "TOKEN=your-token-here\n");

    let refused = at_peek(fixture.path(), "slice", &[".env"]);
    assert_eq!(refused.status.code(), Some(4));
    assert!(
        stderr(&refused).contains("--include-secret-paths"),
        "{}",
        stderr(&refused)
    );
    assert!(
        !stdout(&refused).contains("placeholder"),
        "a refusal prints no contents"
    );

    let forced = at_peek(fixture.path(), "slice", &[".env", "--include-secret-paths"]);
    assert_eq!(forced.status.code(), Some(0));
    assert!(stdout(&forced).contains("placeholder"));

    let template = at_peek(fixture.path(), "slice", &[".env.example"]);
    assert_eq!(
        template.status.code(),
        Some(0),
        "a committed template is not a secret"
    );
}

#[test]
fn a_symbol_target_says_it_is_not_implemented_yet() {
    let fixture = Fixture::new("symbol");
    fixture.write("one.rs", "fn main() {}\n");

    let out = at_peek(fixture.path(), "slice", &["one.rs@main"]);

    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("not implemented yet"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn zero_is_not_a_line_number() {
    let fixture = Fixture::new("zero");
    fixture.write("one.txt", "one\n");

    let out = at_peek(fixture.path(), "slice", &["one.txt:0"]);

    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("1-based"), "{}", stderr(&out));
}

#[test]
fn help_lists_every_verb_in_the_catalogue() {
    let out = Command::new(BIN).arg("help").output().expect("run");
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0));
    for verb in at_peek::catalogue::VERBS {
        assert!(
            text.contains(verb.name),
            "{} is advertised but not listed",
            verb.name
        );
    }
}

#[test]
fn no_write_api_in_the_source() {
    // The guarantee is structural, so the cheapest honest check is absence in the source: a
    // behavioural test can be passed by an unlucky path, a missing `fs::write` cannot.
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

    let tools = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tools")
        .to_path_buf();
    let mut checked = 0;
    for source in [
        tools.join("at-peek").join("src"),
        tools.join("at-describe").join("src"),
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
    assert!(
        checked >= 6,
        "the scan found only {checked} source files; it is not looking"
    );
}

#[test]
fn no_command_writes() {
    let fixture = Fixture::new("writes");
    fixture.write("src/one.rs", "fn main() {}\nfn other() {}\n");
    fixture.write(".env", "TOKEN=placeholder\n");

    let before = snapshot(fixture.path());
    for (verb, args) in [
        ("stat", vec!["src/one.rs"]),
        ("slice", vec!["src/one.rs:1-2"]),
        ("slice", vec!["src/one.rs"]),
        ("slice", vec![".."]),
        ("slice", vec![".env"]),
    ] {
        let _ = at_peek(fixture.path(), verb, &args);
    }
    let after = snapshot(fixture.path());

    assert_eq!(
        before, after,
        "a read-only tool that writes is not read-only"
    );
}

fn empty_or_meta(bytes: &[u8]) -> bool {
    String::from_utf8_lossy(bytes)
        .lines()
        .all(|line| line.is_empty() || line.starts_with('#'))
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
                let modified = meta.modified().map(at_peek::contract::iso8601_utc);
                out.push(format!("file {rel} {} {modified:?}", meta.len()));
            }
        }
    }

    let mut entries = Vec::new();
    walk(dir, dir, &mut entries);
    entries.sort();
    entries
}

fn source_files(dir: &Path) -> Vec<PathBuf> {
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
    walk(dir, &mut files);
    files.sort();
    files
}
