//! What a run produced, and the summary written beside the streams.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::cli::Args;
use crate::score::Check;

#[derive(Debug, Serialize)]
pub struct RunResult {
    pub scenario: String,
    pub home: String,
    pub repeat: u32,
    pub model: String,
    pub workdir: String,
    pub wall_ms: u64,
    pub timed_out: bool,
    pub exit_code: Option<i32>,
    pub exit_error: Option<String>,
    pub session: Option<String>,
    pub effects: Option<String>,
    pub model_calls: usize,
    pub compactions: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub tool_calls: std::collections::BTreeMap<String, usize>,
    pub tool_call_total: usize,
    pub tool_results: usize,
    pub refused: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub output_bytes: usize,
    pub unparsed_lines: usize,
    pub contaminated: bool,
    pub answer: Option<String>,
    pub citations: Vec<String>,
    pub checks: Vec<Check>,
    pub passed: usize,
    pub checks_total: usize,
    pub score: f64,
    pub precision: Option<f64>,
}

pub fn print_run(result: &RunResult) {
    let missed: Vec<&str> = result
        .checks
        .iter()
        .filter(|check| !check.found)
        .map(|check| check.required.as_str())
        .collect();
    println!(
        "  {:>5.1}s · {} model call(s) · {} tool call(s) · {} refused · {} compaction(s) · {} output · score {:.2} · {}",
        result.wall_ms as f64 / 1000.0,
        result.model_calls,
        result.tool_call_total,
        result.refused,
        result.compactions,
        human_bytes(result.output_bytes),
        result.score,
        if missed.is_empty() {
            "all checks passed".to_owned()
        } else {
            format!("missed: {}", missed.join(", "))
        }
    );
    if result.unparsed_lines > 0 {
        println!(
            "  {} stream line(s) did not parse; the counts are incomplete",
            result.unparsed_lines
        );
    }
    if result.contaminated {
        println!("  CONTAMINATED: a tool read the scenario corpus; this run is not evidence");
    }
    if result.timed_out {
        println!("  TIMED OUT: the launcher was killed");
    }
    if let Some(error) = &result.exit_error {
        println!("  exit: {error}");
    }
}

pub fn write(args: &Args, results: &[RunResult]) -> Result<()> {
    let json = serde_json::to_string_pretty(results).context("encoding the results")?;
    let path = args.out.join("results.json");
    fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;

    let path = args.out.join("summary.md");
    fs::write(&path, summary(args, results))
        .with_context(|| format!("writing {}", path.display()))?;

    println!("\nwrote {}", args.out.display());
    Ok(())
}

