//! Argument parsing — a closed grammar of a noun and an aspect.
//!
//! The flags a verb accepts come from its [`crate::catalogue`] entry, so a flag that is not in
//! the table is a usage error naming it, and the aspects a noun accepts come from the same
//! entry. There is no `--` pass-through, no arbitrary URL, and no subprocess; an approval over
//! this grammar is a complete description of what the tool can be asked to do.

use crate::catalogue::{self, Noun, Route, Verb};
use crate::contract::{Exit, Fail};
use crate::koios::Network;
use crate::verbs::{self, targets, Options};

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
        "help" => return help(&args[1..]),
        "--version" => {
            println!("{} {}", crate::TOOL, crate::VERSION);
            return Ok(Exit::Results);
        }
        _ => {}
    }

    let noun = catalogue::find_noun(first).ok_or_else(|| unknown_noun(first))?;

    // `at-cardano pool --help` asks about the noun, not its principal aspect. A noun with one
    // aspect has no aspects to list, so there the verb's flags are the useful answer.
    if args.len() == 2 && matches!(args[1].as_str(), "-h" | "--help") {
        match (noun.verbs.len() == 1)
            .then_some(noun.default_verb())
            .flatten()
        {
            Some(verb) => print!("{}", catalogue::help_for(verb)),
            None => print!("{}", catalogue::help_noun(noun)),
        }
        return Ok(Exit::Results);
    }

    let (verb, consumed) = resolve(noun, &args[1..])?;
    let parsed = parse(verb, &args[1 + consumed..])?;

    if parsed.has("--help") {
        print!("{}", catalogue::help_for(verb));
        return Ok(Exit::Results);
    }

    if let Err(mut fail) = validate(verb, &parsed.positionals) {
        if noun.verbs.len() > 1 {
            fail.message.push_str(&format!(
                " · aspects of `{}`: {} · run `{} help {}`",
                noun.name,
                noun.aspects().join(", "),
                crate::TOOL,
                noun.name
            ));
        }
        return Err(fail);
    }
    let options = options(&parsed)?;

    let outcome = verbs::run(verb, &parsed.positionals, &options)?;
    let rendered = outcome.report.render();
    if !rendered.is_empty() {
        print!("{rendered}");
    }
    Ok(outcome.exit)
}

/// `at-cardano help`, `at-cardano help pool`, `at-cardano help pool delegators`.
fn help(args: &[String]) -> Result<Exit, Fail> {
    let Some(name) = args.first() else {
        print!("{}", catalogue::help_all());
        return Ok(Exit::Results);
    };
    if let Some(noun) = catalogue::find_noun(name) {
        match args.get(1) {
            Some(aspect) => {
                let verb = noun.find(aspect).ok_or_else(|| {
                    Fail::usage(format!(
                        "`{}` has no aspect `{aspect}` · aspects: {}",
                        noun.name,
                        noun.aspects().join(", ")
                    ))
                })?;
                print!("{}", catalogue::help_for(verb));
            }
            None => print!("{}", catalogue::help_noun(noun)),
        }
        return Ok(Exit::Results);
    }
    // `at-cardano help "pool delegators"` and the two-word spelling both work.
    let wanted = args.join(" ");
    let verb = catalogue::find(&wanted).ok_or_else(|| unknown_noun(name))?;
    print!("{}", catalogue::help_for(verb));
    Ok(Exit::Results)
}

/// The noun's aspect if one was typed, otherwise its principal question.
fn resolve(noun: &'static Noun, rest: &[String]) -> Result<(&'static Verb, usize), Fail> {
    if let Some(candidate) = rest.first() {
        if !candidate.starts_with('-') {
            if let Some(verb) = noun.find(candidate) {
                return Ok((verb, 1));
            }
        }
    }
    if let Some(verb) = noun.default_verb() {
        return Ok((verb, 0));
    }
    Err(Fail::usage(format!(
        "`{}` needs an aspect · {} · run `{} help {}`",
        noun.name,
        noun.aspects().join(", "),
        crate::TOOL,
        noun.name
    )))
}

fn unknown_noun(name: &str) -> Fail {
    let hint = if name.starts_with('-') {
        " · flags come after the noun, e.g. `at-cardano tip --network preprod`"
    } else {
        ""
    };
    Fail::usage(format!(
        "unknown noun `{name}` · nouns: {}{hint}",
        catalogue::names()
    ))
}

fn validate(verb: &'static Verb, targets: &[String]) -> Result<(), Fail> {
    match &verb.route {
        Route::Call(call) => targets::validate(call, targets),
        Route::Auth => {
            if let Some(extra) = targets.first() {
                return Err(Fail::usage(format!(
                    "`auth` takes no target · got `{extra}` · {}",
                    verb.usage
                )));
            }
            Ok(())
        }
        Route::Endpoints => {
            if targets.len() > 1 {
                return Err(Fail::usage(format!(
                    "`endpoints` takes one filter word · {}",
                    verb.usage
                )));
            }
            Ok(())
        }
        Route::Query => {
            if targets.len() != 1 {
                return Err(Fail::usage(format!(
                    "`query` needs one endpoint · {}",
                    verb.usage
                )));
            }
            Ok(())
        }
        Route::Scenario => {
            if let Some(extra) = targets.first() {
                return Err(Fail::usage(format!(
                    "`run` takes no target · name the scenario with --scenario · got `{extra}`"
                )));
            }
            Ok(())
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
                    "`-` means stdin, which at-cardano does not read · pass a target",
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

    let epoch = match parsed.value("--epoch") {
        Some(raw) => Some(
            raw.parse::<u64>()
                .map_err(|_| Fail::usage(format!("--epoch {raw} is not a number")))?,
        ),
        None => None,
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
        network_explicit: parsed.value("--network").is_some(),
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
        epoch,
        history: parsed.has("--history"),
        post,
        body,
        body_file,
        query,
        scenario: parsed.value("--scenario").map(str::to_string),
        check: parsed.has("--check"),
    })
}

pub(crate) fn valid_order(raw: &str) -> Result<&str, Fail> {
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

pub(crate) fn valid_select(raw: &str) -> Result<&str, Fail> {
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

pub(crate) fn parse_query(raw: &str) -> Result<(String, String), Fail> {
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
