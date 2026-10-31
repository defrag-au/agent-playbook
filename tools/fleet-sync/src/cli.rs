//! Argument parsing — a closed grammar, in the same shape as the other tools.
//!
//! Flags come after the verb. There is no `--` pass-through, no shell, and no host id that is
//! not first looked up in the private fleet tree.

use std::collections::BTreeMap;
use std::path::Path;

use at_core::contract::{Exit, Fail};
use at_reach::fleet::Fleet;

use crate::bundle;
use crate::target::{self, Clean};
use crate::{TOOL, VERSION};

const VERBS: &[&str] = &["bundle", "plan", "apply"];

const HELP: &str = "\
fleet-sync — seed and resync a read-only fleet satellite

usage:
  fleet-sync bundle <host> --out <dir> [--source-root <path>] [--ref <ref>] [--fleet-root <path>]
  fleet-sync plan   <host> --from <dir> [--clean artifacts|reset|none] [--force] [--allow-writer] [--include-env] [--fleet-root <path>]
  fleet-sync apply  <host> --from <dir> [--clean artifacts|reset|none] [--force] [--allow-writer] [--include-env] [--fleet-root <path>]
  fleet-sync help [bundle|plan|apply]
  fleet-sync --version

bundle runs on the machine that has the work and writes git bundles plus a manifest; it never
touches the target. plan reads that payload on the target and reports what apply would do.
apply writes only the checkout paths in the profile, and refuses unless the local hostname
matches the profile and the profile is not a writer. No command contacts another machine.
";

const BUNDLE_HELP: &str = "\
fleet-sync bundle — write the payload on the machine that has the work

usage:
  fleet-sync bundle <host> --out <dir> [--source-root <path>] [--ref <ref>] [--fleet-root <path>]

The profile supplies the repositories and their paths. --source-root overrides repo_root when
the checkouts are not where the target keeps them. --ref names the branch to bundle; the default
is the checked-out branch. The bundle carries commits, not working-tree changes.

The payload is a directory of .bundle files and one flat `manifest`. Copy it to the target on
an encrypted volume, or by whatever path the target already trusts.
";

const PLAN_HELP: &str = "\
fleet-sync plan — report what apply would do, without writing

usage:
  fleet-sync plan <host> --from <dir> [--clean artifacts|reset|none] [--force] [--allow-writer] [--include-env] [--fleet-root <path>]

Runs only on the host the profile names. Reports each repository's action, current and target
commit, tracked changes, reclaimable artifacts and free space. Exit 4 means at least one row is
blocked, and nothing would be written.

--clean defaults to the profile's sync_clean, then to artifacts. --force allows a checkout with
tracked changes. --allow-writer permits a machine whose writes is not `none`.
";

const APPLY_HELP: &str = "\
fleet-sync apply — clean the target's artifacts and reset its checkouts to the payload

usage:
  fleet-sync apply <host> --from <dir> [--clean artifacts|reset|none] [--force] [--allow-writer] [--include-env] [--fleet-root <path>]

--clean artifacts (the default) removes target, .direnv, node_modules, .tmp and .venv.
--clean reset also runs git clean -xfd, protecting .env unless --include-env is given.
Refuses on a machine that writes a repository, on a checkout with tracked changes, and when
the payload plus headroom does not fit. The checksum of every bundle is verified before a
checkout is touched.
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

    match first.as_str() {
        "-h" | "--help" => {
            print!("{HELP}");
            return Ok(Exit::Results);
        }
        "help" => {
            print!("{}", help_for(args.get(1).map(String::as_str))?);
            return Ok(Exit::Results);
        }
        "--version" => {
            println!("{TOOL} {VERSION}");
            return Ok(Exit::Results);
        }
        _ => {}
    }

    if !VERBS.contains(&first.as_str()) {
        return Err(Fail::usage(format!(
            "unknown verb `{first}` · verbs: {}",
            VERBS.join(", ")
        )));
    }

    let parsed = parse(first, &args[1..])?;
    if parsed.has("--help") {
        print!("{}", help_for(Some(first))?);
        return Ok(Exit::Results);
    }

    let host = match parsed.positionals.as_slice() {
        [host] => host.clone(),
        [] => {
            return Err(Fail::usage(format!(
                "`{first}` needs one host · see `{TOOL} help {first}`"
            )))
        }
        _ => return Err(Fail::usage(format!("`{first}` takes one host at a time"))),
    };

    let fleet = Fleet::resolve(parsed.value("--fleet-root").map(Path::new))?;

    let outcome = match first.as_str() {
        "bundle" => {
            let out = required(&parsed, "--out", first)?;
            bundle::run(
                &host,
                Path::new(&out),
                parsed.value("--source-root").map(Path::new),
                parsed.value("--ref"),
                &fleet,
            )?
        }
        "plan" | "apply" => {
            let from = required(&parsed, "--from", first)?;
            let clean = parsed.value("--clean").map(Clean::parse).transpose()?;
            let force = parsed.has("--force");
            let allow_writer = parsed.has("--allow-writer");
            let include_env = parsed.has("--include-env");
            if first == "plan" {
                target::plan(
                    &host,
                    Path::new(&from),
                    clean,
                    force,
                    allow_writer,
                    include_env,
                    &fleet,
                )?
            } else {
                target::apply(
                    &host,
                    Path::new(&from),
                    clean,
                    force,
                    allow_writer,
                    include_env,
                    &fleet,
                )?
            }
        }
        other => {
            return Err(Fail::usage(format!(
                "`{other}` is catalogued but not implemented"
            )))
        }
    };

    let rendered = outcome.report.render();
    if !rendered.is_empty() {
        print!("{rendered}");
    }
    Ok(outcome.exit)
}

