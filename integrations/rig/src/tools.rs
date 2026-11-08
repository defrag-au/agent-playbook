//! The `at-*` verbs as native rig tools.
//!
//! Each tool takes typed arguments, derives its schema from them, declares that it only reads, and
//! runs the binary in the agent's own directory. The descriptions come from the verbs' own
//! catalogue, so the text the model reads and the grammar the parser enforces cannot drift.

use std::path::{Path, PathBuf};

use rig_harness::prelude::*;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::spawn;

/// Register every tool.
pub fn add(app: &mut App) {
    app.add_tool_per_call(Search::default(), options());
    app.add_tool_per_call(Slice::default(), options());
    app.add_tool_per_call(Stat::default(), options());
    app.add_tool_per_call(State::default(), options());
    app.add_tool_per_call(Log::default(), options());
    app.add_tool_per_call(Diff::default(), options());
    app.add_tool_per_call(Pr::default(), options());
}

/// A tool that only reads, whatever it is aimed at. Two reads of different paths can run side by
/// side; a read waits for any write that touches what it reads.
fn options() -> ToolOptions {
    ToolOptions {
        footprint: Some(Touches::Reads(Scope::Anything)),
    }
}

fn describe(verb: Option<&'static at_peek::catalogue::Verb>) -> String {
    match verb {
        Some(verb) => format!(
            "{}. {}. On the command line: {}",
            verb.question, verb.notes, verb.usage
        ),
        None => "A read-only inspection from the agent toolkit.".to_owned(),
    }
}

fn execute(binary: &str, argv: &[String]) -> Result<String, ToolExecutionError> {
    match spawn::run(binary, argv) {
        Ok(outcome) => finish(outcome),
        Err(error) => Err(ToolExecutionError::other(format!(
            "could not run {binary}: {error}"
        ))),
    }
}

/// The exit code is the answer: 1 is a successful read that found nothing, and a refusal is a
/// refusal rather than a generic failure.
fn finish(outcome: spawn::Outcome) -> Result<String, ToolExecutionError> {
    let spawn::Outcome {
        code,
        stdout,
        stderr,
    } = outcome;
    let message = if stderr.trim().is_empty() {
        stdout.clone()
    } else {
        stderr
    };

    match code {
        0 | 1 => Ok(stdout),
        2 => Err(ToolExecutionError::invalid_args(message)),
        3 => Err(ToolExecutionError::other(message)),
        4 => Err(ToolExecutionError::refused(message)),
        other => Err(ToolExecutionError::other(format!(
            "exited {other}: {message}"
        ))),
    }
}

fn push_value(argv: &mut Vec<String>, flag: &str, value: Option<String>) {
    if let Some(value) = value {
        argv.push(flag.to_owned());
        argv.push(value);
    }
}

fn push_switch(argv: &mut Vec<String>, flag: &str, on: bool) {
    if on {
        argv.push(flag.to_owned());
    }
}

/// The agent's directory, so a subagent reads its own tree rather than the process's.
fn push_root(argv: &mut Vec<String>, base: Option<&Path>) {
    if let Some(base) = base {
        push_value(argv, "--root", Some(base.display().to_string()));
    }
}

#[derive(Clone, Default)]
pub struct Search {
    base: Option<PathBuf>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchArgs {
    /// A regular expression, walked in path order.
    pub pattern: String,
    /// Paths to search; omit to search the whole root.
    #[serde(default)]
    pub paths: Vec<String>,
    /// Matches per file instead of the matching lines.
    #[serde(default)]
    pub count: bool,
    /// Paths containing a match, without lines.
    #[serde(default)]
    pub files_only: bool,
    /// The frame without the rows: how many, and where the walk stopped.
    #[serde(default)]
    pub summary: bool,
    /// Cap the output lines; default 200, ceiling 2000.
    #[serde(default)]
    pub limit: Option<u16>,
}

impl PerCallTool for Search {
    fn at(&self, at: CallPlace) -> Self {
        Self {
            base: Some(at.base),
        }
    }
}

impl Tool for Search {
    const NAME: &'static str = "at_peek_search";
    type Args = SearchArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        describe(at_peek::catalogue::find("search"))
    }

    fn parameters(&self) -> serde_json::Value {
        args_schema::<SearchArgs>()
    }

    async fn call(
        &self,
        _: &mut ToolContext,
        args: SearchArgs,
    ) -> Result<String, ToolExecutionError> {
        let mut argv = vec!["search".to_owned(), args.pattern];
        argv.extend(args.paths);
        push_switch(&mut argv, "--count", args.count);
        push_switch(&mut argv, "--files-only", args.files_only);
        push_switch(&mut argv, "--summary", args.summary);
        push_value(&mut argv, "--limit", args.limit.map(|n| n.to_string()));
        push_root(&mut argv, self.base.as_deref());

        blocking(move || execute("at-peek", &argv)).await
    }
}

