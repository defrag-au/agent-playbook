//! What a run produced, the summary written beside the streams, and the dry-run plan.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::cli::Args;
use crate::scenario::Scenario;
use crate::score::Check;
use crate::tag::RunTag;
use crate::target::Target;

#[derive(Debug, Serialize)]
pub struct RunResult {
    pub scenario: String,
    pub target: String,
    pub repeat: u32,
    pub model: String,
    pub workdir: String,
    pub wall_ms: u64,
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
    /// Why this run is excluded, or what it inspected.
    pub tags: BTreeSet<RunTag>,
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
    if result.tags.contains(&RunTag::Contaminated) {
        println!(
            "  CONTAMINATED: a tool output contained the expected answer; this run is excluded"
        );
    } else if result.tags.contains(&RunTag::InspectedCorpus) {
        println!("  inspected: a tool call named the sealed corpus; the read was refused");
    }
    if result.tags.contains(&RunTag::TimedOut) {
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

pub fn print_plan(args: &Args, scenarios: &[Scenario]) {
    println!(
        "# rig-eval · {} scenario(s) · {} target(s) · {} run(s) each",
        scenarios.len(),
        args.targets.len(),
        args.repeat
    );
    println!("rig      {}", args.rig.display());
    println!(
        "opencode bunx {} ({})",
        crate::driver::opencode::PACKAGE,
        args.opencode_config.display()
    );
    println!("pi       {}", args.pi.display());
    println!("model    {}", args.model);
    match crate::cli::read_env(&args.env_file) {
        Ok(vars) => {
            let keys: Vec<&str> = vars.iter().map(|(key, _)| key.as_str()).collect();
            println!("env      {} ({})", args.env_file.display(), keys.join(", "));
        }
        Err(why) => println!("env      {why}"),
    }
    println!("out      {}", args.out.display());
    for target in &args.targets {
        match target {
            Target::Rig(home) => {
                let state = if home.path.join("settings.toml").is_file() {
                    "settings.toml"
                } else {
                    "MISSING settings.toml"
                };
                println!("{:12} {} ({state})", target.label(), home.path.display());
            }
            Target::Opencode => {
                println!("{:12} {}", target.label(), args.opencode_config.display())
            }
            Target::Pi => println!("{:12} {}", target.label(), args.pi.display()),
        }
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

fn summary(args: &Args, results: &[RunResult]) -> String {
    // A run that saw the expected answer is not evidence. Keep it in `results.json` and name it
    // below, but never let it into a table or a median.
    let clean: Vec<&RunResult> = results
        .iter()
        .filter(|result| !result.tags.contains(&RunTag::Contaminated))
        .collect();
    let mut text = String::new();
    let _ = writeln!(
        text,
        "# rig-eval · {} scenario(s) · {} target(s) · {} repeat(s)\n",
        clean
            .iter()
            .map(|result| result.scenario.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        args.targets.len(),
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
        "| Scenario | Target | Score | Recall | Prec | Model calls | Compactions | Tool calls | Refused | Output | Time |"
    );
    let _ = writeln!(
        text,
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |"
    );
    for result in &clean {
        let _ = writeln!(
            text,
            "| {} | {} | {:.2} | {}/{} | {} | {} | {} | {} | {} | {} | {:.1}s |",
            result.scenario,
            result.target,
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

    let _ = writeln!(text, "\n## By target\n");
    let _ = writeln!(
        text,
        "| Target | Runs | Median score | Median tool calls | Median output | Median model calls |"
    );
    let _ = writeln!(text, "| --- | --- | --- | --- | --- | --- |");
    for summary in by_target(clean.iter().copied()) {
        let _ = writeln!(
            text,
            "| {} | {} | {:.2} | {} | {} | {} |",
            summary.target,
            summary.runs,
            summary.score,
            summary.tool_calls,
            human_bytes(summary.output_bytes),
            summary.model_calls,
        );
    }

    let _ = writeln!(text, "\n## Misses\n");
    let mut missed_any = false;
    for result in &clean {
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
            result.target,
            result.repeat,
            missed.join(", ")
        );
    }
    if !missed_any {
        let _ = writeln!(text, "None: every check passed in every run.");
    }

    if clean.len() != results.len() {
        let _ = writeln!(
            text,
            "\n{} contaminated run(s) are excluded from the tables and medians above.",
            results.len() - clean.len()
        );
    }

    let contaminated: Vec<&RunResult> = results
        .iter()
        .filter(|result| result.tags.contains(&RunTag::Contaminated))
        .collect();
    if !contaminated.is_empty() {
        let _ = writeln!(text, "\n## Contaminated\n");
        for result in contaminated {
            let _ = writeln!(
                text,
                "- {} · {} · run {}: a tool output contained `scenarios/`; excluded.",
                result.scenario, result.target, result.repeat
            );
        }
    }

    let inspected: Vec<&RunResult> = results
        .iter()
        .filter(|result| {
            result.tags.contains(&RunTag::InspectedCorpus)
                && !result.tags.contains(&RunTag::Contaminated)
        })
        .collect();
    if !inspected.is_empty() {
        let _ = writeln!(text, "\n## Inspected the corpus\n");
        for result in inspected {
            let _ = writeln!(
                text,
                "- {} · {} · run {}: a tool call named the sealed corpus; the read was refused.",
                result.scenario, result.target, result.repeat
            );
        }
    }

    let _ = writeln!(
        text,
        "\nStreams, stderr and effect logs are beside this file as `<scenario>-<target>-r<n>.*`."
    );
    text
}

struct TargetSummary {
    target: String,
    runs: usize,
    score: f64,
    tool_calls: usize,
    output_bytes: usize,
    model_calls: usize,
}

fn by_target<'a>(results: impl Iterator<Item = &'a RunResult>) -> Vec<TargetSummary> {
    let results: Vec<&RunResult> = results.collect();
    let mut summaries = Vec::new();
    let targets: BTreeSet<&str> = results
        .iter()
        .map(|result| result.target.as_str())
        .collect();
    for target in targets {
        let runs: Vec<&RunResult> = results
            .iter()
            .copied()
            .filter(|result| result.target == target)
            .collect();
        if runs.is_empty() {
            continue;
        }
        summaries.push(TargetSummary {
            target: target.to_owned(),
            runs: runs.len(),
            score: median_f64(runs.iter().map(|result| result.score)),
            tool_calls: median(runs.iter().map(|result| result.tool_call_total)),
            output_bytes: median(runs.iter().map(|result| result.output_bytes)),
            model_calls: median(runs.iter().map(|result| result.model_calls)),
        });
    }
    summaries
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
    use std::path::PathBuf;

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

    #[test]
    fn a_contaminated_run_is_not_in_the_tables() {
        let clean = result("pi", BTreeSet::new());
        let contaminated = result("rig:gated", BTreeSet::from([RunTag::Contaminated]));
        let text = summary(&args(), &[clean, contaminated]);
        assert!(text.contains("| s | pi |"));
        assert!(!text.contains("| s | rig:gated |"));
        assert!(text.contains("1 contaminated run(s) are excluded"));
        assert!(text.contains("## Contaminated"));
    }

    fn args() -> Args {
        let root = PathBuf::from("/repo");
        Args {
            root: root.clone(),
            scenario_dir: root.join("scenarios"),
            only: Vec::new(),
            targets: Vec::new(),
            model: "model".to_owned(),
            rig: root.join("rig"),
            opencode_config: root.join("opencode.json"),
            pi: root.join("pi"),
            env_file: root.join("env"),
            out: root.join("out"),
            repeat: 1,
            timeout: None,
            dry_run: false,
        }
    }

    fn result(target: &str, tags: BTreeSet<RunTag>) -> RunResult {
        RunResult {
            scenario: "s".to_owned(),
            target: target.to_owned(),
            repeat: 1,
            model: "model".to_owned(),
            workdir: ".".to_owned(),
            wall_ms: 0,
            exit_code: Some(0),
            exit_error: None,
            session: None,
            effects: None,
            model_calls: 0,
            compactions: 0,
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: 0.0,
            tool_calls: Default::default(),
            tool_call_total: 0,
            tool_results: 0,
            refused: 0,
            failed: 0,
            cancelled: 0,
            output_bytes: 0,
            unparsed_lines: 0,
            tags,
            answer: None,
            citations: Vec::new(),
            checks: Vec::new(),
            passed: 0,
            checks_total: 0,
            score: 1.0,
            precision: None,
        }
    }
}
