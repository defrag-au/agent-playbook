//! The `at-*` verbs as native rig tools.
//!
//! Each tool takes typed arguments, derives its schema from them, declares that it only reads, and
//! calls the verb's `collect` in process in the agent's own directory. The descriptions come from
//! the verbs' own catalogue, so the text the model reads and the grammar the parser enforces cannot
//! drift.
//!
//! Every tool publishes its typed answer under `agent_playbook.*`, so an effect log records what a
//! call found rather than only the string the model was shown. The CLI renders the same facts; a
//! tool and the binary cannot answer differently.

use std::path::{Path, PathBuf};

use at_peek::paths::Root;
use at_peek::verbs::search::{self, Mode, SearchFacts};
use at_peek::verbs::slice::{self as slice_verb, SliceFacts};
use at_peek::verbs::stat::{self as stat_verb, StatFacts};
use at_peek::verbs::Opts as PeekOpts;
use at_recall::verbs::diff::{self as diff_verb, DiffFacts};
use at_recall::verbs::log::{self as log_verb, LogFacts};
use at_recall::verbs::pr::{self as pr_verb, PrFacts};
use at_recall::verbs::state::{self as state_verb, StateFacts};
use at_recall::verbs::{Config as RecallConfig, Opts as RecallOpts};
use rig_harness::prelude::*;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::fail;

/// Which read tools a plugin registers.
#[derive(Debug, Clone, Copy)]
pub struct Toolset {
    /// How `at_peek_stat` answers, or `None` to leave it out.
    pub stat: Option<StatStyle>,
    /// Whether `at_recall_log` is registered.
    pub log: bool,
}

impl Default for Toolset {
    fn default() -> Self {
        Self {
            stat: Some(StatStyle::Report),
            log: true,
        }
    }
}

impl Toolset {
    /// The full set with the two least-used read tools left out.
    pub const fn lean() -> Self {
        Self {
            stat: None,
            log: false,
        }
    }
}

/// Register every tool, with `at_peek_stat` answering as the CLI does.
pub fn add(app: &mut App) {
    add_with(app, Toolset::default());
}

