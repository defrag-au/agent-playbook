//! Argument parsing — a closed grammar.
//!
//! The set of flags a verb accepts is read from [`crate::catalogue`] rather than written out here,
//! so the parser and the catalogue cannot drift: a flag that is not in the table is a usage error
//! naming it, and nothing is forwarded anywhere else. There is no `--` pass-through and no
//! environment variable, so an approval over this grammar is a complete description of what the
//! tool can be asked to do.
//!
//! The `--` the verbs use to separate paths is built by the tool from parsed positionals; it is not
//! something a caller can reach through. That matters more here than in `at-peek`, because the
//! second half of this grammar is a git argv.
//!
//! The parse loop is the same one `at-peek` runs. It is written twice rather than shared so far,
//! because sharing it means either a trait or a macro over two different verb types, and neither
//! earns its keep yet — the duplication is one function and one struct, and the alternative is a
//! third abstraction for two callers. Moving it into `at-core` is the obvious tidy when a third
//! tool needs it.

use std::path::Path;

use at_core::contract::{Exit, Fail, DEFAULT_LIMIT, MAX_LIMIT};
use at_core::paths::Root;

use crate::catalogue::{self, Verb};
use crate::git::{Git, Noun};
use crate::verbs::{self, Opts};

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
    // A verb in the catalogue with no arm here fails loudly rather than being advertised and
    // missing — the catalogue is the interface, so the two have to be wired together.
    let outcome = match verb.name {
        "state" => {
            if !parsed.positionals.is_empty() {
                return Err(Fail::usage(format!(
                    "`state` answers for the whole worktree and takes no path · {}",
                    verb.usage
                )));
            }
            verbs::state::run(&opts)?
        }
        "diff" => verbs::diff::run(&parsed.positionals, &opts)?,
        "log" => verbs::log::run(&parsed.positionals, &opts)?,
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
    // A flag before the verb is the likeliest mistake a caller makes, and the parser's answer to it
    // should say where flags go rather than only that the verb is unknown.
    let hint = if name.starts_with('-') {
        " · flags come after the verb, e.g. `at-recall diff --patch`"
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
                    "`-` means stdin, which at-recall does not read · pass a path",
                ));
            }
            let (name, inline) = match arg.split_once('=') {
                Some((name, value)) => (name.to_string(), Some(value.to_string())),
                None => (arg.to_string(), None),
            };
            // `-h` is an alias rather than a second table entry, so help is listed once.
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

/// The root and the one subprocess, resolved once per invocation.
fn context(parsed: &Parsed) -> Result<Opts, Fail> {
    let named_root = parsed.value("--root").map(str::to_string);
    let guess = match &named_root {
        Some(path) => Root::at(Path::new(path))?,
        None => {
            let cwd = std::env::current_dir().map_err(|e| {
                Fail::environment(format!("cannot read the working directory: {e}"))
            })?;
            Root::discover(&cwd)?
        }
    };

    // Ask git where the top of the worktree is rather than trusting the guess: `--root` may name a
    // subdirectory, and every path git prints is relative to the top. This is also the check that
    // the tree is a worktree at all, so a repository-less directory fails with one clear sentence
    // instead of four of git's.
    let probe = Git::at(guess.dir());
    let toplevel = probe
        .probe(Noun::RevParse, &["--show-toplevel"])
        .ok_or_else(|| {
            Fail::environment(format!(
                "{} is not a git worktree · at-recall reads history, at-peek reads the working tree",
                guess.dir().display()
            ))
        })?;
    let root = Root::at(Path::new(toplevel.trim()))?;
    let git = Git::at(root.dir());

    let (limit, limit_clamped_from) = match parsed.value("--limit") {
        Some(raw) => {
            let asked = raw
                .parse::<usize>()
                .map_err(|_| Fail::usage(format!("--limit {raw} is not a number")))?;
            if asked == 0 {
                return Err(Fail::usage(
                    "--limit 0 asks for no output · the smallest useful limit is 1",
                ));
            }
            if asked > MAX_LIMIT {
                (MAX_LIMIT, Some(asked))
            } else {
                (asked, None)
            }
        }
        None => (DEFAULT_LIMIT, None),
    };

    Ok(Opts {
        root,
        root_was_explicit: named_root.is_some(),
        git,
        limit,
        limit_clamped_from,
        include_secret_paths: parsed.has("--include-secret-paths"),
        patch: parsed.has("--patch"),
        summary: parsed.has("--summary"),
        ignore_space: parsed.has("--ignore-space"),
    })
}
