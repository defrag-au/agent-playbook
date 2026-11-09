//! Run the playbook's harness comparison over real-work scenarios and score it.
//!
//! `integrations/rig/tests/redirect.rs` proves the plugin mechanics with a scripted model. This
//! binary answers the next question: over a task that needs several reads, how do rig (with and
//! without the playbook plugin), opencode and Pi compare? Each run is the harness's own headless
//! mode in a scenario's working directory, with the same model and the same sealed corpus. The
//! raw stream is kept verbatim beside the score, so every number in the summary can be re-derived
//! from a file.
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
//! It is a separate package on purpose: a runner inside `integrations/rig` is part of the thing
//! the scenarios investigate, and a model asked about the plugin reads it.
//!
//! The rig homes default to `.tmp/rig-home` (gated) and `.tmp/rig-home-baseline` (tools only);
//! `--target rig:vanilla` is the no-plugin control. The model key is read from
//! `.tmp/rig-home/model.env` and never printed.

mod cli;
mod driver;
mod process;
mod report;
mod sandbox;
mod scenario;
mod score;
mod stream;
mod tag;
mod target;

use std::process::ExitCode;

use anyhow::{bail, Context, Result};

use crate::cli::Args;
use crate::driver::RunContext;

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
        report::print_plan(&args, &scenarios);
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
        for target in &args.targets {
            for repeat in 1..=args.repeat {
                println!(
                    "· {} · {} · run {repeat}/{}",
                    scenario.name,
                    target.label(),
                    args.repeat
                );
                let result = target::run_one(target, scenario, &workdir, repeat, &context)?;
                report::print_run(&result);
                results.push(result);
            }
        }
    }

    report::write(&args, &results)?;
    Ok(())
}
