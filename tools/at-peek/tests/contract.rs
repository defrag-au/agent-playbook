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

/// The exits an answer printed, as `(command, why)`.
fn printed_exits(text: &str) -> Vec<(String, String)> {
    text.lines()
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
    shell.output().expect("run an exit")
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
    // The rest of the file is offered as a command rather than as prose inside the bound: the
    // reader should be able to continue without composing the range themselves.
    assert!(
        printed_exits(&text)
            .iter()
            .any(|(command, _)| command.contains("slice src/one.rs:4-4")),
        "and how to widen: {text}"
    );
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
fn every_printed_exit_is_a_command_that_runs() {
    // An exit that names a flag the parser refuses, or a target it cannot read, is worse than no
    // exit: the reader trusts it, runs it, and eats the failure.
    let fixture = Fixture::new("exits-run");
    fixture.write("a.txt", &numbered_lines(40));
    fixture.write("b.txt", &numbered_lines(40));
    fixture.write("many.txt", &numbered_lines(3000));

    let mut checked = 0;
    for (verb, args) in [
        ("stat", vec!["a.txt", "b.txt", "many.txt", "--limit", "2"]),
        ("slice", vec!["a.txt", "b.txt", "--limit", "5"]),
        ("slice", vec!["many.txt", "--limit", "20"]),
        ("slice", vec!["a.txt:1-3"]),
        ("search", vec!["line", "--limit", "2"]),
        ("search", vec!["line", "--count", "--limit", "1"]),
        ("search", vec!["line", "--max-files", "2"]),
    ] {
        let out = at_peek(fixture.path(), verb, &args);
        let text = stdout(&out);
        let exits = printed_exits(&text);
        assert!(!exits.is_empty(), "{verb} {args:?} printed no exit: {text}");
        for (command, why) in exits {
            assert!(command.starts_with("at-peek "), "{command}");
            assert!(!why.is_empty(), "{command} has no reason attached");
            let ran = run_printed(&command);
            assert_eq!(
                ran.status.code(),
                Some(0),
                "`{command}` asked again did not answer: {} {}",
                stdout(&ran),
                stderr(&ran)
            );
            checked += 1;
        }
    }
    assert!(checked >= 5, "only {checked} exits were exercised");
}

#[test]
fn stat_names_the_paths_a_limit_cut() {
    let fixture = Fixture::new("stat-cut");
    fixture.write("a.txt", "one\n");
    fixture.write("b.txt", "one\n");

    let out = at_peek(fixture.path(), "stat", &["a.txt", "b.txt", "--limit", "1"]);
    let text = stdout(&out);

    assert!(text.contains("1 of 2 paths"), "{text}");
    let exits = printed_exits(&text);
    assert_eq!(exits.len(), 1, "{exits:?}");
    assert!(exits[0].0.contains("a.txt b.txt --limit 2"), "{exits:?}");
}

#[test]
fn a_search_summary_is_the_count_without_the_listing() {
    let fixture = Fixture::new("search-summary");
    fixture.write("a.txt", "needle one\nother\nneedle two\n");
    fixture.write("b.txt", "needle three\n");

    let out = at_peek(fixture.path(), "search", &["needle", "--summary"]);
    let text = stdout(&out);

    assert_eq!(
        content_lines(&text),
        0,
        "the frame is the answer here: {text}"
    );
    assert!(text.contains("3 matches in 2 files, not shown"), "{text}");
    // The one exit is the same question in full, in the shape that was asked for.
    let exits = printed_exits(&text);
    assert_eq!(exits.len(), 1, "{exits:?}");
    assert!(exits[0].0.contains("needle --limit 3"), "{exits:?}");
    assert_eq!(exits[0].1, "all 3 matches");
}

#[test]
fn a_search_widen_keeps_the_shape_it_was_asked_in() {
    let fixture = Fixture::new("search-shape");
    fixture.write("a.txt", "needle\n");
    fixture.write("b.txt", "needle\n");

    let count = stdout(&at_peek(
        fixture.path(),
        "search",
        &["needle", "--count", "--limit", "1"],
    ));
    let exits = printed_exits(&count);
    assert!(
        exits[0].0.contains("--count") && exits[0].0.contains("--limit 2"),
        "a widen of --count is still --count: {exits:?}"
    );
    assert_eq!(
        exits[0].1, "all 2 files",
        "the rows are files here: {exits:?}"
    );
}

#[test]
fn a_capped_walk_offers_a_wider_walk_and_says_where_the_number_came_from() {
    let fixture = Fixture::new("search-capped");
    for index in 0..8 {
        fixture.write(&format!("f{index}.txt"), "needle\n");
    }

    let text = stdout(&at_peek(
        fixture.path(),
        "search",
        &["needle", "--max-files", "2", "--summary"],
    ));

    assert!(text.contains("stopped after 2 files considered"), "{text}");
    // The body the summary withheld, and the wider walk. Two exits, and the second says where its
    // number came from.
    let exits = printed_exits(&text);
    assert_eq!(exits.len(), 2, "{exits:?}");
    assert!(exits[0].0.contains("--limit 2"), "{exits:?}");
    let walk = &exits[1];
    assert!(walk.0.contains("--max-files 4"), "{exits:?}");
    assert_eq!(walk.1, "twice the walk");
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
        text.contains("next: at-peek slice b.txt:2-4 c.txt"),
        "a mid-target cut resumes, and carries the target it never reached: {text}"
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
        ("search", vec!["fn"]),
        ("search", vec!["fn", "--count"]),
        ("search", vec!["fn", "--files-only"]),
    ] {
        let _ = at_peek(fixture.path(), verb, &args);
    }
    let after = snapshot(fixture.path());

    assert_eq!(
        before, after,
        "a read-only tool that writes is not read-only"
    );
}

