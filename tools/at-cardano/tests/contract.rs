//! `at-cardano` is allowlisted for what its grammar can do, so its tests are about that
//! grammar: every noun and aspect has help, a write is refused before a socket opens, the key
//! is named but never printed, every published endpoint is answered, and there is no write API
//! in the source.

use std::path::{Path, PathBuf};
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

fn temp_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "at-cardano-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).expect("create temp root");
    root
}

/// A verb name is one noun and one aspect: `tip`, `pool`, `pool delegators`.
fn words(verb: &at_cardano::catalogue::Verb) -> Vec<&'static str> {
    verb.name.split(' ').collect()
}

#[test]
fn the_help_lists_every_noun() {
    let out = at_cardano(&["--help"]);
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    for noun in at_cardano::catalogue::nouns() {
        assert!(text.contains(noun.name), "{} is not listed", noun.name);
    }
    assert!(text.contains("/submittx refused"), "{text}");
}

#[test]
fn every_verb_has_its_own_help() {
    for verb in at_cardano::catalogue::verbs() {
        let mut args = words(verb);
        args.push("--help");
        let out = at_cardano(&args);
        let text = stdout(&out);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{} --help: {}",
            verb.name,
            stderr(&out)
        );
        assert!(
            text.contains(&format!("at-cardano {}", verb.name)),
            "{} help does not name itself: {text}",
            verb.name
        );
        assert!(
            text.contains(verb.usage),
            "{} help does not carry its usage: {text}",
            verb.name
        );
    }
}

#[test]
fn every_noun_help_lists_its_aspects() {
    for noun in at_cardano::catalogue::nouns() {
        let out = at_cardano(&["help", noun.name]);
        let text = stdout(&out);
        assert_eq!(
            out.status.code(),
            Some(0),
            "help {}: {}",
            noun.name,
            stderr(&out)
        );
        for aspect in noun.aspects() {
            assert!(
                text.contains(aspect),
                "`{}` help does not list `{aspect}`: {text}",
                noun.name
            );
        }
    }
}

#[test]
fn a_noun_without_a_principal_question_names_its_aspects() {
    let out = at_cardano(&["proposal"]);

    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("needs an aspect"), "{}", stderr(&out));
    assert!(stderr(&out).contains("summary"), "{}", stderr(&out));
}

#[test]
fn a_mistyped_aspect_names_the_aspects() {
    let out = at_cardano(&["pool", "deegators"]);

    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("aspects of `pool`"),
        "{}",
        stderr(&out)
    );
    assert!(stderr(&out).contains("delegators"), "{}", stderr(&out));
}

#[test]
fn an_unknown_noun_is_a_usage_error_that_names_what_is_known() {
    let out = at_cardano(&["tipx"]);

    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("unknown noun `tipx`"),
        "{}",
        stderr(&out)
    );
    assert!(stderr(&out).contains("network"), "{}", stderr(&out));
    assert!(stderr(&out).contains("pools"), "{}", stderr(&out));
}

#[test]
fn an_unknown_flag_is_a_usage_error_that_names_the_flags() {
    let out = at_cardano(&["tip", "--chain", "mainnet"]);

    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("unknown flag `--chain`"),
        "{}",
        stderr(&out)
    );
    assert!(stderr(&out).contains("--network"), "{}", stderr(&out));
}

