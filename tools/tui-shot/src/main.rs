//! `tui-shot` — drive a terminal UI in a PTY this tool owns, then write the screen and a report.
//!
//! The whole grammar is a binary target name, a size, keys, waits and output paths. There is no
//! command string, no shell, no attach, and no session that outlives the invocation.

mod bin;
mod keys;
mod pty;
mod report;
mod runner;
mod scenario;
mod screen;
mod signals;

use std::path::PathBuf;

use anyhow::{anyhow, bail, Result};

use runner::{CheckOptions, RunOptions, SingleAction, SingleShot};
use scenario::{
    parse_duration_value, parse_size_value, Profile, DEFAULT_SETTLE, DEFAULT_TERM, DEFAULT_TIMEOUT,
};

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match dispatch(args) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("tui-shot: {error:#}");
            std::process::exit(1);
        }
    }
}

fn dispatch(args: Vec<String>) -> Result<i32> {
    if args
        .iter()
        .take_while(|argument| argument.as_str() != "--")
        .any(|argument| argument == "-h" || argument == "--help")
    {
        print_help();
        return Ok(0);
    }

    match args.first().map(String::as_str) {
        None => {
            print_help();
            Ok(0)
        }
        Some("run") => runner::run(parse_run(&args[1..])?),
        Some("check") => runner::check(parse_check(&args[1..])?),
        Some(other) if other.starts_with('-') => bail!("unknown option {other}; try --help"),
        Some(_) => runner::single_shot(parse_single(&args)?),
    }
}

fn parse_run(args: &[String]) -> Result<RunOptions> {
    let mut path = None;
    let mut artifacts = None;
    let mut json = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--scenario" => path = Some(PathBuf::from(value(args, &mut index, "--scenario")?)),
            "--artifacts" => {
                artifacts = Some(PathBuf::from(value(args, &mut index, "--artifacts")?));
            }
            "--json" => json = true,
            other => bail!("run: unknown option {other}"),
        }
        index += 1;
    }
    Ok(RunOptions {
        scenario: path.ok_or_else(|| anyhow!("run needs --scenario <file.dsl>"))?,
        artifacts,
        json,
    })
}

fn parse_check(args: &[String]) -> Result<CheckOptions> {
    let mut path = None;
    let mut json = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--scenario" => path = Some(PathBuf::from(value(args, &mut index, "--scenario")?)),
            "--json" => json = true,
            other => bail!("check: unknown option {other}"),
        }
        index += 1;
    }
    Ok(CheckOptions {
        scenario: path.ok_or_else(|| anyhow!("check needs --scenario <file.dsl>"))?,
        json,
    })
}

fn parse_single(args: &[String]) -> Result<SingleShot> {
    let split = args.iter().position(|argument| argument == "--");
    let (own, child): (&[String], Vec<String>) = match split {
        Some(index) => (&args[..index], args[index + 1..].to_vec()),
        None => (args, Vec::new()),
    };

    let mut iterator = own.iter();
    let bin = iterator
        .next()
        .ok_or_else(|| anyhow!("missing <bin>; see --help"))?
        .clone();
    let size = iterator
        .next()
        .ok_or_else(|| anyhow!("missing <cols>x<rows>; see --help"))?;
    let out = iterator
        .next()
        .ok_or_else(|| anyhow!("missing <out>; see --help"))?;

    let mut options = SingleShot {
        bin,
        size: parse_size_value(size)?,
        out: PathBuf::from(out),
        profile: Profile::Debug,
        term: DEFAULT_TERM.to_owned(),
        actions: Vec::new(),
        args: child,
        timeout: DEFAULT_TIMEOUT,
        settle: DEFAULT_SETTLE,
        json: false,
    };

    while let Some(argument) = iterator.next() {
        match argument.as_str() {
            "--profile" => {
                options.profile = match iterator
                    .next()
                    .ok_or_else(|| anyhow!("--profile needs debug or release"))?
                    .as_str()
                {
                    "debug" => Profile::Debug,
                    "release" => Profile::Release,
                    other => bail!("profile {other:?} is not `debug` or `release`"),
                };
            }
            "--term" => {
                options.term = iterator
                    .next()
                    .ok_or_else(|| anyhow!("--term needs a value"))?
                    .clone();
            }
            "--wait-for" => options.actions.push(SingleAction::Wait(
                iterator
                    .next()
                    .ok_or_else(|| anyhow!("--wait-for needs text"))?
                    .clone(),
            )),
            "--key" => options.actions.push(SingleAction::Key(
                iterator
                    .next()
                    .ok_or_else(|| anyhow!("--key needs a key name"))?
                    .clone(),
            )),
            "--type" => options.actions.push(SingleAction::Type(
                iterator
                    .next()
                    .ok_or_else(|| anyhow!("--type needs text"))?
                    .clone(),
            )),
            "--timeout" => {
                options.timeout = parse_duration_value(
                    iterator
                        .next()
                        .ok_or_else(|| anyhow!("--timeout needs a duration"))?,
                )?;
            }
            "--settle" => {
                options.settle = parse_duration_value(
                    iterator
                        .next()
                        .ok_or_else(|| anyhow!("--settle needs a duration"))?,
                )?;
            }
            "--json" => options.json = true,
            other => bail!("unknown option {other}"),
        }
    }

    Ok(options)
}

fn value<'a>(args: &'a [String], index: &mut usize, option: &str) -> Result<&'a str> {
    *index += 1;
    args.get(*index)
        .map(String::as_str)
        .ok_or_else(|| anyhow!("{option} needs a value"))
}

fn print_help() {
    println!(
        "\
tui-shot — drive a terminal UI in a PTY this tool owns

  tui-shot <bin> <cols>x<rows> <out> [options] [-- <args>...]
  tui-shot run --scenario <file.dsl> [--artifacts <dir>] [--json]
  tui-shot check --scenario <file.dsl> [--json]

It runs a binary target built in this workspace, never a path and never a shell command.
Every capture writes <out>.txt (the screen) and <out>.json (the report).

Single-shot options:
  --profile debug|release   build profile to run (default debug)
  --term NAME               TERM for the child (default {DEFAULT_TERM})
  --wait-for TEXT           wait until the screen contains TEXT
  --key NAME                send a key: enter, esc, up, ctrl-c, q, …
  --type TEXT               send literal text
  --timeout DURATION        wait bound (default 10s; ms/s suffixes)
  --settle DURATION         screen-quiet window (default 40ms)
  --json                    print the run report as JSON
  --                        everything after this goes to the child's argv

Scenario example:

  run artifacts=.tmp/tui
  tui app bin=factotum-tui size=120x42 args=\"--session s-1 --token demo\"
  wait app contains=\"factotum\"
  capture app out=01-arrival
  key app i
  type app \"second question\"
  key app enter
  wait app contains=\"second question\"
  capture app out=02-sent

Exit codes: 0 pass, 2 ran but a wait or assertion failed, 1 operational error."
    );
}
