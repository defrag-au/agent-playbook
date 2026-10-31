//! Argument parsing. Flags only: this tool has one job and no verbs.

use std::collections::BTreeMap;
use std::path::Path;

use at_core::contract::{Exit, Fail};

use crate::payload;
use crate::{TOOL, VERSION};

const HELP: &str = "\
travel-secrets — build a whitelisted, encrypted pass-store subset for travel

usage:
  travel-secrets --store <dir> --whitelist <file> --recipient <fingerprint> --out <dir> \
[--allow-non-tty]

The source store is opened read-only. Each whitelisted entry is decrypted with the keyring gpg
finds in GNUPGHOME and re-encrypted to --recipient through a pipe; no plaintext file is written.
The output is a self-contained pass store plus a flat `manifest`.

This tool decrypts secrets. Run it yourself from an interactive terminal. An agent must not run
it, `pass`, or `gpg --decrypt` against a real keyring; an agent may prepare the whitelist and
leave this command to the human.

  --store <dir>         the source pass store (e.g. ~/.password-store)
  --whitelist <file>    flat `pass: area,area/entry` whitelist
  --recipient <fpr>     the travel public key, already in GNUPGHOME
  --out <dir>           an empty or absent directory for `store/` and `manifest`
  --allow-non-tty       skip the interactive-terminal check (only when pinentry is arranged)
";

pub fn run(args: &[String]) -> i32 {
    match dispatch(args) {
        Ok(exit) => exit.code(),
        Err(fail) => {
            eprintln!("{TOOL}: {}", fail.message);
            fail.exit.code()
        }
    }
}

fn dispatch(args: &[String]) -> Result<Exit, Fail> {
    let Some(first) = args.first() else {
        print!("{HELP}");
        return Ok(Exit::Results);
    };

    if first == "-h" || first == "--help" {
        print!("{HELP}");
        return Ok(Exit::Results);
    }
    if first == "--version" {
        println!("{TOOL} {VERSION}");
        return Ok(Exit::Results);
    }

    let parsed = parse(args)?;
    if parsed.has("--help") {
        print!("{HELP}");
        return Ok(Exit::Results);
    }

    let store = required(&parsed, "--store")?;
    let whitelist = required(&parsed, "--whitelist")?;
    let recipient = required(&parsed, "--recipient")?;
    let out = required(&parsed, "--out")?;

    let outcome = payload::build(
        Path::new(&store),
        Path::new(&whitelist),
        &recipient,
        Path::new(&out),
        parsed.has("--allow-non-tty"),
    )?;

    let rendered = outcome.report.render();
    if !rendered.is_empty() {
        print!("{rendered}");
    }
    Ok(outcome.exit)
}

fn required(parsed: &Parsed, flag: &str) -> Result<String, Fail> {
    parsed
        .value(flag)
        .map(str::to_string)
        .ok_or_else(|| Fail::usage(format!("{flag} is required · see `{TOOL} --help`")))
}

#[derive(Debug, Default)]
struct Parsed {
    flags: BTreeMap<String, Option<String>>,
}

impl Parsed {
    fn value(&self, name: &str) -> Option<&str> {
        self.flags.get(name).and_then(|value| value.as_deref())
    }

    fn has(&self, name: &str) -> bool {
        self.flags.contains_key(name)
    }
}

fn flags() -> &'static [(&'static str, bool)] {
    &[
        ("--store", true),
        ("--whitelist", true),
        ("--recipient", true),
        ("--out", true),
        ("--allow-non-tty", false),
        ("--help", false),
    ]
}

fn parse(args: &[String]) -> Result<Parsed, Fail> {
    let allowed = flags();
    let mut parsed = Parsed::default();
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        if arg == "-" {
            return Err(Fail::usage(
                "`-` means stdin, which travel-secrets does not read",
            ));
        }

        let (name, inline) = if arg == "-h" {
            ("--help".to_string(), None)
        } else if let Some(rest) = arg.strip_prefix("--") {
            match rest.split_once('=') {
                Some((name, value)) => (format!("--{name}"), Some(value.to_string())),
                None => (arg.clone(), None),
            }
        } else if arg.starts_with('-') {
            return Err(Fail::usage(format!(
                "unknown flag `{arg}` · flags: {}",
                flag_names()
            )));
        } else {
            return Err(Fail::usage(format!(
                "`{arg}` is a positional argument · travel-secrets takes flags only"
            )));
        };

        let Some((_, takes_value)) = allowed.iter().find(|(flag, _)| *flag == name.as_str()) else {
            return Err(Fail::usage(format!(
                "unknown flag `{arg}` · flags: {}",
                flag_names()
            )));
        };

        let value = if *takes_value {
            match inline {
                Some(value) => Some(value),
                None => {
                    index += 1;
                    match args.get(index) {
                        Some(value) => Some(value.clone()),
                        None => return Err(Fail::usage(format!("`{name}` needs a value"))),
                    }
                }
            }
        } else {
            if inline.is_some() {
                return Err(Fail::usage(format!("`{name}` takes no value")));
            }
            None
        };

        if parsed.flags.insert(name.clone(), value).is_some() {
            return Err(Fail::usage(format!("`{name}` given twice")));
        }
        index += 1;
    }

    Ok(parsed)
}

fn flag_names() -> String {
    flags()
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_flag_is_parsed() {
        let parsed = parse(&[
            "--store".to_string(),
            "/store".to_string(),
            "--whitelist=/travel".to_string(),
            "--recipient".to_string(),
            "ABCD".to_string(),
            "--out".to_string(),
            "/payload".to_string(),
            "--allow-non-tty".to_string(),
        ])
        .expect("parse");
        assert_eq!(parsed.value("--store"), Some("/store"));
        assert_eq!(parsed.value("--whitelist"), Some("/travel"));
        assert_eq!(parsed.value("--recipient"), Some("ABCD"));
        assert_eq!(parsed.value("--out"), Some("/payload"));
        assert!(parsed.has("--allow-non-tty"));
    }

    #[test]
    fn a_positional_argument_is_refused() {
        let error = parse(&["store".to_string()]).expect_err("positional");
        assert!(error.message.contains("flags only"));
    }

    #[test]
    fn an_unknown_flag_is_named() {
        let error = parse(&["--wat".to_string()]).expect_err("unknown");
        assert!(error.message.contains("unknown flag `--wat`"));
    }

    #[test]
    fn a_value_flag_needs_a_value() {
        let error = parse(&["--store".to_string()]).expect_err("missing value");
        assert!(error.message.contains("needs a value"));
    }

    #[test]
    fn a_boolean_flag_refuses_a_value() {
        let error = parse(&["--allow-non-tty=yes".to_string()]).expect_err("value");
        assert!(error.message.contains("takes no value"));
    }

    #[test]
    fn a_duplicate_flag_is_an_error() {
        let error = parse(&[
            "--store".to_string(),
            "/a".to_string(),
            "--store".to_string(),
            "/b".to_string(),
        ])
        .expect_err("duplicate");
        assert!(error.message.contains("given twice"));
    }
}
