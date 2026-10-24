//! Argument parsing — a closed grammar.
//!
//! The flags a verb accepts come from [`crate::catalogue`], so the parser and the catalogue cannot
//! drift. There is no `--` pass-through, no environment variable that changes a flag, and no
//! forwarding anywhere else: an approval over this grammar is a complete description of what the
//! tool can be asked to do. The three environment reads the tool does make — `CARGO_HOME`,
//! `CARGO_TARGET_DIR` and the home directory — name locations to read, never a write or a
//! command.

use std::fs;
use std::path::{Path, PathBuf};

use at_core::contract::{Exit, Fail, DEFAULT_LIMIT, DEFAULT_MAX_FILES, MAX_FILES, MAX_LIMIT};

use crate::catalogue::{self, Verb};
use crate::verbs::{self, Options};

const DEFAULT_DEPTH: usize = 2;
const MAX_DEPTH: usize = 6;

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

    let opts = options(&parsed)?;
    // A verb in the catalogue with no arm here fails loudly rather than being advertised and
    // missing — the catalogue is the interface, so the two have to be wired together.
    let outcome = match verb.name {
        "where" => verbs::location::run(&parsed.positionals, &opts)?,
        "installed" => verbs::installed::run(&parsed.positionals, &opts)?,
        "files" => verbs::files::run(&parsed.positionals, &opts)?,
        "slice" => verbs::slice::run(&parsed.positionals, &opts)?,
        "search" => verbs::search::run(&parsed.positionals, &opts)?,
        "out" => verbs::out::run(&parsed.positionals, &opts)?,
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
        " · flags come after the verb, e.g. `at-crate search <pattern> <crate>`"
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
                    "`-` means stdin, which at-crate does not read · pass a crate name",
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

fn options(parsed: &Parsed) -> Result<Options, Fail> {
    let (limit, limit_clamped_from) = number(parsed, "--limit", DEFAULT_LIMIT, MAX_LIMIT)?;
    let (max_files, max_files_clamped_from) =
        number(parsed, "--max-files", DEFAULT_MAX_FILES, MAX_FILES)?;
    let (depth, depth_clamped_from) = number(parsed, "--depth", DEFAULT_DEPTH, MAX_DEPTH)?;

    if parsed.has("--count") && parsed.has("--files-only") {
        return Err(Fail::usage(
            "--count and --files-only ask different questions · pass one",
        ));
    }

    let build = parsed.has("--build");
    if parsed.value("--build-dir").is_some() && !build {
        return Err(Fail::usage(
            "--build-dir only applies with --build · it chooses among build-script outputs",
        ));
    }

    let cwd = std::env::current_dir().map_err(|error| {
        Fail::environment(format!("cannot read the working directory: {error}"))
    })?;

    Ok(Options {
        limit,
        limit_clamped_from,
        max_files,
        max_files_clamped_from,
        depth,
        depth_clamped_from,
        include_secret_paths: parsed.has("--include-secret-paths"),
        summary: parsed.has("--summary"),
        count: parsed.has("--count"),
        files_only: parsed.has("--files-only"),
        version: parsed.value("--version").map(str::to_string),
        registry: parsed.value("--registry").map(str::to_string),
        build,
        build_dir: dir_value(parsed, "--build-dir")?,
        cargo_home: dir_value(parsed, "--cargo-home")?,
        project: dir_value(parsed, "--project")?,
        target_dir: dir_value(parsed, "--target-dir")?,
        cwd,
    })
}

fn number(
    parsed: &Parsed,
    flag: &str,
    default: usize,
    ceiling: usize,
) -> Result<(usize, Option<usize>), Fail> {
    let Some(raw) = parsed.value(flag) else {
        return Ok((default, None));
    };
    let asked = raw
        .parse::<usize>()
        .map_err(|_| Fail::usage(format!("{flag} {raw} is not a number")))?;
    if asked == 0 {
        return Err(Fail::usage(format!(
            "{flag} 0 asks for no output · the smallest useful value is 1"
        )));
    }
    if asked > ceiling {
        Ok((ceiling, Some(asked)))
    } else {
        Ok((asked, None))
    }
}

fn dir_value(parsed: &Parsed, flag: &str) -> Result<Option<PathBuf>, Fail> {
    let Some(raw) = parsed.value(flag) else {
        return Ok(None);
    };
    let path = Path::new(raw);
    let resolved = fs::canonicalize(path)
        .map_err(|error| Fail::environment(format!("cannot read {flag} {raw}: {error}")))?;
    if !resolved.is_dir() {
        return Err(Fail::usage(format!("{flag} {raw} is not a directory")));
    }
    Ok(Some(resolved))
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
            verb("search"),
            &[
                "protocol".to_string(),
                "headless_chrome".to_string(),
                "--limit".to_string(),
                "20".to_string(),
            ],
        )
        .expect("a parse");
        assert_eq!(parsed.positionals, ["protocol", "headless_chrome"]);
        assert_eq!(parsed.value("--limit"), Some("20"));
    }

    #[test]
    fn an_unknown_flag_is_named() {
        let error = parse(verb("search"), &["--wat".to_string()]).expect_err("unknown flag");
        assert!(error.message.contains("unknown flag `--wat`"));
    }

    #[test]
    fn a_flag_that_takes_no_value_refuses_one() {
        let error = parse(verb("search"), &["--count=yes".to_string()]).expect_err("no value");
        assert!(error.message.contains("takes no value"));
    }

    #[test]
    fn count_and_files_only_together_is_a_usage_error() {
        let parsed = parse(
            verb("search"),
            &["--count".to_string(), "--files-only".to_string()],
        )
        .expect("a parse");
        let error = options(&parsed).expect_err("both modes");
        assert!(error.message.contains("--count and --files-only"));
    }
}
