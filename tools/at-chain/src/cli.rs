//! Argument parsing — a closed grammar.
//!
//! The flags a verb accepts come from [`crate::catalogue`], so a flag that is not in the table
//! is a usage error naming it. There is no `--` pass-through, no arbitrary URL, and no
//! subprocess; an approval over this grammar is a complete description of what the tool can be
//! asked to do.

use crate::catalogue::{self, Verb};
use crate::contract::{Exit, Fail};
use crate::koios::Network;
use crate::verbs::{self, Options, MAX_BATCH};

pub const DEFAULT_ROWS: usize = 20;
pub const MAX_ROWS: usize = 1000;

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

    let targets = targets(verb, &parsed)?;
    verbs::validate_targets(verb.name, &targets)?;
    let options = options(&parsed)?;

    let outcome = verbs::run(verb.name, &targets, &options)?;
    let rendered = outcome.report.render();
    if !rendered.is_empty() {
        print!("{rendered}");
    }
    Ok(outcome.exit)
}

fn unknown(name: &str) -> Fail {
    let hint = if name.starts_with('-') {
        " · flags come after the verb, e.g. `at-chain tip --network preprod`"
    } else {
        ""
    };
    Fail::usage(format!(
        "unknown verb `{name}` · verbs: {}{hint}",
        catalogue::names()
    ))
}

