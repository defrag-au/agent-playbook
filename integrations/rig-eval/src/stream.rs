//! rig's `--json` event stream, reduced to the facts a run is scored on.
//!
//! The shape is from `rig help-agents`: one JSON object per line, `type` naming the event. Only the
//! events a score reads are modelled; everything else is `Other`, and a line that does not parse is
//! counted rather than dropped.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::tag::RunTag;

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Event {
    #[serde(rename = "ready")]
    Ready {
        #[serde(default)]
        session: Option<String>,
    },
    #[serde(rename = "tool_call")]
    ToolCall {
        name: String,
        /// The one dynamic shape in the stream: each tool defines its own arguments. It is read
        /// for the contamination check only and never scored.
        #[serde(default)]
        args: Option<serde_json::Value>,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        name: String,
        #[serde(default)]
        ok: Option<bool>,
        #[serde(default)]
        refused: bool,
        #[serde(default)]
        cancelled: bool,
        #[serde(default)]
        output: String,
    },
    #[serde(rename = "turn_end")]
    TurnEnd {
        #[serde(default)]
        answer: Option<String>,
        #[serde(default)]
        error: Option<String>,
    },
    #[serde(rename = "usage")]
    Usage {
        #[serde(default)]
        purpose: String,
        #[serde(default)]
        usage: Option<Tokens>,
    },
    #[serde(rename = "notice")]
    Notice {
        #[serde(default)]
        level: String,
        #[serde(default)]
        text: String,
    },
    #[serde(rename = "exit")]
    Exit {
        #[serde(default)]
        code: Option<i32>,
        #[serde(default)]
        answer: Option<String>,
        #[serde(default)]
        error: Option<String>,
        #[serde(default)]
        session: Option<String>,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Default, Deserialize)]
struct Tokens {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    /// The part of `input_tokens` the provider read from its prompt cache.
    #[serde(default)]
    cached_input_tokens: Option<u64>,
    #[serde(default)]
    cost: Option<Cost>,
}

#[derive(Debug, Default, Deserialize)]
struct Cost {
    #[serde(default)]
    total: f64,
}

#[derive(Default)]
pub struct Facts {
    pub model_calls: usize,
    pub compactions: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: u64,
    pub cost: f64,
    pub tool_calls: std::collections::BTreeMap<String, usize>,
    pub tool_call_total: usize,
    pub tool_results: usize,
    pub refused: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub output_bytes: usize,
    /// The run called a playbook tool rather than the CLI.
    pub used_plugin_tools: bool,
    /// A shell call named one of the `at-*` binaries.
    pub used_toolkit_cli: bool,
    /// How many shell calls named one.
    pub toolkit_cli_calls: usize,
    /// The harness loaded a skill — on these harnesses, how the toolkit is discovered.
    pub loaded_skill: bool,
    /// Every tool result's text, for the `tool_says` checks.
    pub tool_output: String,
    pub unparsed: usize,
    pub answer: Option<String>,
    pub exit_code: Option<i32>,
    pub exit_error: Option<String>,
    pub session: Option<String>,
    /// Why this run is excluded, or what it inspected. Nothing else is a tag.
    pub tags: BTreeSet<RunTag>,
}

impl Facts {
    fn absorb(&mut self, event: Event) {
        match event {
            Event::Ready { session } => {
                if self.session.is_none() {
                    self.session = session;
                }
            }
            Event::ToolCall { name, args } => {
                *self.tool_calls.entry(name.clone()).or_default() += 1;
                self.tool_call_total += 1;
                if name == "skill" {
                    self.loaded_skill = true;
                }
                if is_plugin_tool(&name) {
                    self.used_plugin_tools = true;
                }
                if let Some(args) = args {
                    if let Ok(text) = serde_json::to_string(&args) {
                        if names_eval_data(&text) {
                            self.tags.insert(RunTag::InspectedCorpus);
                        }
                        if matches!(name.as_str(), "shell" | "bash") && names_toolkit(&text) {
                            self.used_toolkit_cli = true;
                            self.toolkit_cli_calls += 1;
                        }
                    }
                }
            }
            Event::ToolResult {
                name,
                ok,
                refused,
                cancelled,
                output,
            } => {
                self.tool_results += 1;
                self.output_bytes += output.len();
                self.tool_output.push_str(&name);
                self.tool_output.push('\n');
                self.tool_output.push_str(&output);
                self.tool_output.push('\n');
                if refused {
                    self.refused += 1;
                } else if ok == Some(false) {
                    self.failed += 1;
                }
                if cancelled {
                    self.cancelled += 1;
                }
                if contains_eval_document(&output) {
                    self.tags.insert(RunTag::Contaminated);
                }
            }
            Event::TurnEnd { answer, error } => {
                if answer.is_some() {
                    self.answer = answer;
                } else if let Some(error) = error {
                    self.exit_error = Some(error);
                }
            }
            Event::Usage { purpose, usage } => {
                self.model_calls += 1;
                if purpose == "compaction" {
                    self.compactions += 1;
                }
                if let Some(usage) = usage {
                    self.input_tokens += usage.input_tokens.unwrap_or_default();
                    self.output_tokens += usage.output_tokens.unwrap_or_default();
                    self.cached_input_tokens += usage.cached_input_tokens.unwrap_or_default();
                    self.cost += usage.cost.map(|cost| cost.total).unwrap_or_default();
                }
            }
            Event::Notice { level, text } => {
                if level == "error" {
                    self.exit_error.get_or_insert(text);
                }
            }
            Event::Exit {
                code,
                answer,
                error,
                session,
            } => {
                self.exit_code = code;
                if answer.is_some() {
                    self.answer = answer;
                }
                if let Some(error) = error {
                    self.exit_error = Some(error);
                }
                if session.is_some() {
                    self.session = session;
                }
            }
            Event::Other => {}
        }
    }
}

