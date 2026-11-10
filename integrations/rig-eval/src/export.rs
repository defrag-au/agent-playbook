//! Flattened records for Observable.
//!
//! `FileAttachment("traces.json").json()` yields an array directly, and every record is a run with
//! four named groups — outcome, work, usage, status — rather than a long undifferentiated row. The
//! notebook flattens the groups once, for charts. `observable.md` beside the exports is that
//! starting cell set.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::report::RunResult;
use crate::score::Source;
use crate::tag::RunTag;

#[derive(Debug, Serialize, Deserialize)]
pub struct TraceRow {
    pub scenario: String,
    pub target: String,
    pub repeat: u32,
    pub model: String,
    pub outcome: Outcome,
    pub work: Work,
    pub usage: Usage,
    pub status: Status,
    pub tags: Vec<RunTag>,
    pub answer: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Outcome {
    pub passed: usize,
    pub total: usize,
    pub value: f64,
    pub precision: Option<f64>,
    pub citations: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Work {
    pub model_calls: usize,
    pub tool_calls: usize,
    pub tool_results: usize,
    pub compactions: usize,
    pub wall_ms: u64,
    pub refused: usize,
    pub failed: usize,
    pub cancelled: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub output_bytes: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Status {
    pub exit_code: Option<i32>,
    pub exit_error: Option<String>,
    pub session: Option<String>,
    pub unparsed_lines: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ToolRow {
    pub scenario: String,
    pub target: String,
    pub repeat: u32,
    pub tool: String,
    pub count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CheckRow {
    pub scenario: String,
    pub target: String,
    pub repeat: u32,
    pub required: String,
    pub source: Source,
    pub found: bool,
}

pub fn traces(results: &[RunResult]) -> Vec<TraceRow> {
    results
        .iter()
        .map(|result| TraceRow {
            scenario: result.scenario.clone(),
            target: result.target.clone(),
            repeat: result.repeat,
            model: result.model.clone(),
            outcome: Outcome {
                passed: result.passed,
                total: result.checks_total,
                value: result.score,
                precision: result.precision,
                citations: result.citations.len(),
            },
            work: Work {
                model_calls: result.model_calls,
                tool_calls: result.tool_call_total,
                tool_results: result.tool_results,
                compactions: result.compactions,
                wall_ms: result.wall_ms,
                refused: result.refused,
                failed: result.failed,
                cancelled: result.cancelled,
            },
            usage: Usage {
                input_tokens: result.input_tokens,
                output_tokens: result.output_tokens,
                cost_usd: result.cost_usd,
                output_bytes: result.output_bytes,
            },
            status: Status {
                exit_code: result.exit_code,
                exit_error: result.exit_error.clone(),
                session: result.session.clone(),
                unparsed_lines: result.unparsed_lines,
            },
            tags: result.tags.iter().copied().collect(),
            answer: result.answer.clone(),
        })
        .collect()
}

pub fn tools(results: &[RunResult]) -> Vec<ToolRow> {
    let mut rows = Vec::new();
    for result in results {
        for (tool, count) in &result.tool_calls {
            rows.push(ToolRow {
                scenario: result.scenario.clone(),
                target: result.target.clone(),
                repeat: result.repeat,
                tool: tool.clone(),
                count: *count,
            });
        }
    }
    rows
}

pub fn checks(results: &[RunResult]) -> Vec<CheckRow> {
    let mut rows = Vec::new();
    for result in results {
        for check in &result.checks {
            rows.push(CheckRow {
                scenario: result.scenario.clone(),
                target: result.target.clone(),
                repeat: result.repeat,
                required: check.required.clone(),
                source: check.source,
                found: check.found,
            });
        }
    }
    rows
}

pub fn write(directory: &Path, results: &[RunResult]) -> Result<()> {
    fs::create_dir_all(directory).with_context(|| format!("creating {}", directory.display()))?;
    write_json(directory, "traces.json", &traces(results))?;
    write_json(directory, "tools.json", &tools(results))?;
    write_json(directory, "checks.json", &checks(results))?;
    let notebook = directory.join("observable.md");
    fs::write(&notebook, NOTEBOOK).with_context(|| format!("writing {}", notebook.display()))?;
    Ok(())
}

fn write_json<T>(directory: &Path, name: &str, rows: &[T]) -> Result<()>
where
    T: Serialize,
{
    let path = directory.join(name);
    let text = serde_json::to_string_pretty(rows).with_context(|| format!("encoding {name}"))?;
    fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
}

const NOTEBOOK: &str = r#"# rig-eval · Observable

Three arrays sit beside this file:

- `traces.json` — one row per run, grouped as `outcome`, `work`, `usage` and `status`.
- `tools.json` — one row per `(scenario, target, repeat, tool)` with `count`.
- `checks.json` — one row per check: `required`, `source`, `found`.

```js
const traces = FileAttachment("traces.json").json();
const tools = FileAttachment("tools.json").json();
const checks = FileAttachment("checks.json").json();
```

```js
const runs = traces.map(d => ({
  scenario: d.scenario,
  target: d.target,
  repeat: d.repeat,
  score: d.outcome.value,
  passed: d.outcome.passed,
  checks: d.outcome.total,
  tool_calls: d.work.tool_calls,
  model_calls: d.work.model_calls,
  wall_ms: d.work.wall_ms,
  output_bytes: d.usage.output_bytes,
  cost_usd: d.usage.cost_usd,
  tags: d.tags
}));
```

```js
Inputs.table(runs, {
  columns: ["scenario", "target", "repeat", "score", "tool_calls", "output_bytes", "model_calls", "tags"],
  sort: "score"
})
```

```js
Plot.plot({
  height: 320,
  marginLeft: 70,
  x: {label: null},
  y: {grid: true, label: "checks passed"},
  color: {legend: true},
  marks: [
    Plot.barY(runs, Plot.groupX({y: "median"}, {x: "scenario", y: "score", fill: "target"})),
    Plot.ruleY([0])
  ]
})
```

```js
Plot.plot({
  grid: true,
  color: {legend: true},
  marks: [
    Plot.dot(runs, {x: "tool_calls", y: "score", fill: "target", tip: true}),
    Plot.ruleY([1])
  ]
})
```

```js
Plot.plot({
  marginLeft: 90,
  marks: [
    Plot.barX(tools, Plot.groupY({x: "sum"}, {y: "tool", x: "count", fill: "target"})),
    Plot.ruleX([0])
  ]
})
```

```js
Inputs.table(checks.filter(d => !d.found), {
  columns: ["scenario", "target", "repeat", "required", "source"]
})
```
"#;

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::score::Check;

    fn result() -> RunResult {
        RunResult {
            scenario: "s".to_owned(),
            target: "t".to_owned(),
            repeat: 1,
            model: "m".to_owned(),
            tool_calls: BTreeMap::from([("read".to_owned(), 2), ("bash".to_owned(), 1)]),
            tool_call_total: 3,
            checks: vec![Check {
                required: "a.rs".to_owned(),
                source: Source::Path,
                found: true,
            }],
            passed: 1,
            checks_total: 1,
            score: 1.0,
            tags: [RunTag::InspectedCorpus].into_iter().collect(),
            answer: Some("ok".to_owned()),
            ..RunResult::default()
        }
    }

    #[test]
    fn a_trace_groups_the_run_and_carries_its_tags() {
        let rows = traces(&[result()]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].scenario, "s");
        assert_eq!(rows[0].outcome.value, 1.0);
        assert_eq!(rows[0].work.tool_calls, 3);
        assert_eq!(rows[0].tags, vec![RunTag::InspectedCorpus]);
    }

    #[test]
    fn tools_are_one_row_per_name() {
        let rows = tools(&[result()]);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|row| row.tool == "read" && row.count == 2));
    }

    #[test]
    fn checks_are_one_row_each() {
        let rows = checks(&[result()]);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].found);
    }

    #[test]
    fn the_export_writes_arrays_that_load_as_json() {
        let directory =
            std::env::temp_dir().join(format!("rig-eval-export-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        write(&directory, &[result()]).expect("writing the export");
        let text = fs::read_to_string(directory.join("traces.json")).expect("traces");
        let rows: Vec<TraceRow> = serde_json::from_str(&text).expect("an array");
        assert_eq!(rows.len(), 1);
        let _ = fs::remove_dir_all(&directory);
    }
}
