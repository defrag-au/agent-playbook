//! What a run produced, the summary written beside the streams, and the dry-run plan.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::cli::Args;
use crate::manifest::Manifest;
use crate::scenario::Scenario;
use crate::score::Check;
use crate::tag::RunTag;
use crate::target::Target;

#[derive(Debug, Default, Serialize, Deserialize)]
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
    /// The part of `input_tokens` the provider read from its prompt cache. Absent from an older
    /// `results.json`, so it reads as zero rather than refusing the file.
    #[serde(default)]
    pub cached_input_tokens: u64,
    pub cost_usd: f64,
    pub tool_calls: std::collections::BTreeMap<String, usize>,
    pub tool_call_total: usize,
    pub tool_results: usize,
    pub refused: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub output_bytes: usize,
    /// The run called a playbook plugin tool.
    #[serde(default)]
    pub used_plugin_tools: bool,
    /// A shell call named one of the `at-*` binaries.
    #[serde(default)]
    pub used_toolkit_cli: bool,
    /// How many shell calls named one. Absent from an older `results.json`.
    #[serde(default)]
    pub toolkit_cli_calls: usize,
    /// The harness loaded a skill — on these harnesses, how the toolkit is discovered.
    #[serde(default)]
    pub loaded_skill: bool,
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

impl RunResult {
    /// What one passed check cost, in microdollars — the unit the invoice is actually made of.
    ///
    /// A run that passed nothing, or that the provider did not price, has no per-check cost to
    /// state: the column says so rather than dividing by zero or reporting a run that looks free.
    pub fn cost_per_check(&self) -> Option<f64> {
        (self.passed > 0 && self.cost_usd > 0.0)
            .then(|| self.cost_usd / self.passed as f64 * 1_000_000.0)
    }

    /// The share of input tokens the provider read from its prompt cache, in percent.
    ///
    /// This is the leading indicator behind the bill: a stable prompt prefix is charged at a
    /// fraction of the uncached rate, and a change that breaks the prefix shows up here first.
    pub fn cache_share(&self) -> Option<f64> {
        (self.input_tokens > 0)
            .then(|| self.cached_input_tokens as f64 / self.input_tokens as f64 * 100.0)
    }

    /// How many playbook plugin tools this run called. CLI calls are shell calls under another
    /// name, so they are counted by `used_toolkit_cli`, not here.
    pub fn plugin_calls(&self) -> usize {
        self.tool_calls
            .iter()
            .filter(|(tool, _)| tool.starts_with("at_"))
            .map(|(_, count)| count)
            .sum()
    }

    /// Whether every check passed. A run with no checks is not solved: it measured nothing.
    pub fn solved(&self) -> bool {
        self.checks_total > 0 && self.passed == self.checks_total
    }

    /// The number of shell calls that named an `at-*` binary, when the run recorded it.
    ///
    /// An older `results.json` has the boolean but no count. A run that used the CLI cannot have
    /// zero calls, so a zero beside a true flag means "not recorded" rather than "none".
    pub fn cli_calls(&self) -> Option<usize> {
        (self.toolkit_cli_calls > 0 || !self.used_toolkit_cli).then_some(self.toolkit_cli_calls)
    }
}