pub fn parse(path: &Path) -> Result<Facts> {
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

/// A corpus document or a scored result, pretty-printed: the array opens at the end of a line. The
/// design doc's schema example keeps the array on one line, so reading the design is not a false
/// positive.
pub fn contains_eval_document(text: &str) -> bool {
    text.contains("\"gold\": [\n") || text.contains("\"checks\": [\n")
}

/// A command that names the corpus or a prior run. Searching for a file called `summary.md` is not
/// a read of one; `find` cannot reach the corpus when it lives outside the checkout.
pub fn names_eval_data(text: &str) -> bool {
    ["/scenarios/", "rig-eval/archive", "/rig-eval/run-"]
        .iter()
        .any(|needle| text.contains(needle))
}

/// The binaries the toolkit installs. Used to tell a run that reached for the CLI from one that
/// only used its harness's own tools.
const TOOLKIT: &[&str] = &[
    "at-peek",
    "at-crate",
    "at-recall",
    "at-reach",
    "at-cost",
    "at-cardano",
    "at-describe",
];

pub fn names_toolkit(text: &str) -> bool {
    TOOLKIT.iter().any(|name| text.contains(name))
}

/// A tool the playbook plugin registers. The names are the plugin's own, so a shell call and a
/// plugin call are never confused.
pub fn is_plugin_tool(name: &str) -> bool {
    name.starts_with("at_peek_") || name.starts_with("at_recall_")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(lines: &[&str]) -> Facts {
        let mut facts = Facts::default();
        for line in lines {
            facts.absorb(serde_json::from_str(line).expect("a rig event"));
        }
        facts
    }

    #[test]
    fn naming_the_corpus_is_inspected_not_contaminated() {
        let facts = facts(&[
            r#"{"type":"tool_call","name":"bash","args":{"command":"ls integrations/rig-eval/scenarios/x.json"}}"#,
        ]);
        assert!(facts.tags.contains(&RunTag::InspectedCorpus));
        assert!(!facts.tags.contains(&RunTag::Contaminated));
    }

    #[test]
    fn a_corpus_document_in_the_output_is_contamination() {
        let facts =
            facts(&[r#"{"type":"tool_result","name":"bash","output":"{\n  \"gold\": [\n"}"#]);
        assert!(facts.tags.contains(&RunTag::Contaminated));
        assert!(!facts.tags.contains(&RunTag::InspectedCorpus));
    }

    #[test]
    fn a_cli_call_is_counted_once() {
        let facts = facts(&[
            r#"{"type":"tool_call","name":"bash","args":{"command":"at-peek search foo"}}"#,
            r#"{"type":"tool_call","name":"bash","args":{"command":"at-recall state"}}"#,
            r#"{"type":"tool_call","name":"bash","args":{"command":"ls"}}"#,
        ]);
        assert!(facts.used_toolkit_cli);
        assert_eq!(facts.toolkit_cli_calls, 2);
    }

    #[test]
    fn the_usage_carries_the_cached_part_of_the_input() {
        let facts = facts(&[
            r#"{"type":"usage","purpose":"turn","usage":{"input_tokens":100,"output_tokens":5,"cached_input_tokens":80,"cost":{"total":0.01}}}"#,
        ]);
        assert_eq!(facts.input_tokens, 100);
        assert_eq!(facts.cached_input_tokens, 80);
        assert_eq!(facts.cost, 0.01);
    }
}
