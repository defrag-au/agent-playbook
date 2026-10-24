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
    for verb in at_crate::catalogue::VERBS {
        assert!(text.contains(verb.name), "{} is not described", verb.name);
    }
    for verb in at_recall::catalogue::VERBS {
        assert!(text.contains(verb.name), "{} is not described", verb.name);
    }
    for verb in at_reach::catalogue::VERBS {
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
    // The promises differ — one runs nothing, one reads a cargo store, one runs git — and an
    // allowlist decision needs the difference rather than a shared slogan.
    let text = stdout(&describe(&[]));
    assert!(text.contains("No git, no subprocess"), "{text}");
    assert!(
        text.contains("No subprocess, no network, no writes"),
        "{text}"
    );
    assert!(text.contains("One subprocess, git"), "{text}");
}

#[test]
fn every_binary_can_be_asked_for_its_flags() {
    let peek = stdout(&describe(&["at-peek"]));
    assert!(peek.contains("--limit"), "{peek}");

    let source = stdout(&describe(&["at-crate"]));
    assert!(source.contains("--cargo-home"), "{source}");
    assert!(source.contains("--build"), "{source}");

    let recall = stdout(&describe(&["at-recall"]));
    assert!(recall.contains("--patch"), "{recall}");
    assert!(recall.contains("diff"), "{recall}");

    let reach = stdout(&describe(&["at-reach"]));
    assert!(reach.contains("--fleet-root"), "{reach}");
}

#[test]
fn a_verb_is_found_whichever_binary_owns_it() {
    let slice = stdout(&describe(&["slice"]));
    assert!(slice.contains("at-peek slice"), "{slice}");

    let diff = stdout(&describe(&["diff"]));
    assert!(diff.contains("at-recall diff"), "{diff}");
    assert!(diff.contains("revision"), "the notes come with it: {diff}");

    let location = stdout(&describe(&["where"]));
    assert!(location.contains("at-crate where"), "{location}");
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
fn the_crate_binary_can_be_asked_for_its_flags() {
    // `where` is at-crate's alone, so this is the flag table rather than a summary usage line.
    let out = describe(&["where"]);
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0));
    assert!(text.contains("at-crate where"), "{text}");
    assert!(text.contains("--cargo-home"), "{text}");
    assert!(text.contains("--version"), "{text}");
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
fn the_catalogue_lists_the_recipes_with_their_sections() {
    // A recipe that is built but not described is the silent case this suite exists for: the agent
    // looking for it asks `at-describe` first and would conclude it does not exist.
    let text = stdout(&describe(&[]));

    assert!(!text.contains("none in this build"), "{text}");
    for recipe in at_recall::catalogue::RECIPES {
        assert!(
            text.contains(recipe.verb),
            "{} is not described: {text}",
            recipe.verb
        );
        assert!(
            at_recall::catalogue::VERBS
                .iter()
                .any(|verb| verb.name == recipe.verb),
            "{} is a recipe for a verb that does not exist",
            recipe.verb
        );
        for section in recipe.sections {
            assert!(text.contains(section), "{section} is not described: {text}");
        }
    }
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
