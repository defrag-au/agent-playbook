//! The rig arm: `rig -p --json`, parsed by the stream module.

use std::fs;
use std::path::Path;

use anyhow::Result;

use crate::cli::Home;
use crate::driver::RunContext;
use crate::process::{self, Invocation};
use crate::report::RunResult;
use crate::scenario::Scenario;
use crate::score::{self, Citation};
use crate::stream;
use crate::tag::RunTag;

pub fn run(
    scenario: &Scenario,
    workdir: &Path,
    home: &Home,
    repeat: u32,
    context: &RunContext<'_>,
) -> Result<RunResult> {
    let stem = format!("{}-{}-r{repeat}", scenario.name, context.arm);
    let mut env = vec![("RIG_HOME".to_owned(), home.path.display().to_string())];
    env.extend(context.env.iter().cloned());
    let invocation = Invocation {
        program: context.args.rig.clone(),
        args: vec![
            "-p".to_owned(),
            "--json".to_owned(),
            scenario.prompt.clone(),
            "--model".to_owned(),
            context.args.model.clone(),
        ],
        env,
        cwd: workdir.to_path_buf(),
    };

    let completed = process::run(
        invocation,
        &context.args.out,
        &stem,
        context.args.timeout,
        context.sandbox.as_deref(),
    )?;
    let mut facts = stream::parse(&completed.stdout)?;
    if completed.timed_out {
        facts.tags.insert(RunTag::TimedOut);
    }
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
        target: format!("rig:{}", home.label),
        repeat,
        model: context.args.model.clone(),
        workdir: scenario.workdir.clone(),
        wall_ms: completed.wall.as_millis().try_into().unwrap_or(u64::MAX),
        exit_code: facts.exit_code.or(completed.status.code()),
        exit_error: facts.exit_error,
        session: facts.session,
        effects,
        model_calls: facts.model_calls,
        compactions: facts.compactions,
        input_tokens: facts.input_tokens,
        output_tokens: facts.output_tokens,
        cached_input_tokens: facts.cached_input_tokens,
        cost_usd: facts.cost,
        tool_calls: facts.tool_calls,
        tool_call_total: facts.tool_call_total,
        tool_results: facts.tool_results,
        refused: facts.refused,
        failed: facts.failed,
        cancelled: facts.cancelled,
        output_bytes: facts.output_bytes,
        used_plugin_tools: facts.used_plugin_tools,
        used_toolkit_cli: facts.used_toolkit_cli,
        toolkit_cli_calls: facts.toolkit_cli_calls,
        loaded_skill: facts.loaded_skill,
        unparsed_lines: facts.unparsed,
        toolkit: None,
        tags: facts.tags,
        answer,
        citations: scored.cited.iter().map(Citation::render).collect(),
        checks: scored.checks,
        passed: scored.passed,
        checks_total: scored.total,
        score: scored.score,
        precision: scored.precision,
    })
}
