//! Starting rig, sealing the eval data, and turning one run into a result.

use std::fs::{self, File};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::cli::{Args, Home};
use crate::report::RunResult;
use crate::scenario::Scenario;
use crate::score::{self, Citation};
use crate::stream;

const POLL: Duration = Duration::from_millis(50);

pub struct RunContext<'a> {
    pub args: &'a Args,
    pub env: &'a [(String, String)],
    pub sandbox: Option<String>,
}

pub fn run_one(
    scenario: &Scenario,
    workdir: &Path,
    home: &Home,
    repeat: u32,
    context: &RunContext<'_>,
) -> Result<RunResult> {
    let stem = format!("{}-{}-r{repeat}", scenario.name, home.label);
    let stream_path = context.args.out.join(format!("{stem}.jsonl"));
    let error_path = context.args.out.join(format!("{stem}.err"));

    let stdout = File::create(&stream_path)
        .with_context(|| format!("creating {}", stream_path.display()))?;
    let stderr =
        File::create(&error_path).with_context(|| format!("creating {}", error_path.display()))?;

    let mut command = match &context.sandbox {
        Some(profile) => {
            let mut command = Command::new("/usr/bin/sandbox-exec");
            command.arg("-p").arg(profile).arg(&context.args.rig);
            command
        }
        None => Command::new(&context.args.rig),
    };
    command
        .arg("-p")
        .arg("--json")
        .arg(&scenario.prompt)
        .arg("--model")
        .arg(&context.args.model)
        .current_dir(workdir)
        .env("RIG_HOME", &home.path)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    for (key, value) in context.env {
        command.env(key, value);
    }

    let started = Instant::now();
    let mut child = command
        .spawn()
        .with_context(|| format!("starting {}", context.args.rig.display()))?;
    let (status, timed_out) = wait(&mut child, context.args.timeout)?;
    let wall = started.elapsed();

    let facts = stream::parse(&stream_path)?;
    let answer = facts.answer.clone();
    let scored = score::score(scenario, answer.as_deref(), &facts.tool_output);

    let effects = facts.session.as_ref().and_then(|session| {
        let source = home
            .path
            .join("sessions")
            .join(session)
            .join("files")
            .join("effects.jsonl");
        let copy = context.args.out.join(format!("{stem}.effects.jsonl"));
        if source.is_file() && fs::copy(&source, &copy).is_ok() {
            Some(copy.display().to_string())
        } else {
            None
        }
    });

    Ok(RunResult {
        scenario: scenario.name.clone(),
        home: home.label.clone(),
        repeat,
        model: context.args.model.clone(),
        workdir: workdir.display().to_string(),
        wall_ms: wall.as_millis().try_into().unwrap_or(u64::MAX),
        timed_out,
        exit_code: facts.exit_code.or(status.code()),
        exit_error: facts.exit_error,
        session: facts.session,
        effects,
        model_calls: facts.model_calls,
        compactions: facts.compactions,
        input_tokens: facts.input_tokens,
        output_tokens: facts.output_tokens,
        cost_usd: facts.cost,
        tool_calls: facts.tool_calls,
        tool_call_total: facts.tool_call_total,
        tool_results: facts.tool_results,
        refused: facts.refused,
        failed: facts.failed,
        cancelled: facts.cancelled,
        output_bytes: facts.output_bytes,
        unparsed_lines: facts.unparsed,
        contaminated: facts.contaminated,
        answer,
        citations: scored.cited.iter().map(Citation::render).collect(),
        checks: scored.checks,
        passed: scored.passed,
        checks_total: scored.total,
        score: scored.score,
        precision: scored.precision,
    })
}

/// Wait for the child, killing it if the run passes its limit. One owner keeps the poll simple.
fn wait(child: &mut Child, limit: Option<Duration>) -> Result<(std::process::ExitStatus, bool)> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().context("waiting for rig")? {
            return Ok((status, false));
        }
        if limit.is_some_and(|limit| started.elapsed() >= limit) {
            let _ = child.kill();
            return Ok((child.wait().context("waiting for rig")?, true));
        }
        thread::sleep(POLL);
    }
}

pub fn print_plan(args: &Args, scenarios: &[Scenario]) {
    println!(
        "# rig-eval · {} scenario(s) · {} home(s) · {} run(s) each",
        scenarios.len(),
        args.homes.len(),
        args.repeat
    );
    println!("rig      {}", args.rig.display());
    println!("model    {}", args.model);
    match crate::cli::read_env(&args.env_file) {
        Ok(vars) => {
            let keys: Vec<&str> = vars.iter().map(|(key, _)| key.as_str()).collect();
            println!("env      {} ({})", args.env_file.display(), keys.join(", "));
        }
        Err(why) => println!("env      {why}"),
    }
    println!("out      {}", args.out.display());
    for home in &args.homes {
        let state = if home.path.join("settings.toml").is_file() {
            "settings.toml"
        } else {
            "MISSING settings.toml"
        };
        println!("{:8} {} ({state})", home.label, home.path.display());
    }
    println!(
        "sandbox  {}",
        if crate::sandbox::profile(args).is_some() {
            "the corpus and results are unreadable to the agent"
        } else {
            "NOT available: the agent can read the corpus and the results"
        }
    );
    println!();
    for scenario in scenarios {
        println!(
            "{:28} workdir {:<6} {:>2} check(s)  {}",
            scenario.name,
            scenario.workdir,
            scenario.checks(),
            scenario.why
        );
    }
}
