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
fn the_toolkit_view_lists_every_verb_in_every_binary() {
    let out = describe(&[]);
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0));
    for verb in at_peek::catalogue::VERBS {
        assert!(text.contains(verb.name), "{} is not described", verb.name);
    }
    for verb in at_recall::catalogue::VERBS {
        assert!(text.contains(verb.name), "{} is not described", verb.name);
    }
    assert!(
        text.contains("read-only"),
        "the namespace contract is stated: {text}"
    );
    assert!(text.contains("Exit codes"), "{text}");
}

#[test]
fn each_binary_states_what_it_does_not_do() {
    // The two promises differ — one runs nothing, the other runs git — and an allowlist decision
    // needs the difference rather than a shared slogan.
    let text = stdout(&describe(&[]));
    assert!(text.contains("No git, no subprocess"), "{text}");
    assert!(text.contains("One subprocess, git"), "{text}");
}

#[test]
fn both_binaries_can_be_asked_for_their_flags() {
    let peek = stdout(&describe(&["at-peek"]));
    assert!(peek.contains("--limit"), "{peek}");

    let recall = stdout(&describe(&["at-recall"]));
    assert!(recall.contains("--patch"), "{recall}");
    assert!(recall.contains("diff"), "{recall}");
}

#[test]
fn a_verb_is_found_whichever_binary_owns_it() {
    let slice = stdout(&describe(&["slice"]));
    assert!(slice.contains("at-peek slice"), "{slice}");

    let diff = stdout(&describe(&["diff"]));
    assert!(diff.contains("at-recall diff"), "{diff}");
    assert!(diff.contains("revision"), "the notes come with it: {diff}");
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
        stderr(&out).contains("stat") && stderr(&out).contains("diff"),
        "it lists what it does know: {}",
        stderr(&out)
    );
}
