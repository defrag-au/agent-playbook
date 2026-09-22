//! `at-describe` is the toolkit's catalogue, so its tests are about the catalogue being the
//! one the binaries enforce: every verb listed, and nothing listed that is not there.

use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_at-describe");

fn describe(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("run at-describe")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn the_toolkit_view_lists_every_verb_in_the_catalogue() {
    let out = describe(&[]);
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0));
    for verb in at_peek::catalogue::VERBS {
        assert!(text.contains(verb.name), "{} is not described", verb.name);
    }
    assert!(
        text.contains("read-only"),
        "the namespace contract is stated: {text}"
    );
    assert!(text.contains("Exit codes"), "{text}");
}

#[test]
fn one_binary_can_be_asked_for_its_flags() {
    let out = describe(&["at-peek"]);
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0));
    assert!(text.contains("--limit"), "{text}");
    assert!(text.contains("--root"), "{text}");
}

#[test]
fn one_verb_can_be_asked_for_on_its_own() {
    let out = describe(&["slice"]);
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0));
    assert!(text.contains("at-peek slice <path>:40-60"), "{text}");
    assert!(text.contains("1-based"), "the notes come with it: {text}");
}

#[test]
fn an_unknown_name_is_a_usage_error_that_names_what_is_known() {
    let out = describe(&["statx"]);

    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("unknown `statx`"), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("stat"),
        "it lists what it does know: {}",
        stderr(&out)
    );
}
