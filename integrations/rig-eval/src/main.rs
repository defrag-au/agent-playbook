//! Run the playbook's rig homes over real-work scenarios and score them.
//!
//! `tests/redirect.rs` proves the mechanics with a scripted model. This binary answers the next
//! question: over a task that needs several reads, does the harness cost less than the same tools
//! without enforcement? Each run is `rig -p --json "<prompt>"` in a scenario's working directory,
//! under a named `RIG_HOME`. The JSON stream is kept verbatim beside the score, so every number in
//! the summary can be re-derived from a file.
//!
//! The corpus ships at `integrations/rig-eval/scenarios`; `--scenarios` points elsewhere if you
//! keep a private copy. A corpus inside the checkout is only run where the macOS sandbox is
//! available, because the sandbox is what denies the agent the gold it is scored against. `--out`
//! is explicit: the streams and results are the artefact you keep. A scenario's gold is a set of
//! `path:line` citations, plus optional paths and literal strings the answer must contain, and
//! literal strings the tool output must contain. Every check is validated before a run, so stale
//! gold is an error rather than a model failure.
//!
//! ```sh
//! cargo run --manifest-path integrations/rig-eval/Cargo.toml -- --out /tmp/rig-eval --dry-run
//! cargo run --manifest-path integrations/rig-eval/Cargo.toml -- --out /tmp/rig-eval
//! ```
//!
//! It is a separate package on purpose: a runner inside `integrations/rig` is part of the thing the
//! scenarios investigate, and a model asked about the plugin reads it.
//!
//! The homes default to `.tmp/rig-home` (gated) and `.tmp/rig-home-baseline` (tools only). The
//! model key is read from `.tmp/rig-home/model.env` and never printed.

mod cli;
mod report;
mod runner;
mod sandbox;
mod scenario;
mod score;
mod stream;

use std::process::ExitCode;

use anyhow::{bail, Context, Result};

use crate::cli::Args;
use crate::runner::RunContext;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rig-eval: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let args = Args::parse()?;
    let scenarios = scenario::load(&args)?;
    if scenarios.is_empty() {
        bail!("no scenarios in {}", args.scenario_dir.display());
    }

    if args.dry_run {
        runner::print_plan(&args, &scenarios);
        return Ok(());
    }

    let vars = cli::read_env(&args.env_file)?;
    std::fs::create_dir_all(&args.out)
        .with_context(|| format!("creating {}", args.out.display()))?;

    let sandbox = sandbox::profile(&args);
    match &sandbox {
        Some(_) => println!(
            "# sandbox: reads of {} and {} are denied to the agent",
            args.scenario_dir.display(),
            args.out.display()
        ),
        None => println!("# sandbox: not available; the agent can read the corpus and the results"),
    }

    let context = RunContext {
        args: &args,
        env: &vars,
        sandbox,
    };

    let mut results = Vec::new();
    for scenario in &scenarios {
        let workdir = args.root.join(&scenario.workdir);
        for home in &args.homes {
            for repeat in 1..=args.repeat {
                println!(
                    "· {} · {} · run {repeat}/{}",
                    scenario.name, home.label, args.repeat
                );
                let result = runner::run_one(scenario, &workdir, home, repeat, &context)?;
                report::print_run(&result);
                results.push(result);
            }
        }
    }

    report::write(&args, &results)?;
    Ok(())
}