fn required(parsed: &Parsed, flag: &str, verb: &str) -> Result<String, Fail> {
    parsed
        .value(flag)
        .map(str::to_string)
        .ok_or_else(|| Fail::usage(format!("`{verb}` needs {flag} · see `{TOOL} help {verb}`")))
}

fn help_for(verb: Option<&str>) -> Result<String, Fail> {
    match verb {
        None => Ok(HELP.to_string()),
        Some("bundle") => Ok(BUNDLE_HELP.to_string()),
        Some("plan") => Ok(PLAN_HELP.to_string()),
        Some("apply") => Ok(APPLY_HELP.to_string()),
        Some(other) => Err(Fail::usage(format!(
            "unknown verb `{other}` · verbs: {}",
            VERBS.join(", ")
        ))),
    }
}

#[derive(Debug, Default)]
struct Parsed {
    positionals: Vec<String>,
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

fn flags(verb: &str) -> Vec<(&'static str, bool)> {
    let mut flags = vec![("--fleet-root", true), ("--help", false)];
    match verb {
        "bundle" => flags.extend([("--out", true), ("--source-root", true), ("--ref", true)]),
        "plan" | "apply" => flags.extend([
            ("--from", true),
            ("--clean", true),
            ("--force", false),
            ("--allow-writer", false),
            ("--include-env", false),
        ]),
        _ => {}
    }
    flags
}

fn parse(verb: &str, args: &[String]) -> Result<Parsed, Fail> {
    let allowed = flags(verb);
    let mut parsed = Parsed::default();
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        if arg == "-" {
            return Err(Fail::usage(
                "`-` means stdin, which fleet-sync does not read · pass a host id",
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
                "unknown flag `{arg}` for `{verb}` · flags: {}",
                flag_names(&allowed)
            )));
        } else {
            parsed.positionals.push(arg.clone());
            index += 1;
            continue;
        };

        let Some((_, takes_value)) = allowed.iter().find(|(flag, _)| *flag == name.as_str()) else {
            return Err(Fail::usage(format!(
                "unknown flag `{arg}` for `{verb}` · flags: {}",
                flag_names(&allowed)
            )));
        };

        let value = if *takes_value {
            match inline {
                Some(value) => Some(value),
                None => {
                    index += 1;
                    match args.get(index) {
                        Some(value) => Some(value.clone()),
                        None => {
                            return Err(Fail::usage(format!("`{name}` needs a value")));
                        }
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

fn flag_names(allowed: &[(&str, bool)]) -> String {
    allowed
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flag_with_a_value_is_parsed() {
        let parsed = parse(
            "bundle",
            &[
                "defrag-air".to_string(),
                "--out".to_string(),
                "/Volumes/travel/air".to_string(),
            ],
        )
        .expect("parse");
        assert_eq!(parsed.positionals, ["defrag-air"]);
        assert_eq!(parsed.value("--out"), Some("/Volumes/travel/air"));
    }

    #[test]
    fn an_inline_value_is_parsed() {
        let parsed = parse(
            "plan",
            &["defrag-air".to_string(), "--from=/tmp/x".to_string()],
        )
        .expect("parse");
        assert_eq!(parsed.value("--from"), Some("/tmp/x"));
    }

    #[test]
    fn a_boolean_flag_takes_no_value() {
        let parsed = parse(
            "apply",
            &[
                "defrag-air".to_string(),
                "--from".to_string(),
                "/tmp/x".to_string(),
                "--force".to_string(),
            ],
        )
        .expect("parse");
        assert!(parsed.has("--force"));
    }

    #[test]
    fn an_unknown_flag_is_named() {
        let error = parse("plan", &["--wat".to_string()]).expect_err("unknown");
        assert!(error.message.contains("unknown flag `--wat`"));
    }

    #[test]
    fn a_boolean_flag_refuses_a_value() {
        let error = parse("apply", &["--force=yes".to_string()]).expect_err("no value");
        assert!(error.message.contains("takes no value"));
    }

    #[test]
    fn a_duplicate_flag_is_an_error() {
        let error = parse(
            "plan",
            &[
                "--from".to_string(),
                "/a".to_string(),
                "--from".to_string(),
                "/b".to_string(),
            ],
        )
        .expect_err("duplicate");
        assert!(error.message.contains("given twice"));
    }

    #[test]
    fn a_bare_dash_is_refused() {
        let error = parse("plan", &["-".to_string()]).expect_err("stdin");
        assert!(error.message.contains("stdin"));
    }

    #[test]
    fn help_is_a_verb() {
        assert!(help_for(Some("bundle"))
            .expect("help")
            .contains("fleet-sync bundle"));
        assert!(help_for(Some("nope")).is_err());
    }
}