fn summary(args: &Args, results: &[RunResult]) -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "# rig-eval · {} scenario(s) · {} home(s) · {} repeat(s)\n",
        results
            .iter()
            .map(|result| result.scenario.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        args.homes.len(),
        args.repeat
    );
    let _ = writeln!(
        text,
        "Model `{}` · launcher `{}`.",
        args.model,
        args.rig.display()
    );
    let _ = writeln!(
        text,
        "Score is checks passed: `path:line` citations in the answer, paths in the answer, \
         literals in the answer, and literals in the tool output.\n"
    );

    let _ = writeln!(
        text,
        "| Scenario | Home | Score | Recall | Prec | Model calls | Compactions | Tool calls | Refused | Output | Time |"
    );
    let _ = writeln!(
        text,
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |"
    );
    for result in results {
        let _ = writeln!(
            text,
            "| {} | {} | {:.2} | {}/{} | {} | {} | {} | {} | {} | {} | {:.1}s |",
            result.scenario,
            result.home,
            result.score,
            result.passed,
            result.checks_total,
            result
                .precision
                .map(|precision| format!("{precision:.2}"))
                .unwrap_or_else(|| "—".to_owned()),
            result.model_calls,
            result.compactions,
            result.tool_call_total,
            result.refused,
            human_bytes(result.output_bytes),
            result.wall_ms as f64 / 1000.0,
        );
    }

    let _ = writeln!(text, "\n## Paired\n");
    let _ = writeln!(
        text,
        "| Scenario | Tool calls (gated / baseline) | Output bytes | Model calls | Score |"
    );
    let _ = writeln!(text, "| --- | --- | --- | --- | --- |");
    for pairing in pairings(results) {
        let _ = writeln!(
            text,
            "| {} | {} / {} | {} / {} | {} / {} | {:.2} / {:.2} |",
            pairing.scenario,
            pairing.gated_tool_calls,
            pairing.baseline_tool_calls,
            human_bytes(pairing.gated_output_bytes),
            human_bytes(pairing.baseline_output_bytes),
            pairing.gated_model_calls,
            pairing.baseline_model_calls,
            pairing.gated_score,
            pairing.baseline_score,
        );
    }

    let _ = writeln!(text, "\n## Misses\n");
    let mut missed_any = false;
    for result in results {
        let missed: Vec<String> = result
            .checks
            .iter()
            .filter(|check| !check.found)
            .map(|check| format!("`{}` ({:?})", check.required, check.source))
            .collect();
        if missed.is_empty() {
            continue;
        }
        missed_any = true;
        let _ = writeln!(
            text,
            "- {} · {} · run {}: {}",
            result.scenario,
            result.home,
            result.repeat,
            missed.join(", ")
        );
    }
    if !missed_any {
        let _ = writeln!(text, "None: every check passed in every run.");
    }

    let contaminated: Vec<&RunResult> = results
        .iter()
        .filter(|result| result.contaminated)
        .collect();
    if !contaminated.is_empty() {
        let _ = writeln!(text, "\n## Contaminated\n");
        for result in contaminated {
            let _ = writeln!(
                text,
                "- {} · {} · run {}: a tool read `scenarios/`; exclude this run.",
                result.scenario, result.home, result.repeat
            );
        }
    }

    let _ = writeln!(
        text,
        "\nStreams, stderr and effect logs are beside this file as `<scenario>-<home>-r<n>.*`."
    );
    text
}

struct Pairing {
    scenario: String,
    gated_tool_calls: usize,
    baseline_tool_calls: usize,
    gated_output_bytes: usize,
    baseline_output_bytes: usize,
    gated_model_calls: usize,
    baseline_model_calls: usize,
    gated_score: f64,
    baseline_score: f64,
}

fn pairings(results: &[RunResult]) -> Vec<Pairing> {
    let mut pairings = Vec::new();
    let scenarios: BTreeSet<&str> = results
        .iter()
        .map(|result| result.scenario.as_str())
        .collect();
    for scenario in scenarios {
        let of = |home: &str| -> Vec<&RunResult> {
            results
                .iter()
                .filter(|result| result.scenario == scenario && result.home == home)
                .collect()
        };
        let gated = of("gated");
        let baseline = of("baseline");
        if gated.is_empty() || baseline.is_empty() {
            continue;
        }
        pairings.push(Pairing {
            scenario: scenario.to_owned(),
            gated_tool_calls: median(gated.iter().map(|result| result.tool_call_total)),
            baseline_tool_calls: median(baseline.iter().map(|result| result.tool_call_total)),
            gated_output_bytes: median(gated.iter().map(|result| result.output_bytes)),
            baseline_output_bytes: median(baseline.iter().map(|result| result.output_bytes)),
            gated_model_calls: median(gated.iter().map(|result| result.model_calls)),
            baseline_model_calls: median(baseline.iter().map(|result| result.model_calls)),
            gated_score: median_f64(gated.iter().map(|result| result.score)),
            baseline_score: median_f64(baseline.iter().map(|result| result.score)),
        });
    }
    pairings
}

fn median(values: impl Iterator<Item = usize>) -> usize {
    let mut values: Vec<usize> = values.collect();
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    values[values.len() / 2]
}

fn median_f64(values: impl Iterator<Item = f64>) -> f64 {
    let mut values: Vec<f64> = values.collect();
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn human_bytes(bytes: usize) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_are_human() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1536), "1.5 KB");
    }

    #[test]
    fn the_median_of_one_value_is_that_value() {
        assert_eq!(median([7].into_iter()), 7);
        assert_eq!(median_f64([0.5].into_iter()), 0.5);
    }
}
