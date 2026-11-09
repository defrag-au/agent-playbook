//! The scenario corpus: what a task is, and the checks it is scored against.

use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::cli::Args;

#[derive(Debug, Deserialize)]
pub struct Scenario {
    pub name: String,
    #[serde(default)]
    pub why: String,
    pub prompt: String,
    #[serde(default = "current_dir")]
    pub workdir: String,
    /// `path:line` citations the answer must contain.
    #[serde(default)]
    pub gold: Vec<String>,
    /// Paths the answer must name, where an exact line is not required.
    #[serde(default)]
    pub extra_paths: Vec<String>,
    /// Literal strings the answer must contain.
    #[serde(default)]
    pub must_say: Vec<String>,
    /// Literal strings the tool output must contain: evidence the work happened.
    #[serde(default)]
    pub tool_says: Vec<String>,
}

impl Scenario {
    pub fn checks(&self) -> usize {
        self.gold.len() + self.extra_paths.len() + self.must_say.len() + self.tool_says.len()
    }
}

pub fn load(args: &Args) -> Result<Vec<Scenario>> {
    let mut files: Vec<_> = fs::read_dir(&args.scenario_dir)
        .with_context(|| format!("reading {}", args.scenario_dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    files.sort();

    let mut scenarios = Vec::new();
    for file in files {
        let text =
            fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
        let scenario: Scenario =
            serde_json::from_str(&text).with_context(|| format!("parsing {}", file.display()))?;
        validate(&scenario, &args.root)?;
        scenarios.push(scenario);
    }

    if !args.only.is_empty() {
        for name in &args.only {
            if !scenarios.iter().any(|scenario| &scenario.name == name) {
                bail!("no scenario named `{name}`");
            }
        }
        scenarios.retain(|scenario| args.only.contains(&scenario.name));
    }

    Ok(scenarios)
}

/// A gold citation must still name a line that exists; stale gold is an error, not a model miss.
fn validate(scenario: &Scenario, root: &Path) -> Result<()> {
    if scenario.name.trim().is_empty() {
        bail!("a scenario has an empty `name`");
    }
    if scenario.prompt.trim().is_empty() {
        bail!("{}: `prompt` is empty", scenario.name);
    }
    if scenario.checks() == 0 {
        bail!(
            "{}: no gold, paths, `must_say` or `tool_says` — nothing to score",
            scenario.name
        );
    }

    let workdir = root.join(&scenario.workdir);
    if !workdir.is_dir() {
        bail!(
            "{}: workdir {} is not a directory",
            scenario.name,
            workdir.display()
        );
    }

    for cite in &scenario.gold {
        let (path, line) = cite
            .rsplit_once(':')
            .with_context(|| format!("{}: gold `{cite}` is not `path:line`", scenario.name))?;
        let number: usize = line
            .parse()
            .with_context(|| format!("{}: gold `{cite}` has no line number", scenario.name))?;
        let file = workdir.join(path);
        let text = fs::read_to_string(&file)
            .with_context(|| format!("{}: gold `{cite}`", scenario.name))?;
        let lines = text.lines().count();
        if number == 0 || number > lines {
            bail!(
                "{}: gold `{cite}` is past the end of {path} ({lines} lines) — the corpus is stale",
                scenario.name
            );
        }
    }

    for path in &scenario.extra_paths {
        if !workdir.join(path).exists() {
            bail!(
                "{}: path `{path}` does not exist — the corpus is stale",
                scenario.name
            );
        }
    }

    Ok(())
}

fn current_dir() -> String {
    ".".to_owned()
}