#[derive(Clone, Default)]
pub struct Slice {
    base: Option<PathBuf>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SliceArgs {
    /// One or more `path:range` targets. Ranges are 1-based and inclusive: `40-60`, or `40+20`
    /// for twenty lines from 40. No range reads the whole file.
    pub targets: Vec<String>,
    /// Cap the output lines for the whole invocation; default 200, ceiling 2000.
    #[serde(default)]
    pub limit: Option<u16>,
}

impl PerCallTool for Slice {
    fn at(&self, at: CallPlace) -> Self {
        Self {
            base: Some(at.base),
        }
    }
}

impl Tool for Slice {
    const NAME: &'static str = "at_peek_slice";
    type Args = SliceArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        describe(at_peek::catalogue::find("slice"))
    }

    fn parameters(&self) -> serde_json::Value {
        args_schema::<SliceArgs>()
    }

    async fn call(
        &self,
        _: &mut ToolContext,
        args: SliceArgs,
    ) -> Result<String, ToolExecutionError> {
        let mut argv = vec!["slice".to_owned()];
        argv.extend(args.targets);
        push_value(&mut argv, "--limit", args.limit.map(|n| n.to_string()));
        push_root(&mut argv, self.base.as_deref());

        blocking(move || execute("at-peek", &argv)).await
    }
}

#[derive(Clone, Default)]
pub struct Stat {
    base: Option<PathBuf>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatArgs {
    /// Files or directories to describe.
    pub paths: Vec<String>,
    /// Cap the output lines; default 200, ceiling 2000.
    #[serde(default)]
    pub limit: Option<u16>,
}

impl PerCallTool for Stat {
    fn at(&self, at: CallPlace) -> Self {
        Self {
            base: Some(at.base),
        }
    }
}

impl Tool for Stat {
    const NAME: &'static str = "at_peek_stat";
    type Args = StatArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        describe(at_peek::catalogue::find("stat"))
    }

    fn parameters(&self) -> serde_json::Value {
        args_schema::<StatArgs>()
    }

    async fn call(
        &self,
        _: &mut ToolContext,
        args: StatArgs,
    ) -> Result<String, ToolExecutionError> {
        let mut argv = vec!["stat".to_owned()];
        argv.extend(args.paths);
        push_value(&mut argv, "--limit", args.limit.map(|n| n.to_string()));
        push_root(&mut argv, self.base.as_deref());

        blocking(move || execute("at-peek", &argv)).await
    }
}

#[derive(Clone, Default)]
pub struct State {
    base: Option<PathBuf>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StateArgs {
    /// The frame without the changed paths.
    #[serde(default)]
    pub summary: bool,
    /// Cap the changed paths; default 200, ceiling 2000.
    #[serde(default)]
    pub limit: Option<u16>,
}

impl PerCallTool for State {
    fn at(&self, at: CallPlace) -> Self {
        Self {
            base: Some(at.base),
        }
    }
}

impl Tool for State {
    const NAME: &'static str = "at_recall_state";
    type Args = StateArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        describe(at_recall::catalogue::find("state"))
    }

    fn parameters(&self) -> serde_json::Value {
        args_schema::<StateArgs>()
    }

    async fn call(
        &self,
        _: &mut ToolContext,
        args: StateArgs,
    ) -> Result<String, ToolExecutionError> {
        let mut argv = vec!["state".to_owned()];
        push_switch(&mut argv, "--summary", args.summary);
        push_value(&mut argv, "--limit", args.limit.map(|n| n.to_string()));
        push_root(&mut argv, self.base.as_deref());

        blocking(move || execute("at-recall", &argv)).await
    }
}

#[derive(Clone, Default)]
pub struct Log {
    base: Option<PathBuf>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LogArgs {
    /// The revision to walk; default HEAD.
    #[serde(default)]
    pub rev: Option<String>,
    /// A path filter, so `log` on a path is that file's history.
    #[serde(default)]
    pub paths: Vec<String>,
    /// Cap the commits listed; default 200, ceiling 2000.
    #[serde(default)]
    pub limit: Option<u16>,
}

impl PerCallTool for Log {
    fn at(&self, at: CallPlace) -> Self {
        Self {
            base: Some(at.base),
        }
    }
}

impl Tool for Log {
    const NAME: &'static str = "at_recall_log";
    type Args = LogArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        describe(at_recall::catalogue::find("log"))
    }

    fn parameters(&self) -> serde_json::Value {
        args_schema::<LogArgs>()
    }

    async fn call(&self, _: &mut ToolContext, args: LogArgs) -> Result<String, ToolExecutionError> {
        let mut argv = vec!["log".to_owned()];
        argv.extend(args.rev);
        argv.extend(args.paths);
        push_value(&mut argv, "--limit", args.limit.map(|n| n.to_string()));
        push_root(&mut argv, self.base.as_deref());

        blocking(move || execute("at-recall", &argv)).await
    }
}

