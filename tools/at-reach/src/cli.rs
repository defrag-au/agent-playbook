//! Argument parsing — a closed grammar.
//!
//! The flags a verb accepts come from the catalogue, and there is no `--` pass-through. The
//! only thing this parser hands to a verb is a fleet root and, for `probe`, one host id from
//! the private inventory. A host id cannot name an arbitrary SSH target: it is looked up in the
//! fleet tree, and only `ssh` from the profile is used.

use std::path::Path;

use at_core::contract::{Exit, Fail};

use crate::catalogue::{self, Verb};
use crate::fleet::Fleet;
use crate::probe;

pub fn run(args: &[String]) -> i32 {
    match dispatch(args) {
        Ok(exit) => exit.code(),
        Err(fail) => {
            eprintln!("{}: {}", crate::TOOL, fail.message);
            fail.exit.code()
        }
    }
}

fn dispatch(args: &[String]) -> Result<Exit, Fail> {
    let Some(first) = args.first() else {
        print!("{}", catalogue::help_all());
        return Ok(Exit::Results);
    };

    match first.as_str() {
        "-h" | "--help" => {
            print!("{}", catalogue::help_all());
            return Ok(Exit::Results);
        }
        "help" => {
            let rendered = match args.get(1) {
                Some(name) => {
                    catalogue::help_for(catalogue::find(name).ok_or_else(|| unknown(name))?)
                }
                None => catalogue::help_all(),
            };
            print!("{rendered}");
            return Ok(Exit::Results);
        }
        "--version" => {
            println!("{} {}", crate::TOOL, crate::VERSION);
            return Ok(Exit::Results);
        }
        _ => {}
    }

    let verb = catalogue::find(first).ok_or_else(|| unknown(first))?;
    let parsed = parse(verb, &args[1..])?;

    if parsed.has("--help") {
        print!("{}", catalogue::help_for(verb));
        return Ok(Exit::Results);
    }

    let fleet = fleet(&parsed)?;

    let outcome = match verb.name {
        "root" => {
            if !parsed.positionals.is_empty() {
                return Err(Fail::usage(format!(
                    "`root` takes no host · {}",
                    verb.usage
                )));
            }
            probe::root(&fleet)
        }
        "hosts" => {
            if !parsed.positionals.is_empty() {
                return Err(Fail::usage(format!(
                    "`hosts` takes no host · {}",
                    verb.usage
                )));
            }
            probe::hosts(&fleet)
        }
        "probe" => {
            let host = match parsed.positionals.as_slice() {
                [host] => host,
                [] => {
                    return Err(Fail::usage(format!(
                        "`probe` needs one host · {}",
                        verb.usage
                    )))
                }
                _ => {
                    return Err(Fail::usage(format!(
                        "`probe` takes one host at a time · {}",
                        verb.usage
                    )))
                }
            };
            probe::probe(&fleet, host)?
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

fn unknown(name: &str) -> Fail {
    let hint = if name.starts_with('-') {
        " · flags come after the verb, e.g. `at-reach probe djo-flip --fleet-root <path>`"
    } else {
        ""
    };
    Fail::usage(format!(
        "unknown verb `{name}` · verbs: {}{hint}",
        catalogue::names()
    ))
}

#[derive(Debug, Default)]
struct Parsed {
    positionals: Vec<String>,
    flags: Vec<(&'static str, Option<String>)>,
}

impl Parsed {
    fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|(flag, _)| *flag == name)
    }

    fn value(&self, name: &str) -> Option<&str> {
        self.flags
            .iter()
            .find(|(flag, _)| *flag == name)
            .and_then(|(_, value)| value.as_deref())
    }
}

fn parse(verb: &'static Verb, args: &[String]) -> Result<Parsed, Fail> {
    let mut parsed = Parsed::default();
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        if let Some(rest) = arg.strip_prefix('-') {
            if rest.is_empty() {
                return Err(Fail::usage(
                    "`-` means stdin, which at-reach does not read · pass a host id",
                ));
            }
            let (name, inline) = match arg.split_once('=') {
                Some((name, value)) => (name.to_string(), Some(value.to_string())),
                None => (arg.to_string(), None),
            };
            let name = if name == "-h" {
                "--help".to_string()
            } else {
                name
            };
            let flag = verb
                .flags
                .iter()
                .find(|flag| flag.name == name)
                .ok_or_else(|| {
                    Fail::usage(format!(
                        "unknown flag `{arg}` for `{}` · flags: {}",
                        verb.name,
                        catalogue::flag_names(verb)
                    ))
                })?;

            let value = if flag.takes_value {
                match inline {
                    Some(value) => Some(value),
                    None => {
                        index += 1;
                        match args.get(index) {
                            Some(value) => Some(value.clone()),
                            None => {
                                return Err(Fail::usage(format!("`{}` needs a value", flag.name)))
                            }
                        }
                    }
                }
            } else {
                if inline.is_some() {
                    return Err(Fail::usage(format!("`{}` takes no value", flag.name)));
                }
                None
            };
            parsed.flags.push((flag.name, value));
        } else {
            parsed.positionals.push(arg.clone());
        }
        index += 1;
    }

    Ok(parsed)
}

fn fleet(parsed: &Parsed) -> Result<Fleet, Fail> {
    let explicit = parsed.value("--fleet-root").map(Path::new);
    Fleet::resolve(explicit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verb(name: &str) -> &'static Verb {
        catalogue::find(name).expect("a catalogued verb")
    }

    #[test]
    fn a_flag_with_a_value_is_parsed() {
        let parsed = parse(
            verb("probe"),
            &[
                "djo-flip".to_string(),
                "--fleet-root".to_string(),
                "/fleet".to_string(),
            ],
        )
        .expect("a parse");
        assert_eq!(parsed.positionals, ["djo-flip"]);
        assert_eq!(parsed.value("--fleet-root"), Some("/fleet"));
    }

    #[test]
    fn an_unknown_flag_is_named() {
        let error = parse(verb("probe"), &["--wat".to_string()]).expect_err("unknown flag");
        assert!(error.message.contains("unknown flag `--wat`"));
    }

    #[test]
    fn a_flag_that_takes_no_value_refuses_one() {
        let error = parse(verb("probe"), &["--help=yes".to_string()]).expect_err("no value");
        assert!(error.message.contains("takes no value"));
    }
}