pub fn print_run(result: &RunResult) {
    let missed: Vec<&str> = result
        .checks
        .iter()
        .filter(|check| !check.found)
        .map(|check| check.required.as_str())
        .collect();
    let mut line = format!(
        "  {:>5.1}s · {} model call(s) · {} tool call(s)",
        result.wall_ms as f64 / 1000.0,
        result.model_calls,
        result.tool_call_total,
    );
    if let Some(check) = result.cost_per_check() {
        line.push_str(&format!(" · {check:.0} µ$/check"));
    }
    if let Some(cache) = result.cache_share() {
        line.push_str(&format!(" · {cache:.0}% cached"));
    }
    line.push_str(&format!(
        " · {} output · score {:.2}",
        human_bytes(result.output_bytes),
        result.score
    ));
    if result.refused > 0 {
        line.push_str(&format!(" · {} refused", result.refused));
    }
    if result.compactions > 0 {
        line.push_str(&format!(" · {} compaction(s)", result.compactions));
    }
    let tail = if missed.is_empty() {
        "all checks passed".to_owned()
    } else {
        format!("missed: {}", missed.join(", "))
    };
    line.push_str(" · ");
    line.push_str(&tail);
    println!("{line}");
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

pub fn write(args: &Args, results: &[RunResult], identity: Option<&Manifest>) -> Result<()> {
    let json = serde_json::to_string_pretty(results).context("encoding the results")?;
    let path = args.out.join("results.json");
    fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;

    let path = args.out.join("summary.md");
    fs::write(&path, summary(results, identity))
        .with_context(|| format!("writing {}", path.display()))?;

    crate::export::write(&args.out, results)?;
    println!("\nwrote {}", args.out.display());
    Ok(())
}

/// Read a run's `results.json` back, for `--reuse`.
pub fn read(directory: &Path) -> Result<Vec<RunResult>> {
    let path = directory.join("results.json");
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
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
    println!("jobs     {}", args.jobs);
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

fn summary(results: &[RunResult], identity: Option<&Manifest>) -> String {
    // A run that saw the expected answer is not evidence. Keep it in `results.json` and name it
    // below, but never let it into a table or a median.
    let clean: Vec<&RunResult> = results
        .iter()
        .filter(|result| !result.tags.contains(&RunTag::Contaminated))
        .collect();
    let scenarios = distinct(clean.iter().map(|result| result.scenario.as_str()));
    let targets = arm_order(&clean, identity);
    let model = clean
        .first()
        .map(|result| result.model.as_str())
        .or_else(|| results.first().map(|result| result.model.as_str()))
        .unwrap_or("unknown");

    let mut text = String::new();
    let _ = writeln!(text, "# rig-eval · {model}\n");

    match identity {
        Some(identity) => {
            let _ = writeln!(
                text,
                "Corpus `{}` ({} files) · checkout `{}` on `{}`{} · snapshot `{}` · runner {} {}\n",
                identity.corpus.fingerprint,
                identity.corpus.files.len(),
                short(identity.checkout.head.as_deref().unwrap_or("unknown")),
                identity.checkout.branch.as_deref().unwrap_or("unknown"),
                if identity.checkout.dirty {
                    " (dirty)"
                } else {
                    ""
                },
                short(&identity.snapshot.commit),
                identity.runner.name,
                identity.runner.version,
            );
        }
        None => {
            let _ = writeln!(
                text,
                "No manifest: this summary was rendered from results alone, so the corpus and \
                 checkout it measured are not recorded.\n"
            );
        }
    }

    let passed: usize = clean.iter().map(|result| result.passed).sum();
    let checks: usize = clean.iter().map(|result| result.checks_total).sum();
    let cost: f64 = clean.iter().map(|result| result.cost_usd).sum();
    let input: u64 = clean.iter().map(|result| result.input_tokens).sum();
    let cached: u64 = clean.iter().map(|result| result.cached_input_tokens).sum();
    let _ = writeln!(
        text,
        "{} · {} · {}/{} checks passed ({}) · {} · {} input ({} cached)\n",
        plural(clean.len(), "run"),
        plural(targets.len(), "arm"),
        passed,
        checks,
        rate_text(passed, checks),
        money(cost),
        human_tokens(input),
        rate_text(cached as usize, input as usize),
    );
    let _ = writeln!(
        text,
        "`Cost/check` is billed microdollars per passed check; `Cost/solved` is billed microdollars \
         per run that passed every check. `Cache` is the share of input tokens read from the \
         provider's prompt cache. A dash means it cannot be stated. A range is min–max across the \
         arm's runs.\n"
    );

    let _ = writeln!(text, "## By arm\n");
    let _ = writeln!(
        text,
        "| Arm | Runs | Pass rate | Solved | Score | Cost/check (µ$) | Cost/solved (µ$) | Cache |"
    );
    let _ = writeln!(text, "| --- | --- | --- | --- | --- | --- | --- | --- |");
    for target in &targets {
        let arm: Vec<&RunResult> = clean
            .iter()
            .copied()
            .filter(|result| result.target.as_str() == target.as_str())
            .collect();
        let passed: usize = arm.iter().map(|result| result.passed).sum();
        let checks: usize = arm.iter().map(|result| result.checks_total).sum();
        let solved = arm.iter().filter(|result| result.solved()).count();
        let cost: f64 = arm.iter().map(|result| result.cost_usd).sum();
        let scores: Vec<f64> = arm.iter().map(|result| result.score).collect();
        let costs: Vec<f64> = arm
            .iter()
            .filter_map(|result| result.cost_per_check())
            .collect();
        let cost_solved = if solved > 0 && cost > 0.0 {
            format!("{:.0}", cost / solved as f64 * 1_000_000.0)
        } else {
            dash()
        };
        let _ = writeln!(
            text,
            "| {} | {} | {} | {}/{} ({}) | {} | {} | {cost_solved} | {} |",
            target,
            arm.len(),
            rate_text(passed, checks),
            solved,
            arm.len(),
            rate_text(solved, arm.len()),
            spread_at(&scores, 2),
            spread_at(&costs, 0),
            median_option(arm.iter().filter_map(|result| result.cache_share()))
                .map(percent)
                .unwrap_or_else(dash),
        );
    }

    let _ = writeln!(text, "\n## Work\n");
    let _ = writeln!(
        text,
        "| Arm | Model calls | Tool calls | Output | Wall (median) |"
    );
    let _ = writeln!(text, "| --- | --- | --- | --- | --- |");
    for target in &targets {
        let arm: Vec<&RunResult> = clean
            .iter()
            .copied()
            .filter(|result| result.target.as_str() == target.as_str())
            .collect();
        let _ = writeln!(
            text,
            "| {} | {} | {} | {} | {:.1}s |",
            target,
            median(arm.iter().map(|result| result.model_calls)),
            median(arm.iter().map(|result| result.tool_call_total)),
            human_bytes(median(arm.iter().map(|result| result.output_bytes))),
            median(arm.iter().map(|result| result.wall_ms as usize)) as f64 / 1000.0,
        );
    }

    let _ = writeln!(text, "\n## Adoption\n");
    let _ = writeln!(
        text,
        "| Arm | Runs | Plugin tools | Plugin calls/run | CLI (`at-*`) | CLI calls/run | Skill |"
    );
    let _ = writeln!(text, "| --- | --- | --- | --- | --- | --- | --- |");
    for target in &targets {
        let arm: Vec<&RunResult> = clean
            .iter()
            .copied()
            .filter(|result| result.target.as_str() == target.as_str())
            .collect();
        let cli_calls: Vec<f64> = arm
            .iter()
            .filter_map(|result| result.cli_calls().map(|count| count as f64))
            .collect();
        let cli_calls = if cli_calls.len() == arm.len() {
            median_option(cli_calls.iter().copied())
                .map(|value| format!("{value:.0}"))
                .unwrap_or_else(dash)
        } else {
            dash()
        };
        let _ = writeln!(
            text,
            "| {} | {} | {} | {} | {} | {cli_calls} | {} |",
            target,
            arm.len(),
            rate_of(&arm, |result| result.plugin_calls() > 0),
            median(arm.iter().map(|result| result.plugin_calls())),
            rate_of(&arm, |result| result.used_toolkit_cli),
            rate_of(&arm, |result| result.loaded_skill),
        );
    }

    let _ = writeln!(text, "\n## Score by scenario\n");
    write_matrix(&mut text, &targets);
    for scenario in &scenarios {
        let _ = write!(text, "| {scenario} |");
        for target in &targets {
            let scores: Vec<f64> = clean
                .iter()
                .filter(|result| {
                    result.scenario.as_str() == scenario && result.target.as_str() == target
                })
                .map(|result| result.score)
                .collect();
            let _ = write!(text, " {} |", spread_at(&scores, 2));
        }
        let _ = writeln!(text);
    }

    let _ = writeln!(text, "\n## Cost per check by scenario (µ$)\n");
    write_matrix(&mut text, &targets);
    for scenario in &scenarios {
        let _ = write!(text, "| {scenario} |");
        for target in &targets {
            let costs: Vec<f64> = clean
                .iter()
                .filter(|result| {
                    result.scenario.as_str() == scenario && result.target.as_str() == target
                })
                .filter_map(|result| result.cost_per_check())
                .collect();
            let _ = write!(text, " {} |", spread_at(&costs, 0));
        }
        let _ = writeln!(text);
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

    let _ = writeln!(text, "\n## Limits\n");
    let _ = writeln!(
        text,
        "- One model: `{model}`. A different model is a different experiment."
    );
    let _ = writeln!(
        text,
        "- `Cost/check` divides by partial credit; `Cost/solved` divides the arm's total bill by \
         runs that passed every check. They diverge when an arm trades completeness for a smaller \
         bill."
    );
    let runs_per_cell = runs_per_cell(&clean, &scenarios, &targets);
    let fewest = runs_per_cell.iter().copied().min().unwrap_or(0);
    let most = runs_per_cell.iter().copied().max().unwrap_or(0);
    if most <= 1 {
        let _ = writeln!(
            text,
            "- One run per scenario and arm: a smoke test, not a rate."
        );
    } else if fewest == most {
        let _ = writeln!(
            text,
            "- {} runs per scenario and arm; each cell is the median, with min–max where the runs disagree.",
            plural(most, "run")
        );
    } else {
        let _ = writeln!(
            text,
            "- {fewest}–{most} runs per scenario and arm; each cell is the median, with min–max where the runs disagree."
        );
    }
    let saturated = saturated_scenarios(&clean, &scenarios, &targets);
    if !saturated.is_empty() {
        let _ = writeln!(
            text,
            "- {} of {} scenarios scored 1.00 for every arm; they separate nothing here: {}.",
            saturated.len(),
            scenarios.len(),
            saturated.join(", ")
        );
    }
    if let Some(identity) = identity {
        if identity.checkout.dirty {
            let _ = writeln!(
                text,
                "- The checkout was dirty: a development run, not a publishable one."
            );
        }
        if identity.jobs > 1 {
            let _ = writeln!(
                text,
                "- {} jobs ran in parallel: wall time is not comparable across arms.",
                identity.jobs
            );
        }
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

/// The arms in the order the run declared them, so the ladder reads top to bottom; anything the
/// manifest does not know about is appended alphabetically.
fn arm_order(clean: &[&RunResult], identity: Option<&Manifest>) -> Vec<String> {
    let present: BTreeSet<String> = clean.iter().map(|result| result.target.clone()).collect();
    let mut targets = Vec::new();
    if let Some(identity) = identity {
        for arm in &identity.targets {
            if present.contains(&arm.label) && !targets.contains(&arm.label) {
                targets.push(arm.label.clone());
            }
        }
    }
    for target in present {
        if !targets.contains(&target) {
            targets.push(target);
        }
    }
    targets
}

/// How many runs each `(scenario, arm)` cell holds. An empty scenario × arm combination is zero.
fn runs_per_cell(clean: &[&RunResult], scenarios: &[String], targets: &[String]) -> Vec<usize> {
    let mut counts = Vec::new();
    for scenario in scenarios {
        for target in targets {
            counts.push(
                clean
                    .iter()
                    .filter(|result| {
                        result.scenario.as_str() == scenario.as_str()
                            && result.target.as_str() == target.as_str()
                    })
                    .count(),
            );
        }
    }
    counts
}

/// The scenarios every arm passed in full. These are the ones that measure nothing between arms.
fn saturated_scenarios(
    clean: &[&RunResult],
    scenarios: &[String],
    targets: &[String],
) -> Vec<String> {
    scenarios
        .iter()
        .filter(|scenario| {
            targets.iter().all(|target| {
                let scores: Vec<f64> = clean
                    .iter()
                    .filter(|result| {
                        result.scenario.as_str() == scenario.as_str()
                            && result.target.as_str() == target.as_str()
                    })
                    .map(|result| result.score)
                    .collect();
                !scores.is_empty() && scores.iter().all(|score| *score >= 0.9999)
            })
        })
        .cloned()
        .collect()
}

/// The distinct values of a column, in order.
fn distinct<'a>(values: impl Iterator<Item = &'a str>) -> Vec<String> {
    values
        .map(str::to_owned)
        .collect::<BTreeSet<String>>()
        .into_iter()
        .collect()
}

/// A scenario × arm matrix header.
fn write_matrix(text: &mut String, targets: &[String]) {
    let _ = write!(
        text,
        "| Scenario |{}",
        targets
            .iter()
            .map(|target| format!(" {target} |"))
            .collect::<String>()
    );
    let _ = write!(text, "\n| --- |{}", " --- |".repeat(targets.len()));
    let _ = writeln!(text);
}

fn rate_of(arm: &[&RunResult], flag: impl Fn(&RunResult) -> bool) -> String {
    if arm.is_empty() {
        return dash();
    }
    let hits = arm.iter().filter(|result| flag(result)).count();
    format!("{hits}/{} ({})", arm.len(), rate_text(hits, arm.len()))
}

fn rate_text(part: usize, total: usize) -> String {
    if total == 0 {
        dash()
    } else {
        format!("{:.0}%", 100.0 * part as f64 / total as f64)
    }
}

/// A median with its min–max when the runs disagree.
fn spread_at(values: &[f64], places: usize) -> String {
    let (Some(min), Some(max), Some(median)) = (
        values.iter().copied().reduce(f64::min),
        values.iter().copied().reduce(f64::max),
        median_option(values.iter().copied()),
    ) else {
        return dash();
    };
    if max > min {
        format!("{median:.places$} [{min:.places$}–{max:.places$}]")
    } else {
        format!("{median:.places$}")
    }
}

fn short(hash: &str) -> &str {
    hash.get(..7).unwrap_or(hash)
}

fn money(usd: f64) -> String {
    format!("${usd:.3}")
}

fn human_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.0}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

fn dash() -> String {
    "—".to_owned()
}

fn plural(count: usize, word: &str) -> String {
    if count == 1 {
        format!("{count} {word}")
    } else {
        format!("{count} {word}s")
    }
}

fn median(values: impl Iterator<Item = usize>) -> usize {
    let mut values: Vec<usize> = values.collect();
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2
    } else {
        values[mid]
    }
}

fn median_option(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut values: Vec<f64> = values.collect();
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        Some((values[mid - 1] + values[mid]) / 2.0)
    } else {
        Some(values[mid])
    }
}

fn percent(value: f64) -> String {
    format!("{value:.0}%")
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
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn bytes_are_human() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1536), "1.5 KB");
    }

    #[test]
    fn the_median_of_one_value_is_that_value() {
        assert_eq!(median([7].into_iter()), 7);
        assert_eq!(median_option([0.5].into_iter()), Some(0.5));
    }

    #[test]
    fn an_even_count_takes_the_middle_pair() {
        assert_eq!(median([1, 3].into_iter()), 2);
        assert_eq!(median_option([1.0, 3.0].into_iter()), Some(2.0));
    }

    #[test]
    fn cost_per_check_states_nothing_when_nothing_passed() {
        let mut run = result("pi", BTreeSet::new());
        run.cost_usd = 0.001;
        run.passed = 2;
        assert_eq!(run.cost_per_check(), Some(500.0));
        run.passed = 0;
        assert_eq!(run.cost_per_check(), None);
        run.passed = 2;
        run.cost_usd = 0.0;
        assert_eq!(
            run.cost_per_check(),
            None,
            "an unpriced run is not a free one"
        );
    }

    #[test]
    fn cache_share_is_the_cached_part_of_the_input() {
        let mut run = result("pi", BTreeSet::new());
        run.input_tokens = 100;
        run.cached_input_tokens = 70;
        assert_eq!(run.cache_share(), Some(70.0));
        run.input_tokens = 0;
        assert_eq!(run.cache_share(), None);
    }

    #[test]
    fn a_contaminated_run_is_not_in_the_tables() {
        let clean = result("pi", BTreeSet::new());
        let contaminated = result("rig:gated", BTreeSet::from([RunTag::Contaminated]));
        let text = summary(&[clean, contaminated], None);
        assert!(text.contains("| pi | 1 |"), "{text}");
        assert!(!text.contains("| rig:gated |"), "{text}");
        assert!(text.contains("1 contaminated run(s) are excluded"));
        assert!(text.contains("## Contaminated"));
    }

    #[test]
    fn the_report_has_a_matrix_and_an_adoption_row_per_arm() {
        let mut first = result("rig:baseline", BTreeSet::new());
        first.scenario = "alpha".to_owned();
        first.score = 1.0;
        first.passed = 2;
        first.checks_total = 2;
        first.cost_usd = 0.002;
        first.input_tokens = 100;
        first.cached_input_tokens = 90;
        first.used_plugin_tools = true;
        first.loaded_skill = true;
        first.tool_calls = BTreeMap::from([("at_peek_search".to_owned(), 2)]);

        let mut second = result("opencode", BTreeSet::new());
        second.scenario = "beta".to_owned();
        second.score = 0.5;
        second.passed = 1;
        second.checks_total = 2;
        second.cost_usd = 0.004;
        second.input_tokens = 100;
        second.cached_input_tokens = 40;
        second.used_toolkit_cli = true;
        second.toolkit_cli_calls = 1;

        let text = summary(&[first, second], None);

        assert!(text.contains("## Score by scenario"), "{text}");
        assert!(
            text.contains("| Scenario | opencode | rig:baseline |"),
            "{text}"
        );
        assert!(text.contains("| alpha | — | 1.00 |"), "{text}");
        assert!(text.contains("| beta | 0.50 | — |"), "{text}");
        assert!(
            text.contains("## Cost per check by scenario (µ$)"),
            "{text}"
        );
        assert!(text.contains("## Adoption"), "{text}");
        assert!(
            text.contains("| rig:baseline | 1 | 1/1 (100%) | 2 | 0/1 (0%) | 0 | 1/1 (100%) |"),
            "{text}"
        );
        assert!(
            text.contains("| opencode | 1 | 0/1 (0%) | 0 | 1/1 (100%) | 1 | 0/1 (0%) |"),
            "{text}"
        );
    }

    #[test]
    fn an_arm_table_shows_the_spread_when_runs_disagree() {
        let mut first = result("pi", BTreeSet::new());
        first.scenario = "alpha".to_owned();
        first.score = 1.0;
        first.passed = 2;
        first.checks_total = 2;
        first.cost_usd = 0.002;

        let mut second = result("pi", BTreeSet::new());
        second.scenario = "beta".to_owned();
        second.score = 0.5;
        second.passed = 1;
        second.checks_total = 2;
        second.cost_usd = 0.002;

        let text = summary(&[first, second], None);
        assert!(
            text.contains(
                "| pi | 2 | 75% | 1/2 (50%) | 0.75 [0.50–1.00] | 1500 [1000–2000] | 4000 | — |"
            ),
            "{text}"
        );
    }

    #[test]
    fn one_run_per_cell_is_called_a_smoke_test() {
        let text = summary(&[result("pi", BTreeSet::new())], None);
        assert!(
            text.contains("One run per scenario and arm: a smoke test"),
            "{text}"
        );
    }

    #[test]
    fn plugin_calls_count_the_at_tools_only() {
        let mut run = result("rig:baseline", BTreeSet::new());
        run.tool_calls = BTreeMap::from([
            ("at_peek_search".to_owned(), 2),
            ("at_recall_log".to_owned(), 1),
            ("read".to_owned(), 7),
            ("bash".to_owned(), 1),
        ]);
        assert_eq!(run.plugin_calls(), 3);
    }

    #[test]
    fn an_old_run_with_the_cli_flag_but_no_count_states_nothing() {
        let mut run = result("opencode", BTreeSet::new());
        run.used_toolkit_cli = true;
        assert_eq!(run.cli_calls(), None);
        run.toolkit_cli_calls = 2;
        assert_eq!(run.cli_calls(), Some(2));
    }

    #[test]
    fn an_arm_with_no_runnable_price_shows_a_dash() {
        let mut run = result("pi", BTreeSet::new());
        run.passed = 1;
        run.checks_total = 1;
        run.cost_usd = 0.0;
        let text = summary(&[run], None);
        assert!(
            text.contains("| pi | 1 | 100% | 1/1 (100%) | 1.00 | — | — | — |"),
            "{text}"
        );
    }

    #[test]
    fn a_saved_result_reads_back() {
        let directory =
            std::env::temp_dir().join(format!("rig-eval-read-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a directory");
        let result = result("pi", BTreeSet::new());
        std::fs::write(
            directory.join("results.json"),
            serde_json::to_string(&[result]).expect("json"),
        )
        .expect("write");
        let read = read(&directory).expect("read");
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].target, "pi");
        let _ = std::fs::remove_dir_all(&directory);
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
            cached_input_tokens: 0,
            cost_usd: 0.0,
            tool_calls: Default::default(),
            tool_call_total: 0,
            tool_results: 0,
            refused: 0,
            failed: 0,
            cancelled: 0,
            output_bytes: 0,
            used_plugin_tools: false,
            used_toolkit_cli: false,
            toolkit_cli_calls: 0,
            loaded_skill: false,
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