#[test]
fn search_finds_matches_and_states_the_total() {
    let fixture = Fixture::new("search-basic");
    fixture.write("src/one.rs", "alpha\nneedle here\nbeta\n");
    fixture.write("src/two.rs", "gamma\n");

    let out = at_peek(fixture.path(), "search", &["needle"]);
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0));
    assert!(text.contains("src/one.rs:2:needle here"), "{text}");
    assert!(text.contains("1 match in 1 file"), "{text}");
}

#[test]
fn search_walks_the_whole_root_when_given_only_a_pattern() {
    let fixture = Fixture::new("search-walk");
    fixture.write("src/a.rs", "needle\n");
    fixture.write("docs/nested/b.md", "needle\n");

    let out = at_peek(fixture.path(), "search", &["needle"]);
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0));
    assert!(text.contains("src/a.rs:1:needle"), "{text}");
    assert!(text.contains("docs/nested/b.md:1:needle"), "{text}");
    assert!(text.contains("2 matches in 2 files"), "{text}");
    assert!(
        text.contains("walked 2 files"),
        "the walk says what it walked: {text}"
    );
}

#[test]
fn count_mode_reports_per_file_totals() {
    let fixture = Fixture::new("search-count");
    fixture.write("a.txt", "needle\nneedle\n");
    fixture.write("b.txt", "needle\nneedle\nneedle\n");

    let out = at_peek(fixture.path(), "search", &["needle", "--count"]);
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0));
    assert!(text.contains("a.txt  2"), "{text}");
    assert!(text.contains("b.txt  3"), "{text}");
    assert!(text.contains("5 matches in 2 files"), "{text}");
}

#[test]
fn files_only_lists_the_paths() {
    let fixture = Fixture::new("search-files-only");
    fixture.write("a.txt", "needle\nneedle\n");
    fixture.write("b.txt", "nothing\n");

    let out = at_peek(fixture.path(), "search", &["needle", "--files-only"]);
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0));
    assert_eq!(content_lines(&text), 1, "one path: {text}");
    assert!(text.contains("a.txt"), "{text}");
    assert!(text.contains("1 file · 2 matches"), "{text}");
}

#[test]
fn search_truncation_is_announced() {
    let fixture = Fixture::new("search-truncated");
    fixture.write("a.txt", &"needle\n".repeat(10));

    let out = at_peek(fixture.path(), "search", &["needle", "--limit", "2"]);
    let text = stdout(&out);

    assert_eq!(content_lines(&text), 2);
    assert!(
        text.contains("2 of 10 matches in 1 file · --limit 2"),
        "{text}"
    );
}

#[test]
fn count_and_files_only_together_is_a_usage_error() {
    let fixture = Fixture::new("search-modes");
    fixture.write("a.txt", "needle\n");

    let out = at_peek(
        fixture.path(),
        "search",
        &["needle", "--count", "--files-only"],
    );

    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("pass one"), "{}", stderr(&out));
}

#[test]
fn an_invalid_regex_is_a_usage_error() {
    let fixture = Fixture::new("search-bad-regex");
    fixture.write("a.txt", "needle\n");

    let out = at_peek(fixture.path(), "search", &["needle("]);

    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("not a valid regex"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn search_names_the_directories_it_skipped_by_rule() {
    let fixture = Fixture::new("search-skip");
    fixture.write("src/a.rs", "needle\n");
    fixture.write("target/generated.rs", "needle\n");

    let out = at_peek(fixture.path(), "search", &["needle"]);
    let text = stdout(&out);

    assert!(
        !text.contains("target/generated.rs"),
        "a build tree is not searched: {text}"
    );
    assert!(text.contains("skipped by rule: target"), "{text}");
}

#[test]
fn search_respects_the_deny_list_and_can_be_forced() {
    let fixture = Fixture::new("search-secrets");
    fixture.write(".env", "TOKEN=needle\n");
    fixture.write("a.txt", "needle\n");

    let out = at_peek(fixture.path(), "search", &["needle"]);
    let text = stdout(&out);
    assert!(!text.contains("TOKEN=needle"), "{text}");
    assert!(text.contains("skipped: 1 secret-shaped"), "{text}");

    let forced = at_peek(
        fixture.path(),
        "search",
        &["needle", "--include-secret-paths"],
    );
    assert!(
        stdout(&forced).contains("TOKEN=needle"),
        "{}",
        stdout(&forced)
    );
}

#[test]
fn search_stops_at_max_files_and_says_so() {
    let fixture = Fixture::new("search-max-files");
    for name in ["a", "b", "c", "d", "e"] {
        fixture.write(&format!("{name}.txt"), "needle\n");
    }

    let out = at_peek(fixture.path(), "search", &["needle", "--max-files", "2"]);
    let text = stdout(&out);

    assert!(text.contains("stopped after 2 files considered"), "{text}");
}

#[test]
fn search_counts_a_binary_file_rather_than_printing_it() {
    let fixture = Fixture::new("search-binary");
    fixture.write_bytes("blob.bin", b"needle\x00needle\n");

    let out = at_peek(fixture.path(), "search", &["needle"]);
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(1), "nothing printable: {text}");
    assert!(text.contains("skipped: 1 binary"), "{text}");
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
