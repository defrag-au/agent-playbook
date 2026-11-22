//! Argument parsing — a closed grammar.
//!
//! The flags a verb accepts come from [`crate::catalogue`], so the parser and the help cannot
//! drift. The loop is the one `at-recall` and `at-peek` run; it is repeated here because sharing it
//! needs a third abstraction over three different verb types, and the duplication is one function.

use std::path::PathBuf;

use at_core::contract::{Exit, Fail};
use zed_thread_tools::digest::{DEFAULT_BUDGET_BYTES, MAX_BUDGET_BYTES};

use crate::catalogue::{self, Verb};
use crate::verbs::{self, Opts, DEFAULT_ROWS, MAX_ROWS};

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

    let opts = context(&parsed)?;
    let outcome = match verb.name {
        "digest" => {
            let query = one_positional(verb, &parsed)?;
            verbs::digest(&query, &opts)?
        }
        "find" => {
            let query = one_positional(verb, &parsed)?;
            verbs::find(&query, &opts)?
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

fn one_positional(verb: &'static Verb, parsed: &Parsed) -> Result<String, Fail> {
    match parsed.positionals.as_slice() {
        [query] if !query.trim().is_empty() => Ok(query.trim().to_owned()),
        [] => Err(Fail::usage(format!(
            "`{}` needs a thread id or a title fragment · {}",
            verb.name, verb.usage
        ))),
        [_] => Err(Fail::usage(format!(
            "`{}` needs a non-empty title fragment · {}",
            verb.name, verb.usage
        ))),
        _ => Err(Fail::usage(format!(
            "`{}` takes one thread id or title fragment, not several · {}",
            verb.name, verb.usage
        ))),
    }
}

fn unknown(name: &str) -> Fail {
    let hint = if name.starts_with('-') {
        " · flags come after the verb, e.g. `at-continue-thread digest <title> --budget-kb 96`"
    } else {
        ""
    };
    Fail::usage(format!(
        "unknown verb `{name}` · verbs: {}{hint}",
        catalogue::names()
    ))
}

#[derive(Default)]
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
                    "`-` means stdin, which at-continue-thread does not read · pass a title or an id",
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

fn context(parsed: &Parsed) -> Result<Opts, Fail> {
    let db = parsed.value("--db").map(PathBuf::from);

    let (budget_bytes, budget_clamped_from) = match parsed.value("--budget-kb") {
        Some(raw) => {
            let kb = raw
                .parse::<usize>()
                .map_err(|_| Fail::usage(format!("--budget-kb {raw} is not a number")))?;
            let bytes = kb.saturating_mul(1024);
            if bytes > MAX_BUDGET_BYTES {
                (MAX_BUDGET_BYTES, Some(kb))
            } else {
                (bytes, None)
            }
        }
        None => (DEFAULT_BUDGET_BYTES, None),
    };

    let (limit, limit_clamped_from) = match parsed.value("--limit") {
        Some(raw) => {
            let limit = raw
                .parse::<usize>()
                .map_err(|_| Fail::usage(format!("--limit {raw} is not a number")))?;
            if limit > MAX_ROWS {
                (MAX_ROWS, Some(limit))
            } else {
                (limit, None)
            }
        }
        None => (DEFAULT_ROWS, None),
    };

    Ok(Opts {
        db,
        budget_bytes,
        budget_clamped_from,
        limit,
        limit_clamped_from,
    })
}
