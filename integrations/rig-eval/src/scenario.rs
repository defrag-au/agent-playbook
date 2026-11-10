//! The scenario corpus: what a task is, and the checks it is scored against.

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::cli::Args;

/// A check the working tree answers, rather than one written into the corpus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GitExpectation {
    /// The current branch and every changed path, from `git status`.
    Status,
}

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
    /// Expectations read from the working tree at run time.
    #[serde(default)]
    pub git: Option<GitExpectation>,
    /// The branch `git: status` found, resolved from the workdir.
    #[serde(skip)]
    pub git_branch: Option<String>,
    /// The changed paths `git: status` found, resolved from the snapshot.
    #[serde(skip)]
    pub git_paths: Vec<String>,
}

impl Scenario {
    pub fn checks(&self) -> usize {
        self.gold.len()
            + self.extra_paths.len()
            + self.git_paths.len()
            + usize::from(self.git_branch.is_some())
            + self.must_say.len()
            + self.tool_says.len()
    }

    /// Fill the fields the corpus cannot know: the state of the snapshot it runs against.
    fn resolve(&mut self, snapshot: &Path) -> Result<()> {
        let Some(GitExpectation::Status) = self.git else {
            return Ok(());
        };
        let workdir = snapshot.join(&self.workdir);
        let output = Command::new("git")
            .arg("-C")
            .arg(&workdir)
            .args(["status", "--porcelain=v1", "--branch", "-uall"])
            .output()
            .with_context(|| {
                format!("{}: running git status in {}", self.name, workdir.display())
            })?;
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            bail!("{}: git status failed: {}", self.name, error.trim());
        }
        let text = String::from_utf8(output.stdout)
            .with_context(|| format!("{}: git status output is not UTF-8", self.name))?;
        let (branch, paths) = parse_status(&text);
        self.git_branch = branch;
        self.git_paths = paths;
        Ok(())
    }
}

pub fn load(args: &Args, snapshot: &Path) -> Result<Vec<Scenario>> {
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
        let mut scenario: Scenario =
            serde_json::from_str(&text).with_context(|| format!("parsing {}", file.display()))?;
        // Select before resolving: a scenario that needs a dirty worktree must not stop a run of
        // the scenarios that do not.
        if !args.only.is_empty() && !args.only.contains(&scenario.name) {
            continue;
        }
        scenario.resolve(snapshot)?;
        validate(&scenario, snapshot)?;
        scenarios.push(scenario);
    }

    if !args.only.is_empty() {
        for name in &args.only {
            if !scenarios.iter().any(|scenario| &scenario.name == name) {
                bail!("no scenario named `{name}`");
            }
        }
    }

    Ok(scenarios)
}

/// The status lines, reduced to the branch and the paths outside the sealed tree.
fn parse_status(text: &str) -> (Option<String>, Vec<String>) {
    let mut branch = None;
    let mut paths = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            let name = rest.split("...").next().unwrap_or(rest).trim();
            branch = Some(name.to_owned());
            continue;
        }
        if line.len() < 4 {
            continue;
        }
        let path = &line[3..];
        let path = path.rsplit_once(" -> ").map(|(_, new)| new).unwrap_or(path);
        paths.push(path.trim_matches('"').to_owned());
    }
    (branch, paths)
}

/// A gold citation must still name a line that exists; stale gold is an error, not a model miss.
fn validate(scenario: &Scenario, snapshot: &Path) -> Result<()> {
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
    if scenario.git == Some(GitExpectation::Status) && scenario.git_paths.is_empty() {
        bail!(
            "{}: `git: status` finds no changed paths; the snapshot has no uncommitted change",
            scenario.name
        );
    }

    let workdir = snapshot.join(&scenario.workdir);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_line_list_carries_the_branch_and_paths() {
        let (branch, paths) = parse_status(
            "## damon-mcp...origin/damon-mcp\n M docs/design/rig-eval.md\n?? src/new.rs\n",
        );
        assert_eq!(branch.as_deref(), Some("damon-mcp"));
        assert_eq!(paths, ["docs/design/rig-eval.md", "src/new.rs"]);
    }

    #[test]
    fn a_rename_keeps_the_new_path() {
        let (_, paths) = parse_status("R  src/old.rs -> src/new.rs\n");
        assert_eq!(paths, ["src/new.rs"]);
    }

    #[test]
    fn a_quoted_path_is_unquoted() {
        let (_, paths) = parse_status("?? \"a path/with space.rs\"\n");
        assert_eq!(paths, ["a path/with space.rs"]);
    }
}
