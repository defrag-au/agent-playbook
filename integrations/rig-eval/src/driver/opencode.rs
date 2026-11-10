//! The opencode arm: `opencode run --format json`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::driver::RunContext;
use crate::process::{self, Invocation};
use crate::report::RunResult;
use crate::scenario::Scenario;
use crate::score::{self, Citation};
use crate::stream;
use crate::tag::RunTag;

pub const PACKAGE: &str = "opencode-ai@1.19.5";

pub fn run(
    scenario: &Scenario,
    workdir: &Path,
    repeat: u32,
    context: &RunContext<'_>,
) -> Result<RunResult> {
    let stem = format!("{}-opencode-r{repeat}", scenario.name);
    let mut env = vec![(
        "OPENCODE_CONFIG".to_owned(),
        context.args.opencode_config.display().to_string(),
    )];
    env.extend(context.env.iter().cloned());

    let invocation = Invocation {
        program: PathBuf::from("bunx"),
        args: vec![
            PACKAGE.to_owned(),
            "run".to_owned(),
            "--format".to_owned(),
            "json".to_owned(),
            "--auto".to_owned(),
            "-m".to_owned(),
            context.args.model.clone(),
            scenario.prompt.clone(),
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
    let mut facts = parse(&completed.stdout)?;
    if completed.timed_out {
        facts.tags.insert(RunTag::TimedOut);
    }
    let answer = if facts.answer.is_empty() {
        None
    } else {
        Some(facts.answer.clone())
    };
    let scored = score::score(scenario, answer.as_deref(), &facts.tool_output);
    let exit_error = if completed.status.success() {
        None
    } else {
        process::tail(&completed.stderr)
    };

    Ok(RunResult {
        scenario: scenario.name.clone(),
        target: "opencode".to_owned(),
        repeat,
        model: context.args.model.clone(),
        workdir: scenario.workdir.clone(),
        wall_ms: completed.wall.as_millis().try_into().unwrap_or(u64::MAX),
        exit_code: completed.status.code(),
        exit_error,
        session: facts.session,
        effects: None,
        model_calls: facts.model_calls,
        compactions: 0,
        input_tokens: facts.input_tokens,
        output_tokens: facts.output_tokens,
        cost_usd: facts.cost,
        tool_calls: facts.tool_calls,
        tool_call_total: facts.tool_call_total,
        tool_results: facts.tool_results,
        refused: 0,
        failed: facts.failed,
        cancelled: 0,
        output_bytes: facts.output_bytes,
        unparsed_lines: facts.unparsed,
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

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Event {
    #[serde(rename = "step_start")]
    StepStart {
        #[serde(rename = "sessionID", default)]
        session: Option<String>,
    },
    #[serde(rename = "text")]
    Text { part: TextPart },
    #[serde(rename = "tool_use")]
    ToolUse { part: ToolPart },
    #[serde(rename = "step_finish")]
    StepFinish { part: StepPart },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
struct TextPart {
    #[serde(default)]
    text: String,
}

#[derive(Debug, Deserialize)]
struct ToolPart {
    #[serde(default)]
    tool: String,
    #[serde(default)]
    state: ToolState,
}

#[derive(Debug, Default, Deserialize)]
struct ToolState {
    #[serde(default)]
    status: String,
    #[serde(default)]
    output: Option<String>,
    #[serde(default)]
    metadata: Option<Metadata>,
    /// Tool arguments are dynamic per tool; read for contamination only.
    #[serde(default)]
    input: Option<serde_json::Value>,
}

#[derive(Debug, Default, Deserialize)]
struct Metadata {
    #[serde(default)]
    output: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StepPart {
    #[serde(default)]
    tokens: Tokens,
    #[serde(default)]
    cost: f64,
}

#[derive(Debug, Default, Deserialize)]
struct Tokens {
    #[serde(default)]
    input: u64,
    #[serde(default)]
    output: u64,
}

#[derive(Default)]
struct Facts {
    answer: String,
    tool_calls: BTreeMap<String, usize>,
    tool_call_total: usize,
    tool_results: usize,
    output_bytes: usize,
    tool_output: String,
    model_calls: usize,
    input_tokens: u64,
    output_tokens: u64,
    cost: f64,
    failed: usize,
    session: Option<String>,
    unparsed: usize,
    tags: BTreeSet<RunTag>,
}

impl Facts {
    fn absorb(&mut self, event: Event) {
        match event {
            Event::StepStart { session } => {
                if session.is_some() {
                    self.session = session;
                }
            }
            Event::Text { part } => {
                if part.text.is_empty() {
                    return;
                }
                if !self.answer.is_empty() {
                    self.answer.push('\n');
                }
                self.answer.push_str(&part.text);
            }
            Event::ToolUse { part } => {
                *self.tool_calls.entry(part.tool.clone()).or_default() += 1;
                self.tool_call_total += 1;
                self.tool_results += 1;
                if let Some(input) = &part.state.input {
                    if serde_json::to_string(input).is_ok_and(|text| stream::names_eval_data(&text))
                    {
                        self.tags.insert(RunTag::InspectedCorpus);
                    }
                }
                let output = part
                    .state
                    .output
                    .or_else(|| part.state.metadata.and_then(|metadata| metadata.output))
                    .unwrap_or_default();
                self.output_bytes += output.len();
                self.tool_output.push_str(&part.tool);
                self.tool_output.push('\n');
                self.tool_output.push_str(&output);
                self.tool_output.push('\n');
                if part.state.status == "error" {
                    self.failed += 1;
                }
                if stream::contains_eval_document(&output) {
                    self.tags.insert(RunTag::Contaminated);
                }
            }
            Event::StepFinish { part } => {
                self.model_calls += 1;
                self.input_tokens += part.tokens.input;
                self.output_tokens += part.tokens.output;
                self.cost += part.cost;
            }
            Event::Other => {}
        }
    }
}

fn parse(path: &Path) -> Result<Facts> {
    let file = File::open(path).with_context(|| format!("reading {}", path.display()))?;
    let mut facts = Facts::default();
    for line in BufReader::new(file).lines() {
        let line = line.with_context(|| format!("reading {}", path.display()))?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Event>(&line) {
            Ok(event) => facts.absorb(event),
            Err(_) => facts.unparsed += 1,
        }
    }
    Ok(facts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(lines: &[&str]) -> Facts {
        let mut facts = Facts::default();
        for line in lines {
            facts.absorb(serde_json::from_str(line).expect("an opencode event"));
        }
        facts
    }

    #[test]
    fn text_and_steps_become_the_answer_and_the_usage() {
        let facts = facts(&[
            r#"{"type":"step_start","sessionID":"ses_1"}"#,
            r#"{"type":"text","part":{"type":"text","text":"ok"}}"#,
            r#"{"type":"step_finish","part":{"tokens":{"input":10,"output":2},"cost":0.5}}"#,
        ]);
        assert_eq!(facts.answer, "ok");
        assert_eq!(facts.model_calls, 1);
        assert_eq!(facts.input_tokens, 10);
        assert_eq!(facts.output_tokens, 2);
        assert_eq!(facts.cost, 0.5);
        assert_eq!(facts.session.as_deref(), Some("ses_1"));
    }

    #[test]
    fn a_completed_tool_keeps_its_name_and_output() {
        let facts = facts(&[
            r#"{"type":"tool_use","part":{"type":"tool","tool":"bash","state":{"status":"completed","output":"one\ntwo\n"}}}"#,
        ]);
        assert_eq!(facts.tool_calls.get("bash"), Some(&1));
        assert_eq!(facts.output_bytes, 8);
        assert!(facts.tool_output.contains("one"));
    }

    #[test]
    fn a_corpus_probe_is_inspected_not_contaminated() {
        let facts = facts(&[
            r#"{"type":"tool_use","part":{"type":"tool","tool":"bash","state":{"status":"error","input":{"command":"ls integrations/rig-eval/scenarios/x.json"},"output":"Operation not permitted"}}}"#,
        ]);
        assert!(facts.tags.contains(&RunTag::InspectedCorpus));
        assert!(!facts.tags.contains(&RunTag::Contaminated));
    }
}
