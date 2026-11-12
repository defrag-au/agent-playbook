//! The Pi arm: `pi --mode json`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

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
    repeat: u32,
    context: &RunContext<'_>,
) -> Result<RunResult> {
    let stem = format!("{}-{}-r{repeat}", scenario.name, context.arm);
    let invocation = Invocation {
        program: context.args.pi.clone(),
        args: vec![
            "--mode".to_owned(),
            "json".to_owned(),
            "--model".to_owned(),
            context.args.model.clone(),
            "--no-session".to_owned(),
            scenario.prompt.clone(),
        ],
        env: context.env.to_vec(),
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
    let exit_error = if completed.status.success() && !facts.aborted {
        None
    } else if facts.aborted {
        Some("the harness aborted the run".to_owned())
    } else {
        process::tail(&completed.stderr)
    };

    Ok(RunResult {
        scenario: scenario.name.clone(),
        target: "pi".to_owned(),
        repeat,
        model: context.args.model.clone(),
        workdir: scenario.workdir.clone(),
        wall_ms: completed.wall.as_millis().try_into().unwrap_or(u64::MAX),
        exit_code: completed.status.code(),
        exit_error,
        session: facts.session,
        effects: None,
        model_calls: facts.model_calls,
        compactions: facts.compactions,
        input_tokens: facts.input_tokens,
        output_tokens: facts.output_tokens,
        cached_input_tokens: facts.cached_input_tokens,
        cost_usd: facts.cost,
        tool_calls: facts.tool_calls,
        tool_call_total: facts.tool_call_total,
        tool_results: facts.tool_results,
        refused: 0,
        failed: facts.failed,
        cancelled: 0,
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

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Event {
    #[serde(rename = "session")]
    Session { id: String },
    #[serde(rename = "turn_start")]
    TurnStart,
    #[serde(rename = "message_end")]
    MessageEnd { message: Message },
    #[serde(rename = "tool_execution_start")]
    ToolStart {
        #[serde(rename = "toolName")]
        tool: String,
        /// Tool arguments are dynamic per tool; read for contamination only.
        #[serde(default)]
        args: Option<serde_json::Value>,
    },
    #[serde(rename = "tool_execution_end")]
    ToolEnd {
        #[serde(rename = "toolName")]
        tool: String,
        #[serde(default)]
        result: ToolResult,
        #[serde(rename = "isError", default)]
        is_error: bool,
    },
    #[serde(rename = "compaction_start")]
    CompactionStart,
    #[serde(rename = "agent_settled")]
    Settled {
        #[serde(default)]
        aborted: bool,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
struct Message {
    #[serde(default)]
    role: String,
    /// Pi writes a string body on the system message and parts on the others.
    #[serde(default)]
    content: Option<Content>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Content {
    Parts(Vec<Part>),
    Text(String),
}

impl Content {
    fn text(&self) -> String {
        match self {
            Content::Parts(parts) => parts
                .iter()
                .filter(|part| part.kind == "text")
                .filter_map(|part| part.text.as_deref())
                .collect::<Vec<_>>()
                .join(""),
            Content::Text(text) => text.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct Part {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ToolResult {
    #[serde(default)]
    content: Option<Content>,
}

#[derive(Debug, Default, Deserialize)]
struct Usage {
    #[serde(default)]
    input: u64,
    #[serde(default)]
    output: u64,
    /// The part of `input` the provider read from its prompt cache.
    #[serde(default, rename = "cacheRead")]
    cache_read: u64,
    #[serde(default)]
    cost: Option<Cost>,
}

#[derive(Debug, Default, Deserialize)]
struct Cost {
    #[serde(default)]
    total: f64,
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
    cached_input_tokens: u64,
    cost: f64,
    compactions: usize,
    failed: usize,
    session: Option<String>,
    aborted: bool,
    unparsed: usize,
    used_plugin_tools: bool,
    used_toolkit_cli: bool,
    toolkit_cli_calls: usize,
    loaded_skill: bool,
    tags: BTreeSet<RunTag>,
}

impl Facts {
    fn absorb(&mut self, event: Event) {
        match event {
            Event::Session { id } => self.session = Some(id),
            Event::TurnStart => self.model_calls += 1,
            Event::MessageEnd { message } => {
                if message.role != "assistant" {
                    return;
                }
                self.answer = message
                    .content
                    .as_ref()
                    .map(Content::text)
                    .unwrap_or_default();
                if let Some(usage) = message.usage {
                    // Pi reports uncached input and cache reads separately; a run's input count is
                    // every input token sent, so the arms stay comparable.
                    self.input_tokens += usage.input + usage.cache_read;
                    self.output_tokens += usage.output;
                    self.cached_input_tokens += usage.cache_read;
                    self.cost += usage.cost.map(|cost| cost.total).unwrap_or_default();
                }
            }
            Event::ToolStart { tool, args } => {
                *self.tool_calls.entry(tool.clone()).or_default() += 1;
                self.tool_call_total += 1;
                if tool == "skill" {
                    self.loaded_skill = true;
                }
                if stream::is_plugin_tool(&tool) {
                    self.used_plugin_tools = true;
                }
                if let Some(args) = args {
                    if let Ok(text) = serde_json::to_string(&args) {
                        if stream::names_eval_data(&text) {
                            self.tags.insert(RunTag::InspectedCorpus);
                        }
                        if tool == "bash" && stream::names_toolkit(&text) {
                            self.used_toolkit_cli = true;
                            self.toolkit_cli_calls += 1;
                        }
                    }
                }
            }
            Event::ToolEnd {
                tool,
                result,
                is_error,
            } => {
                self.tool_results += 1;
                let output = result
                    .content
                    .as_ref()
                    .map(Content::text)
                    .unwrap_or_default();
                self.output_bytes += output.len();
                self.tool_output.push_str(&tool);
                self.tool_output.push('\n');
                self.tool_output.push_str(&output);
                self.tool_output.push('\n');
                if is_error {
                    self.failed += 1;
                }
                if stream::contains_eval_document(&output) {
                    self.tags.insert(RunTag::Contaminated);
                }
            }
            Event::CompactionStart => self.compactions += 1,
            Event::Settled { aborted } => self.aborted = aborted,
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
            facts.absorb(serde_json::from_str(line).expect("a Pi event"));
        }
        facts
    }

    #[test]
    fn the_last_assistant_message_carries_the_cached_part_of_the_input() {
        let facts = facts(&[
            r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"ok"}],"usage":{"input":100,"output":5,"cacheRead":80,"cost":{"total":0.01}}}}"#,
        ]);
        assert_eq!(facts.input_tokens, 180, "cache reads are input too");
        assert_eq!(facts.cached_input_tokens, 80);
    }

    #[test]
    fn the_last_assistant_message_is_the_answer() {
        let facts = facts(&[
            r#"{"type":"session","id":"s1"}"#,
            r#"{"type":"turn_start"}"#,
            r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"first"}],"usage":{"input":5,"output":1,"cost":{"total":0.1}}}}"#,
            r#"{"type":"turn_start"}"#,
            r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"second"}],"usage":{"input":6,"output":2,"cost":{"total":0.2}}}}"#,
        ]);
        assert_eq!(facts.answer, "second");
        assert_eq!(facts.model_calls, 2);
        assert_eq!(facts.input_tokens, 11);
        assert_eq!(facts.output_tokens, 3);
        assert!((facts.cost - 0.3).abs() < f64::EPSILON);
        assert_eq!(facts.session.as_deref(), Some("s1"));
    }

    #[test]
    fn a_tool_keeps_its_name_output_and_failure() {
        let facts = facts(&[
            r#"{"type":"tool_execution_start","toolName":"bash","args":{"command":"ls"}}"#,
            r#"{"type":"tool_execution_end","toolName":"bash","result":{"content":[{"type":"text","text":"one\ntwo"}]},"isError":false}"#,
            r#"{"type":"compaction_start"}"#,
        ]);
        assert_eq!(facts.tool_calls.get("bash"), Some(&1));
        assert_eq!(facts.output_bytes, 7);
        assert_eq!(facts.compactions, 1);
        assert_eq!(facts.failed, 0);
    }

    #[test]
    fn a_system_message_with_a_string_body_parses() {
        let facts = facts(&[
            r#"{"type":"message_end","message":{"role":"system","content":"you are pi"}}"#,
        ]);
        assert_eq!(facts.answer, "");
    }

    #[test]
    fn a_corpus_probe_is_inspected_not_contaminated() {
        let facts = facts(&[
            r#"{"type":"tool_execution_start","toolName":"bash","args":{"command":"ls integrations/rig-eval/scenarios/x.json"}}"#,
        ]);
        assert!(facts.tags.contains(&RunTag::InspectedCorpus));
        assert!(!facts.tags.contains(&RunTag::Contaminated));
    }
}
