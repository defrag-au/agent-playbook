//! rig's `--json` event stream, reduced to the facts a run is scored on.
//!
//! The shape is from `rig help-agents`: one JSON object per line, `type` naming the event. Only the
//! events a score reads are modelled; everything else is `Other`, and a line that does not parse is
//! counted rather than dropped.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

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
    pub cost: f64,
    pub tool_calls: std::collections::BTreeMap<String, usize>,
    pub tool_call_total: usize,
    pub tool_results: usize,
    pub refused: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub output_bytes: usize,
    /// Every tool result's text, for the `tool_says` checks.
    pub tool_output: String,
    pub unparsed: usize,
    pub answer: Option<String>,
    pub exit_code: Option<i32>,
    pub exit_error: Option<String>,
    pub session: Option<String>,
    pub contaminated: bool,
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
                *self.tool_calls.entry(name).or_default() += 1;
                self.tool_call_total += 1;
                if let Some(args) = args {
                    if serde_json::to_string(&args).is_ok_and(|text| names_eval_data(&text)) {
                        self.contaminated = true;
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
                // A corpus document or a scored result, pretty-printed: the array opens at the end
                // of a line. The design doc's schema example keeps the array on one line, so
                // reading the design is not a false positive.
                if output.contains("\"gold\": [\n") || output.contains("\"checks\": [\n") {
                    self.contaminated = true;
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

/// A command that names the corpus or a prior run. Searching for a file called `summary.md` is not
/// a read of one; `find` cannot reach the corpus when it lives outside the checkout.
fn names_eval_data(text: &str) -> bool {
    ["/scenarios/", "rig-eval/archive", "/rig-eval/run-"]
        .iter()
        .any(|needle| text.contains(needle))
}
