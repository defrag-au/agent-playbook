//! The grammar and the exit codes, run against the built binary. The digest itself is tested beside
//! the format in `zed-thread-tools`, because it needs a decoded thread rather than a store.

use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_at-continue-thread"))
        .args(args)
        .output()
        .expect("running at-continue-thread")
}

#[test]
fn help_lists_both_verbs() {
    let output = run(&["--help"]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("digest"));
    assert!(text.contains("find"));
}

#[test]
fn an_unknown_verb_is_a_usage_error() {
    let output = run(&["resume"]);
    assert_eq!(output.status.code(), Some(2));
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(text.contains("unknown verb `resume`"));
}

#[test]
fn a_missing_query_is_a_usage_error() {
    let output = run(&["digest"]);
    assert_eq!(output.status.code(), Some(2));
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(text.contains("needs a thread id or a title fragment"));
}

#[test]
fn an_unknown_flag_is_refused_by_name() {
    let output = run(&["digest", "x", "--budget"]);
    assert_eq!(output.status.code(), Some(2));
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(text.contains("unknown flag `--budget`"));
}

#[test]
fn a_missing_store_is_an_environment_error() {
    let output = run(&["find", "anything", "--db", "/nonexistent/threads.db"]);
    assert_eq!(output.status.code(), Some(3));
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(text.contains("thread database not found"));
}