#[test]
fn transaction_submission_is_refused_before_a_request() {
    let out = at_cardano(&["query", "submittx"]);

    assert_eq!(out.status.code(), Some(4), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("writes to the chain"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn an_endpoint_is_a_name_not_a_url() {
    let out = at_cardano(&["query", "https://api.koios.rest/api/v1/tx_info"]);

    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("not a Koios endpoint name"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn the_endpoint_table_answers_every_read_but_the_documented_long_tail() {
    let out = at_cardano(&["endpoints"]);
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let mut unanswerable: Vec<&str> = text
        .lines()
        .filter(|line| line.ends_with(" query"))
        .map(|line| line.split_whitespace().next().unwrap_or(""))
        .collect();
    unanswerable.sort_unstable();
    assert_eq!(
        unanswerable,
        vec![
            "ogmios",
            "reserve_withdrawals",
            "treasury_withdrawals",
            "tx_outs_epoch"
        ],
        "only the Ogmios proxy and the three endpoints Koios will not answer are left to query"
    );
    assert!(text.contains("submittx"), "{text}");
    assert!(text.contains("refused"), "{text}");
}

#[test]
fn the_key_file_is_named_and_never_printed() {
    let root = temp_root("key");
    let config = root.join("config");
    std::fs::create_dir_all(config.join("koios")).expect("create config dir");
    let key = "sentinel-key-that-must-not-appear";
    std::fs::write(config.join("koios/api_key"), format!("{key}\n")).expect("write key");

    let out = Command::new(BIN)
        .args(["auth", "--network", "preprod"])
        .env_remove("KOIOS_API_KEY")
        .env("HOME", &root)
        .env("XDG_CONFIG_HOME", &config)
        .output()
        .expect("run auth");

    let text = stdout(&out);
    let errors = stderr(&out);
    assert_eq!(out.status.code(), Some(0), "{errors}");
    assert!(text.contains("key from"), "{text}");
    assert!(text.contains("config/koios/api_key"), "{text}");
    assert!(!text.contains(key), "the key was printed: {text}");
    assert!(!errors.contains(key), "the key was printed: {errors}");

    std::fs::remove_dir_all(root).expect("clean up");
}

#[test]
fn a_shared_key_file_is_reported_as_a_caveat() {
    let root = temp_root("mode");
    let config = root.join("config");
    std::fs::create_dir_all(config.join("koios")).expect("create config dir");
    let path = config.join("koios/api_key");
    std::fs::write(&path, "sentinel\n").expect("write key");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("chmod shared");
    }

    let out = Command::new(BIN)
        .args(["auth"])
        .env_remove("KOIOS_API_KEY")
        .env("HOME", &root)
        .env("XDG_CONFIG_HOME", &config)
        .output()
        .expect("run auth");

    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(0));
    #[cfg(unix)]
    assert!(text.contains("chmod 600"), "{text}");

    std::fs::remove_dir_all(root).expect("clean up");
}

#[test]
fn keyless_ignores_the_file() {
    let root = temp_root("keyless");
    let config = root.join("config");
    std::fs::create_dir_all(config.join("koios")).expect("create config dir");
    let key = "another-sentinel";
    std::fs::write(config.join("koios/api_key"), key).expect("write key");

    let out = Command::new(BIN)
        .args(["auth", "--keyless"])
        .env_remove("KOIOS_API_KEY")
        .env("HOME", &root)
        .env("XDG_CONFIG_HOME", &config)
        .output()
        .expect("run auth");

    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(0));
    assert!(text.contains("keyless (--keyless)"), "{text}");
    assert!(!text.contains(key), "{text}");

    std::fs::remove_dir_all(root).expect("clean up");
}

#[test]
fn all_and_json_are_alternatives() {
    let out = at_cardano(&["address", "utxos", "addr1example", "--all", "--json"]);

    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("--all and --json are alternatives"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_zero_limit_is_a_usage_error() {
    let out = at_cardano(&["address", "utxos", "addr1example", "--limit", "0"]);

    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("--limit 0"), "{}", stderr(&out));
}

#[test]
fn an_asset_unit_needs_a_policy() {
    let out = at_cardano(&["asset", "not-a-policy"]);

    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("not an asset unit"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_mixed_block_batch_is_refused() {
    let hash = "a".repeat(64);
    let out = at_cardano(&["block", &hash, "14100188"]);

    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("all hashes or all heights"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_policy_verb_takes_one_policy() {
    let out = at_cardano(&[
        "policy",
        "c364930bd612f42e14d156e1c5410511e77f64cab8f2367a9df544d1",
        "ab",
    ]);

    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("at most 1"), "{}", stderr(&out));
}

#[test]
fn version_is_a_version() {
    let out = at_cardano(&["--version"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).starts_with("at-cardano "), "{}", stdout(&out));
}

#[test]
fn no_write_api_in_the_source() {
    const FORBIDDEN: &[&str] = &[
        "fs::write",
        "File::create",
        "OpenOptions",
        "create_dir",
        "remove_file",
        "remove_dir",
        "fs::rename",
        "std::process::Command",
        "Command::new",
        "TcpStream",
        "UdpSocket",
        "std::net",
    ];

    for (name, text) in source_files() {
        for needle in FORBIDDEN {
            assert!(
                !text.contains(needle),
                "{name} contains `{needle}`; a read-only binary may not have a write path"
            );
        }
    }
}

#[test]
fn ureq_is_confined_to_the_adaptor() {
    for (name, text) in source_files() {
        if name != "koios.rs" {
            assert!(
                !text.contains("ureq::"),
                "{name} reaches the network directly; only koios.rs may"
            );
        }
    }
}

/// Every source file, submodules included — the module split is a structure choice, not a
/// narrower guarantee.
fn source_files() -> Vec<(String, String)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect(&src, &mut files);
    files
}

fn collect(directory: &Path, files: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(directory).expect("read source directory") {
        let entry = entry.expect("directory entry");
        let path = entry.path();
        if path.is_dir() {
            collect(&path, files);
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("file name")
            .to_string();
        let text = std::fs::read_to_string(&path).expect("read source");
        files.push((name, text));
    }
}
