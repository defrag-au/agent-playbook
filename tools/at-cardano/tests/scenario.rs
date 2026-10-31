//! `at-cardano run` is a grammar over the same verbs, so its tests are about the parts that do
//! not need a socket: the flag, the file, and the errors that are reported before any request.

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_at-cardano");

fn at_cardano(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env_remove("KOIOS_API_KEY")
        .output()
        .expect("run at-cardano")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn temp_scenario(label: &str, name: &str, text: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "at-cardano-scenario-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).expect("create temp root");
    let path = root.join(name);
    std::fs::write(&path, text).expect("write scenario");
    path
}

#[test]
fn run_needs_a_scenario_flag() {
    let out = at_cardano(&["run"]);

    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("--scenario"), "{}", stderr(&out));
}

#[test]
fn run_takes_no_positional() {
    let out = at_cardano(&["run", "somewhere"]);

    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("takes no target"), "{}", stderr(&out));
}

#[test]
fn a_scenario_must_be_a_dsl_file() {
    let path = temp_scenario("ext", "plain.txt", "run\necho hi\n");
    let out = at_cardano(&["run", "--scenario", path.to_str().expect("utf8")]);

    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("not a `.dsl` file"),
        "{}",
        stderr(&out)
    );

    std::fs::remove_dir_all(path.parent().expect("parent")).expect("clean up");
}

#[test]
fn a_parse_error_names_the_file_line_and_column() {
    let path = temp_scenario("parse", "bad.dsl", "run\ntarget t typo\n");
    let out = at_cardano(&["run", "--scenario", path.to_str().expect("utf8")]);

    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("bad.dsl:2:"), "{}", stderr(&out));
    assert!(stderr(&out).contains("is not a noun"), "{}", stderr(&out));

    std::fs::remove_dir_all(path.parent().expect("parent")).expect("clean up");
}

#[test]
fn a_scenario_with_no_targets_runs_its_echoes() {
    let path = temp_scenario(
        "echo",
        "echo.dsl",
        "run timeout=5s\necho \"scenario in, scenario out\"\n",
    );
    let out = at_cardano(&["run", "--scenario", path.to_str().expect("utf8")]);

    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(text.contains("scenario in, scenario out"), "{text}");
    assert!(text.contains("0 targets"), "{text}");
    assert!(text.contains("1 passed"), "{text}");

    std::fs::remove_dir_all(path.parent().expect("parent")).expect("clean up");
}

#[test]
fn a_check_names_a_valid_scenario_without_a_request() {
    let path = temp_scenario(
        "check",
        "ok.dsl",
        "run network=mainnet\ntarget t tip\nassert t count=1\n",
    );
    let out = at_cardano(&["run", "--check", "--scenario", path.to_str().expect("utf8")]);

    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(text.contains("no request was made"), "{text}");
    assert!(text.contains("1 target"), "{text}");
    assert!(
        !text.contains("Koios /tip"),
        "the check made a request: {text}"
    );

    std::fs::remove_dir_all(path.parent().expect("parent")).expect("clean up");
}

#[test]
fn a_check_reports_a_parse_error_without_a_request() {
    let path = temp_scenario("check-bad", "bad.dsl", "target t typo\n");
    let out = at_cardano(&["run", "--check", "--scenario", path.to_str().expect("utf8")]);

    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("bad.dsl:1:"), "{}", stderr(&out));
    assert!(stderr(&out).contains("is not a noun"), "{}", stderr(&out));
    assert!(stdout(&out).is_empty(), "{}", stdout(&out));

    std::fs::remove_dir_all(path.parent().expect("parent")).expect("clean up");
}

#[test]
fn run_help_names_the_flag_and_the_grammar() {
    let out = at_cardano(&["run", "--help"]);

    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(text.contains("--scenario"), "{text}");
    assert!(text.contains("`target` lines"), "{text}");
}