/// Register the read tools a [`Toolset`] names.
///
/// The arms that vary the set register a different [`Toolset`]; every tool either arm registers is
/// identical.
pub fn add_with(app: &mut App, tools: Toolset) {
    app.add_tool_per_call(Search::default(), options());
    app.add_tool_per_call(Slice::default(), options());
    if let Some(style) = tools.stat {
        app.add_tool_per_call(
            Stat {
                style,
                ..Stat::default()
            },
            options(),
        );
    }
    app.add_tool_per_call(State::default(), options());
    if tools.log {
        app.add_tool_per_call(Log::default(), options());
    }
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

/// Publish a typed answer with the call's result, under the key its `ContextValue` names.
fn publish<T: rig_harness::rig_core::tool::ContextValue>(
    context: &mut ToolContext,
    value: T,
) -> Result<(), ToolExecutionError> {
    context.insert_result(value).map(|_| ()).map_err(|error| {
        ToolExecutionError::other(format!("could not publish {}: {error}", T::KEY))
    })
}

/// The agent's directory, so a subagent reads its own tree rather than the process's.
fn peek_root(base: Option<&Path>) -> Result<Root, ToolExecutionError> {
    match base {
        Some(base) => Root::at(base).map_err(fail),
        None => {
            let cwd = std::env::current_dir().map_err(|error| {
                ToolExecutionError::other(format!("cannot read the working directory: {error}"))
            })?;
            Root::discover(&cwd).map_err(fail)
        }
    }
}

/// The options a working-tree verb runs with. One constructor means `--limit` cannot mean two
/// things: the CLI's clamp is the harness's clamp.
fn peek_opts(
    base: Option<&Path>,
    limit: Option<u16>,
    summary: bool,
) -> Result<PeekOpts, ToolExecutionError> {
    let root = peek_root(base)?;
    PeekOpts::resolve(
        root,
        base.is_some(),
        limit.map(usize::from),
        None,
        false,
        summary,
    )
    .map_err(fail)
}

/// The options a history verb runs with, resolved through git so a subdirectory root becomes the
/// top of the worktree.
fn recall_opts(
    base: Option<&Path>,
    config: RecallConfig,
) -> Result<RecallOpts, ToolExecutionError> {
    RecallOpts::resolve(base, config).map_err(fail)
}

/// A revision, then paths — the rule git uses, expressed once so every history tool passes its
/// positionals in the same order.
fn positionals(rev: Option<String>, paths: Vec<String>) -> Vec<String> {
    rev.into_iter().chain(paths).collect()
}

/// The search answer, under the key the effect log reads it by.
///
/// A newtype rather than `impl ContextValue for SearchFacts`: the trait is rig's, and the tool
/// crate must not depend on rig to be publishable.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PublishedSearch(pub SearchFacts);

impl rig_harness::rig_core::tool::ContextValue for PublishedSearch {
    const KEY: &'static str = "agent_playbook.search";
}

/// The slice answer: one target's lines and how much of the file it is.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PublishedSlice(pub SliceFacts);

impl rig_harness::rig_core::tool::ContextValue for PublishedSlice {
    const KEY: &'static str = "agent_playbook.slice";
}

/// The stat answer: size, line count and modification time per path.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PublishedStat(pub StatFacts);

impl rig_harness::rig_core::tool::ContextValue for PublishedStat {
    const KEY: &'static str = "agent_playbook.stat";
}

/// The state answer: branch, HEAD, pending operation and changed paths.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PublishedState(pub StateFacts);

impl rig_harness::rig_core::tool::ContextValue for PublishedState {
    const KEY: &'static str = "agent_playbook.state";
}

/// The log answer: the commits and their true total.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PublishedLog(pub LogFacts);

impl rig_harness::rig_core::tool::ContextValue for PublishedLog {
    const KEY: &'static str = "agent_playbook.log";
}

/// The diff answer: the per-file counts, and the hunks when they were asked for.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PublishedDiff(pub DiffFacts);

impl rig_harness::rig_core::tool::ContextValue for PublishedDiff {
    const KEY: &'static str = "agent_playbook.diff";
}

/// The PR answer: base, commits, diffstat and the mechanical area breakdown.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PublishedPr(pub PrFacts);

impl rig_harness::rig_core::tool::ContextValue for PublishedPr {
    const KEY: &'static str = "agent_playbook.pr";
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

/// The read, in process. `collect` returns facts and `report` renders them, so the text is the
/// CLI's own bytes rather than a second implementation of them.
fn search_in_process(
    base: Option<&Path>,
    args: SearchArgs,
) -> Result<(SearchFacts, PeekOpts), ToolExecutionError> {
    let mode = Mode::of(args.count, args.files_only).map_err(fail)?;
    let opts = peek_opts(base, args.limit, args.summary)?;

    let mut targets = vec![args.pattern];
    targets.extend(args.paths);
    let facts = search::collect(&targets, &opts, mode).map_err(fail)?;
    Ok((facts, opts))
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
        context: &mut ToolContext,
        args: SearchArgs,
    ) -> Result<String, ToolExecutionError> {
        let base = self.base.clone();
        let (facts, opts) = blocking(move || search_in_process(base.as_deref(), args)).await?;
        let text = facts.report(&opts).render();
        publish(context, PublishedSearch(facts))?;
        Ok(text)
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

fn slice_in_process(
    base: Option<&Path>,
    args: SliceArgs,
) -> Result<(SliceFacts, PeekOpts), ToolExecutionError> {
    let opts = peek_opts(base, args.limit, false)?;
    let facts = slice_verb::collect(&args.targets, &opts).map_err(fail)?;
    Ok((facts, opts))
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
        context: &mut ToolContext,
        args: SliceArgs,
    ) -> Result<String, ToolExecutionError> {
        let base = self.base.clone();
        let (facts, opts) = blocking(move || slice_in_process(base.as_deref(), args)).await?;
        let text = facts.report(&opts).report.render();
        publish(context, PublishedSlice(facts))?;
        Ok(text)
    }
}

/// How `at_peek_stat` answers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StatStyle {
    /// The CLI's report: header, rows, and a bound.
    #[default]
    Report,
    /// The rows alone, without the header or the bound.
    Content,
}

#[derive(Clone, Default)]
pub struct Stat {
    base: Option<PathBuf>,
    style: StatStyle,
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

fn stat_in_process(
    base: Option<&Path>,
    args: StatArgs,
) -> Result<(StatFacts, PeekOpts), ToolExecutionError> {
    let opts = peek_opts(base, args.limit, false)?;
    let facts = stat_verb::collect(&args.paths, &opts).map_err(fail)?;
    Ok((facts, opts))
}

impl PerCallTool for Stat {
    fn at(&self, at: CallPlace) -> Self {
        Self {
            base: Some(at.base),
            style: self.style,
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
        context: &mut ToolContext,
        args: StatArgs,
    ) -> Result<String, ToolExecutionError> {
        let base = self.base.clone();
        let style = self.style;
        let (facts, opts) = blocking(move || stat_in_process(base.as_deref(), args)).await?;
        let outcome = facts.report(&opts);
        let text = match style {
            StatStyle::Report => outcome.report.render(),
            StatStyle::Content => outcome.report.content_text(),
        };
        publish(context, PublishedStat(facts))?;
        Ok(text)
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

fn state_in_process(
    base: Option<&Path>,
    args: StateArgs,
) -> Result<(StateFacts, RecallOpts), ToolExecutionError> {
    let opts = recall_opts(
        base,
        RecallConfig {
            limit: args.limit.map(usize::from),
            summary: args.summary,
            ..RecallConfig::default()
        },
    )?;
    let facts = state_verb::collect(&opts).map_err(fail)?;
    Ok((facts, opts))
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
        context: &mut ToolContext,
        args: StateArgs,
    ) -> Result<String, ToolExecutionError> {
        let base = self.base.clone();
        let (facts, opts) = blocking(move || state_in_process(base.as_deref(), args)).await?;
        let text = facts.report(&opts).report.render();
        publish(context, PublishedState(facts))?;
        Ok(text)
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

fn log_in_process(
    base: Option<&Path>,
    args: LogArgs,
) -> Result<(LogFacts, RecallOpts), ToolExecutionError> {
    let opts = recall_opts(
        base,
        RecallConfig {
            limit: args.limit.map(usize::from),
            ..RecallConfig::default()
        },
    )?;
    let facts = log_verb::collect(&positionals(args.rev, args.paths), &opts).map_err(fail)?;
    Ok((facts, opts))
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

    async fn call(
        &self,
        context: &mut ToolContext,
        args: LogArgs,
    ) -> Result<String, ToolExecutionError> {
        let base = self.base.clone();
        let (facts, opts) = blocking(move || log_in_process(base.as_deref(), args)).await?;
        let text = facts.report(&opts).report.render();
        publish(context, PublishedLog(facts))?;
        Ok(text)
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

fn diff_in_process(
    base: Option<&Path>,
    args: DiffArgs,
) -> Result<(DiffFacts, RecallOpts), ToolExecutionError> {
    let opts = recall_opts(
        base,
        RecallConfig {
            limit: args.limit.map(usize::from),
            patch: args.patch,
            summary: args.summary,
            ignore_space: args.ignore_space,
            ..RecallConfig::default()
        },
    )?;
    let facts = diff_verb::collect(&positionals(args.rev, args.paths), &opts).map_err(fail)?;
    Ok((facts, opts))
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
        context: &mut ToolContext,
        args: DiffArgs,
    ) -> Result<String, ToolExecutionError> {
        let base = self.base.clone();
        let (facts, opts) = blocking(move || diff_in_process(base.as_deref(), args)).await?;
        let text = facts.report(&opts).report.render();
        publish(context, PublishedDiff(facts))?;
        Ok(text)
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

fn pr_in_process(
    base: Option<&Path>,
    args: PrArgs,
) -> Result<(PrFacts, RecallOpts), ToolExecutionError> {
    let opts = recall_opts(
        base,
        RecallConfig {
            limit: args.limit.map(usize::from),
            base: args.base,
            with: args.with,
            ..RecallConfig::default()
        },
    )?;
    let facts = pr_verb::collect(&opts).map_err(fail)?;
    Ok((facts, opts))
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

    async fn call(
        &self,
        context: &mut ToolContext,
        args: PrArgs,
    ) -> Result<String, ToolExecutionError> {
        let base = self.base.clone();
        let (facts, opts) = blocking(move || pr_in_process(base.as_deref(), args)).await?;
        let text = facts.report(&opts).report.render();
        publish(context, PublishedPr(facts))?;
        Ok(text)
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
    fn a_revision_comes_before_the_paths() {
        assert_eq!(
            positionals(Some("main".to_owned()), vec!["src".to_owned()]),
            ["main", "src"]
        );
        assert_eq!(positionals(None, vec!["src".to_owned()]), ["src"]);
        assert_eq!(positionals(None, Vec::new()), Vec::<String>::new());
    }

    #[test]
    fn the_search_tool_reads_in_process_and_renders_the_cli_answer() {
        let dir = std::env::temp_dir().join(format!("rig-playbook-search-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).expect("fixture src");
        std::fs::write(dir.join("src/a.rs"), "fn alpha() {}\n").expect("fixture file");

        let args = SearchArgs {
            pattern: "alpha".to_owned(),
            paths: Vec::new(),
            count: false,
            files_only: false,
            summary: false,
            limit: None,
        };
        let (facts, opts) = search_in_process(Some(&dir), args).expect("a search");
        let text = facts.report(&opts).render();

        assert_eq!(facts.total, 1);
        assert!(text.contains("src/a.rs:1:fn alpha() {}"), "{text}");
        assert!(text.contains("# 1 match in 1 file"), "{text}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_slice_tool_reads_in_process_and_publishes_its_targets() {
        let dir = std::env::temp_dir().join(format!("rig-playbook-slice-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("fixture dir");
        std::fs::write(dir.join("a.rs"), "one\ntwo\nthree\n").expect("fixture file");

        let args = SliceArgs {
            targets: vec!["a.rs:2-3".to_owned()],
            limit: None,
        };
        let (facts, opts) = slice_in_process(Some(&dir), args).expect("a slice");
        let text = facts.report(&opts).report.render();

        assert_eq!(facts.targets.len(), 1);
        assert_eq!(facts.targets[0].shown, 2);
        assert!(text.contains("2  two"), "{text}");
        assert!(text.contains("# 2 of 3 lines"), "{text}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_history_tools_read_in_process() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let state = state_in_process(
            Some(dir),
            StateArgs {
                summary: true,
                limit: Some(5),
            },
        )
        .expect("a state read");
        assert!(!state.0.branch.is_empty());
        assert!(state.0.pending.contains(&"none".to_owned()) || !state.0.pending.is_empty());
    }
}