fn targets(verb: &Verb, parsed: &Parsed) -> Result<Vec<String>, Fail> {
    match verb.name {
        "tip" | "auth" => {
            if let Some(extra) = parsed.positionals.first() {
                return Err(Fail::usage(format!(
                    "`{}` takes no target · got `{extra}` · {}",
                    verb.name, verb.usage
                )));
            }
            Ok(Vec::new())
        }
        "query" => match parsed.positionals.as_slice() {
            [only] => Ok(vec![only.clone()]),
            [] => Err(Fail::usage(format!(
                "`{}` needs an endpoint · {}",
                verb.name, verb.usage
            ))),
            _ => Err(Fail::usage(format!(
                "`{}` takes one endpoint · {}",
                verb.name, verb.usage
            ))),
        },
        _ => {
            if parsed.positionals.is_empty() {
                return Err(Fail::usage(format!(
                    "`{}` needs at least one target · {}",
                    verb.name, verb.usage
                )));
            }
            if parsed.positionals.len() > MAX_BATCH {
                return Err(Fail::usage(format!(
                    "`{}` takes at most {MAX_BATCH} targets · {}",
                    verb.name, verb.usage
                )));
            }
            Ok(parsed.positionals.clone())
        }
    }
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

    fn values(&self, name: &str) -> Vec<&str> {
        self.flags
            .iter()
            .filter(|(flag, _)| *flag == name)
            .filter_map(|(_, value)| value.as_deref())
            .collect()
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
                    "`-` means stdin, which at-chain does not read · pass a target",
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

fn options(parsed: &Parsed) -> Result<Options, Fail> {
    let network = match parsed.value("--network") {
        Some(raw) => Network::parse(raw).ok_or_else(|| {
            Fail::usage(format!(
                "--network {raw} is not a Koios network · mainnet, preprod, preview or guild"
            ))
        })?,
        None => Network::Mainnet,
    };

    let (limit, limit_clamped_from) = match parsed.value("--limit") {
        Some(raw) => {
            let asked = raw
                .parse::<usize>()
                .map_err(|_| Fail::usage(format!("--limit {raw} is not a number")))?;
            if asked == 0 {
                return Err(Fail::usage(
                    "--limit 0 asks for no rows · the smallest useful limit is 1",
                ));
            }
            if asked > MAX_ROWS {
                (MAX_ROWS, Some(asked))
            } else {
                (asked, None)
            }
        }
        None => (DEFAULT_ROWS, None),
    };

    let offset = match parsed.value("--offset") {
        Some(raw) => raw
            .parse::<usize>()
            .map_err(|_| Fail::usage(format!("--offset {raw} is not a number")))?,
        None => 0,
    };

    let all = parsed.has("--all");
    let json = parsed.has("--json");
    let summary = parsed.has("--summary");
    if all && json {
        return Err(Fail::usage(
            "--all and --json are alternatives · JSON is one page; page it with --offset",
        ));
    }
    if all && parsed.has("--limit") {
        return Err(Fail::usage(
            "--all and --limit are alternatives · --all asks for the 1000-row ceiling",
        ));
    }
    if summary && json {
        return Err(Fail::usage(
            "--summary and --json are alternatives · one prints the frame, the other the body",
        ));
    }
    if parsed.has("--mints") && parsed.has("--addresses") {
        return Err(Fail::usage(
            "--mints and --addresses are alternatives · one policy view at a time",
        ));
    }

    let order = parsed
        .value("--order")
        .map(valid_order)
        .transpose()?
        .map(str::to_string);
    let select = parsed
        .value("--select")
        .map(valid_select)
        .transpose()?
        .map(str::to_string);

    let body = parsed.value("--body").map(str::to_string);
    let body_file = parsed.value("--body-file").map(str::to_string);
    if body.is_some() && body_file.is_some() {
        return Err(Fail::usage(
            "--body and --body-file are alternatives · supply one request body",
        ));
    }
    let post = parsed.has("--post");
    if !post && (body.is_some() || body_file.is_some()) {
        return Err(Fail::usage(
            "a body is only sent with `query --post` · add --post or drop the body",
        ));
    }

    let mut query = Vec::new();
    for raw in parsed.values("--query") {
        query.push(parse_query(raw)?);
    }

    Ok(Options {
        network,
        json,
        keyless: parsed.has("--keyless"),
        summary,
        all,
        limit,
        limit_clamped_from,
        limit_explicit: parsed.has("--limit"),
        offset,
        offset_explicit: parsed.has("--offset"),
        order,
        select,
        extended: !parsed.has("--no-extended"),
        inputs: !parsed.has("--no-inputs"),
        assets: !parsed.has("--no-assets"),
        scripts: parsed.has("--scripts"),
        mints: parsed.has("--mints"),
        addresses: parsed.has("--addresses"),
        post,
        body,
        body_file,
        query,
    })
}

fn valid_order(raw: &str) -> Result<&str, Fail> {
    if raw.is_empty() {
        return Err(Fail::usage("--order is empty"));
    }
    for part in raw.split(',') {
        let (column, direction) = match part.split_once('.') {
            Some((column, direction)) => (column, Some(direction)),
            None => (part, None),
        };
        if !valid_column(column) {
            return Err(Fail::usage(format!(
                "--order `{raw}` · `{}` is not a column name",
                column
            )));
        }
        if let Some(direction) = direction {
            if direction != "asc" && direction != "desc" {
                return Err(Fail::usage(format!(
                    "--order `{raw}` · `{direction}` is not `asc` or `desc`"
                )));
            }
        }
    }
    Ok(raw)
}

fn valid_select(raw: &str) -> Result<&str, Fail> {
    if raw.is_empty() {
        return Err(Fail::usage("--select is empty"));
    }
    for part in raw.split(',') {
        if part != "*" && !valid_column(part) {
            return Err(Fail::usage(format!(
                "--select `{raw}` · `{part}` is not a column name"
            )));
        }
    }
    Ok(raw)
}

fn valid_column(raw: &str) -> bool {
    let mut characters = raw.chars();
    matches!(characters.next(), Some(first) if first.is_ascii_lowercase() || first == '_')
        && characters.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
}

fn parse_query(raw: &str) -> Result<(String, String), Fail> {
    let (key, value) = raw
        .split_once('=')
        .ok_or_else(|| Fail::usage(format!("--query `{raw}` is not `key=value`")))?;
    if !valid_column(key) {
        return Err(Fail::usage(format!(
            "--query `{raw}` · `{key}` is not a parameter name"
        )));
    }
    Ok((key.to_string(), value.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_accepts_columns_and_directions() {
        assert!(valid_order("block_time.desc").is_ok());
        assert!(valid_order("block_time.asc,block_height.desc").is_ok());
        assert!(valid_order("block_time").is_ok());
        assert!(valid_order("block_time.up").is_err());
        assert!(valid_order("block-time.desc").is_err());
    }

    #[test]
    fn select_accepts_columns_and_star() {
        assert!(valid_select("tx_hash,block_height").is_ok());
        assert!(valid_select("*").is_ok());
        assert!(valid_select("tx_hash,").is_err());
        assert!(valid_select("tx-hash").is_err());
    }

    #[test]
    fn query_parameters_are_key_equals_value() {
        assert_eq!(
            parse_query("_asset_policy=abc").expect("valid"),
            ("_asset_policy".to_string(), "abc".to_string())
        );
        assert!(parse_query("_asset_policy").is_err());
        assert!(parse_query("bad-key=value").is_err());
    }
}