#[derive(Clone, Default)]
pub struct Diff {
    base: Option<PathBuf>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiffArgs {
    /// The revision to compare against; default HEAD, so the working tree against it.
    #[serde(default)]
    pub rev: Option<String>,
    /// Paths to filter the difference to.
    #[serde(default)]
    pub paths: Vec<String>,
    /// The hunks themselves, not only the per-file totals.
    #[serde(default)]
    pub patch: bool,
    /// Compare lines ignoring whitespace, and state what that hid.
    #[serde(default)]
    pub ignore_space: bool,
    /// The frame without the per-file table.
    #[serde(default)]
    pub summary: bool,
    /// Cap the output lines; default 200, ceiling 2000.
    #[serde(default)]
    pub limit: Option<u16>,
}

impl PerCallTool for Diff {
    fn at(&self, at: CallPlace) -> Self {
        Self {
            base: Some(at.base),
        }
    }
}

impl Tool for Diff {
    const NAME: &'static str = "at_recall_diff";
    type Args = DiffArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        describe(at_recall::catalogue::find("diff"))
    }

    fn parameters(&self) -> serde_json::Value {
        args_schema::<DiffArgs>()
    }

    async fn call(
        &self,
        _: &mut ToolContext,
        args: DiffArgs,
    ) -> Result<String, ToolExecutionError> {
        let mut argv = vec!["diff".to_owned()];
        argv.extend(args.rev);
        argv.extend(args.paths);
        push_switch(&mut argv, "--patch", args.patch);
        push_switch(&mut argv, "--ignore-space", args.ignore_space);
        push_switch(&mut argv, "--summary", args.summary);
        push_value(&mut argv, "--limit", args.limit.map(|n| n.to_string()));
        push_root(&mut argv, self.base.as_deref());

        blocking(move || execute("at-recall", &argv)).await
    }
}

#[derive(Clone, Default)]
pub struct Pr {
    base: Option<PathBuf>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PrArgs {
    /// What the branch diverged from; default is its upstream, else origin/HEAD. Nothing is
    /// fetched.
    #[serde(default)]
    pub base: Option<String>,
    /// Sections to print: commits, diffstat, areas. Default is all of them.
    #[serde(default)]
    pub with: Vec<String>,
    /// Cap each section's rows; default 200, ceiling 2000.
    #[serde(default)]
    pub limit: Option<u16>,
}

impl PerCallTool for Pr {
    fn at(&self, at: CallPlace) -> Self {
        Self {
            base: Some(at.base),
        }
    }
}

impl Tool for Pr {
    const NAME: &'static str = "at_recall_pr";
    type Args = PrArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        describe(at_recall::catalogue::find("pr"))
    }

    fn parameters(&self) -> serde_json::Value {
        args_schema::<PrArgs>()
    }

    async fn call(&self, _: &mut ToolContext, args: PrArgs) -> Result<String, ToolExecutionError> {
        let mut argv = vec!["pr".to_owned()];
        push_value(&mut argv, "--base", args.base);
        if !args.with.is_empty() {
            push_value(&mut argv, "--with", Some(args.with.join(",")));
        }
        push_value(&mut argv, "--limit", args.limit.map(|n| n.to_string()));
        push_root(&mut argv, self.base.as_deref());

        blocking(move || execute("at-recall", &argv)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_names_a_catalogued_verb() {
        for (name, verb) in [
            (Search::NAME, at_peek::catalogue::find("search")),
            (Slice::NAME, at_peek::catalogue::find("slice")),
            (Stat::NAME, at_peek::catalogue::find("stat")),
            (State::NAME, at_recall::catalogue::find("state")),
            (Log::NAME, at_recall::catalogue::find("log")),
            (Diff::NAME, at_recall::catalogue::find("diff")),
            (Pr::NAME, at_recall::catalogue::find("pr")),
        ] {
            assert!(verb.is_some(), "`{name}` names no catalogued verb");
        }
    }

    #[test]
    fn a_search_argv_carries_the_verb_and_the_root() {
        let args = SearchArgs {
            pattern: "render_claim".to_owned(),
            paths: vec!["src".to_owned()],
            count: true,
            files_only: false,
            summary: false,
            limit: Some(10),
        };
        let mut argv = vec!["search".to_owned(), args.pattern];
        argv.extend(args.paths);
        push_switch(&mut argv, "--count", args.count);
        push_value(&mut argv, "--limit", args.limit.map(|n| n.to_string()));
        push_root(&mut argv, Some(Path::new("/tmp/project")));

        assert_eq!(
            argv,
            [
                "search",
                "render_claim",
                "src",
                "--count",
                "--limit",
                "10",
                "--root",
                "/tmp/project",
            ]
        );
    }
}
