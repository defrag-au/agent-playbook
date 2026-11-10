//! Answer a shell read with the toolkit, without a refusal round-trip.
//!
//! A Gate that refuses costs the model a call: it has to read the refusal and ask again. This
//! module replaces the call's handler in `Prepare` instead, so the shell command the model already
//! knows is served by the verb that answers the same question — with a bound, a true total and the
//! continuation the shell command cannot state.
//!
//! Only the shapes that map without guessing are redirected. `rg`/`grep` become a search; plain
//! `cat`, `sed -n` and `head` become a slice; plain `git status`, `git log` and `git diff` become
//! the history verbs. A pipeline, a redirection, an unknown flag, or a command with a second
//! positional is left alone, and the policy or the Gate sees it as it was written. A wrong
//! substitution is worse than no substitution.

use std::path::{Path, PathBuf};

use rig_harness::prelude::*;
use rig_harness::rig_core::serve::{adapters::ToolAdapter, ErasedHandler};
use rig_harness::rig_core::tool::{Scope, Touches};
use rig_harness::rig_ecs::effects::Handler;
use serde::Deserialize;

use at_peek::paths::Root;
use at_peek::verbs::search::{self, Mode, SearchFacts};
use at_peek::verbs::slice::{self as slice_verb, SliceFacts};
use at_peek::verbs::Opts as PeekOpts;
use at_recall::verbs::diff::{self as diff_verb, DiffFacts};
use at_recall::verbs::log::{self as log_verb, LogFacts};
use at_recall::verbs::state::{self as state_verb, StateFacts};
use at_recall::verbs::{Config as RecallConfig, Opts as RecallOpts};

use crate::fail;
use crate::tools::{
    publish, PublishedDiff, PublishedLog, PublishedSearch, PublishedSlice, PublishedState,
};

/// What the log records about a call the playbook answered with another tool.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Redirection {
    /// The command the model asked for.
    pub command: String,
    /// The tool that answered it.
    pub tool: String,
}

impl rig_harness::rig_core::tool::ContextValue for Redirection {
    const KEY: &'static str = "agent_playbook.redirection";
}

/// A shell read this module can serve without guessing.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Redirect {
    Search {
        pattern: String,
        paths: Vec<String>,
    },
    Slice {
        targets: Vec<String>,
    },
    State,
    Log {
        args: Vec<String>,
        limit: Option<usize>,
    },
    Diff {
        args: Vec<String>,
        patch: bool,
        ignore_space: bool,
    },
    /// Several plain reads separated by `;`, served in order and answered as one result.
    Compound(Vec<Redirect>),
}

impl Redirect {
    fn tool(&self) -> String {
        match self {
            Self::Search { .. } => "at_peek_search".to_owned(),
            Self::Slice { .. } => "at_peek_slice".to_owned(),
            Self::State => "at_recall_state".to_owned(),
            Self::Log { .. } => "at_recall_log".to_owned(),
            Self::Diff { .. } => "at_recall_diff".to_owned(),
            Self::Compound(redirects) => redirects
                .iter()
                .map(Redirect::tool)
                .collect::<Vec<_>>()
                .join(" + "),
        }
    }
}

/// Replace a `shell` call that is a plain read with the in-process one.
pub fn add(app: &mut App) {
    app.add_systems(AgentPass, redirect_reads.in_set(PrepareSystems::Edit));
}

fn redirect_reads(
    mut calls: Query<(&ToolRequest, &ForAgent, &mut CallHandler), (Added<Admitted>, ToolCalls)>,
    agents: Query<Option<&WorkDir>>,
    root: Option<Res<ProjectRoot>>,
) {
    for (request, &ForAgent(agent), mut handler) in &mut calls {
        if request.call.function.name.as_str() != "shell" {
            continue;
        }
        let command = request.call.function.arguments["command"]
            .as_str()
            .unwrap_or_default();
        if redirect_from_shell(command).is_none() {
            continue;
        }

        let base = agents
            .get(agent)
            .ok()
            .flatten()
            .map(|dir| dir.0.clone())
            .or_else(|| root.as_ref().map(|root| root.0.clone()))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        handler.0 = Handler(ErasedHandler::new(ToolAdapter::new(RedirectedRead {
            base,
        })));
    }
}

/// The arguments a redirected call receives: the shell call's own.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ShellCall {
    command: String,
    #[serde(default)]
    cwd: Option<String>,
}

/// The handler that answers a shell read with the toolkit.
#[derive(Clone)]
struct RedirectedRead {
    base: PathBuf,
}

impl Tool for RedirectedRead {
    // The call being served is a `shell` call, and rig's adapter refuses to serve one tool as
    // another. The model still sees a shell result; the content is the verb's.
    const NAME: &'static str = "shell";
    type Args = ShellCall;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "A shell read served by the agent toolkit.".to_owned()
    }

    fn parameters(&self) -> serde_json::Value {
        args_schema::<ShellCall>()
    }

    fn footprint(&self, _: &ShellCall) -> Touches {
        Touches::Reads(Scope::Anything)
    }

    async fn call(
        &self,
        context: &mut ToolContext,
        args: ShellCall,
    ) -> Result<String, ToolExecutionError> {
        let redirect = redirect_from_shell(&args.command).ok_or_else(|| {
            ToolExecutionError::invalid_args(format!(
                "`{}` is not a read this redirect serves",
                args.command
            ))
        })?;
        let tool = redirect.tool();
        let base = resolve_base(&self.base, args.cwd.as_deref());
        let (served, text) = blocking(move || serve(base, redirect)).await?;

        match served.as_slice() {
            [Served::Search(facts)] => publish(context, PublishedSearch(facts.clone()))?,
            [Served::Slice(facts)] => publish(context, PublishedSlice(facts.clone()))?,
            [Served::State(facts)] => publish(context, PublishedState(facts.clone()))?,
            [Served::Log(facts)] => publish(context, PublishedLog(facts.clone()))?,
            [Served::Diff(facts)] => publish(context, PublishedDiff(facts.clone()))?,
            many => publish(
                context,
                PublishedBatch(many.iter().map(Served::fact).collect()),
            )?,
        }
        publish(
            context,
            Redirection {
                command: args.command,
                tool,
            },
        )?;
        Ok(text)
    }
}

/// The typed answer to one served read, so the facts can be published under their own key.
enum Served {
    Search(SearchFacts),
    Slice(SliceFacts),
    State(StateFacts),
    Log(LogFacts),
    Diff(DiffFacts),
}

impl Served {
    fn fact(&self) -> ServedFact {
        match self {
            Self::Search(facts) => ServedFact::Search(facts.clone()),
            Self::Slice(facts) => ServedFact::Slice(facts.clone()),
            Self::State(facts) => ServedFact::State(facts.clone()),
            Self::Log(facts) => ServedFact::Log(facts.clone()),
            Self::Diff(facts) => ServedFact::Diff(facts.clone()),
        }
    }
}

/// One fact from a compound answer, tagged with the tool that produced it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "tool", content = "facts")]
pub enum ServedFact {
    #[serde(rename = "at_peek_search")]
    Search(SearchFacts),
    #[serde(rename = "at_peek_slice")]
    Slice(SliceFacts),
    #[serde(rename = "at_recall_state")]
    State(StateFacts),
    #[serde(rename = "at_recall_log")]
    Log(LogFacts),
    #[serde(rename = "at_recall_diff")]
    Diff(DiffFacts),
}

/// The facts of a compound shell call, under one key. A single redirect keeps its usual key.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PublishedBatch(pub Vec<ServedFact>);

impl rig_harness::rig_core::tool::ContextValue for PublishedBatch {
    const KEY: &'static str = "agent_playbook.redirects";
}

/// Serve the read and render the verb's bytes. The facts are kept for the effect log.
fn serve(base: PathBuf, redirect: Redirect) -> Result<(Vec<Served>, String), ToolExecutionError> {
    match redirect {
        Redirect::Compound(redirects) => {
            let mut served = Vec::new();
            let mut text = String::new();
            for redirect in redirects {
                let (mut facts, part) = serve(base.clone(), redirect)?;
                served.append(&mut facts);
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&part);
            }
            Ok((served, text))
        }
        Redirect::Search { pattern, paths } => {
            let opts = peek_opts(&base)?;
            let mut targets = vec![pattern];
            targets.extend(paths);
            let facts = search::collect(&targets, &opts, Mode::Matches).map_err(fail)?;
            let text = facts.report(&opts).render();
            Ok((vec![Served::Search(facts)], text))
        }
        Redirect::Slice { targets } => {
            let opts = peek_opts(&base)?;
            let facts = slice_verb::collect(&targets, &opts).map_err(fail)?;
            let text = facts.report(&opts).report.render();
            Ok((vec![Served::Slice(facts)], text))
        }
        Redirect::State => {
            let opts = recall_opts(&base, RecallConfig::default())?;
            let facts = state_verb::collect(&opts).map_err(fail)?;
            let text = facts.report(&opts).report.render();
            Ok((vec![Served::State(facts)], text))
        }
        Redirect::Log { args, limit } => {
            let opts = recall_opts(
                &base,
                RecallConfig {
                    limit,
                    ..RecallConfig::default()
                },
            )?;
            let facts = log_verb::collect(&args, &opts).map_err(fail)?;
            let text = facts.report(&opts).report.render();
            Ok((vec![Served::Log(facts)], text))
        }
        Redirect::Diff {
            args,
            patch,
            ignore_space,
        } => {
            let opts = recall_opts(
                &base,
                RecallConfig {
                    patch,
                    ignore_space,
                    ..RecallConfig::default()
                },
            )?;
            let facts = diff_verb::collect(&args, &opts).map_err(fail)?;
            let text = facts.report(&opts).report.render();
            Ok((vec![Served::Diff(facts)], text))
        }
    }
}

fn peek_opts(base: &Path) -> Result<PeekOpts, ToolExecutionError> {
    let root = Root::at(base).map_err(fail)?;
    PeekOpts::resolve(root, true, None, None, false, false).map_err(fail)
}

fn recall_opts(base: &Path, config: RecallConfig) -> Result<RecallOpts, ToolExecutionError> {
    RecallOpts::resolve(Some(base), config).map_err(fail)
}

fn resolve_base(base: &Path, cwd: Option<&str>) -> PathBuf {
    match cwd {
        Some(cwd) if Path::new(cwd).is_absolute() => PathBuf::from(cwd),
        Some(cwd) => base.join(cwd),
        None => base.to_path_buf(),
    }
}

/// Whether a shell command is one this module serves.
pub(crate) fn is_redirect(command: &str) -> bool {
    redirect_from_shell(command).is_some()
}

/// The redirect behind a shell command, when the command is one this module can serve without
/// guessing. `None` means the call is left as the model wrote it.
fn redirect_from_shell(command: &str) -> Option<Redirect> {
    let segments = shell_segments(command)?;
    let mut redirects = Vec::new();
    for words in segments {
        redirects.push(redirect_segment(&words)?);
    }
    match redirects.len() {
        0 => None,
        1 => redirects.pop(),
        _ => Some(Redirect::Compound(redirects)),
    }
}

fn redirect_segment(words: &[String]) -> Option<Redirect> {
    match words.first()?.as_str() {
        "rg" | "grep" => {
            search_from_words(words).map(|(pattern, paths)| Redirect::Search { pattern, paths })
        }
        "cat" => cat_from_words(words),
        "sed" => sed_from_words(words),
        "head" => head_from_words(words),
        "git" => git_from_words(words),
        _ => None,
    }
}

/// Split on unquoted `;`, the shape an exploratory `git status; git log; git diff` uses. A
/// pipeline, a redirection, a conditional (`&&`/`||`), a substitution or a background job is not
/// decomposed: the whole call is left as written. Empty segments are skipped, so a trailing `;`
/// is not a second command.
fn shell_segments(command: &str) -> Option<Vec<Vec<String>>> {
    let mut segments = Vec::new();
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;

    for ch in command.chars() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => current.push(ch),
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None if ch.is_whitespace() => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            None if ch == ';' => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
                if !words.is_empty() {
                    segments.push(std::mem::take(&mut words));
                }
            }
            None if matches!(ch, '|' | '&' | '>' | '<' | '$' | '`') => return None,
            None => current.push(ch),
        }
    }
    if quote.is_some() {
        return None;
    }
    if !current.is_empty() {
        words.push(current);
    }
    if !words.is_empty() {
        segments.push(words);
    }
    if segments.is_empty() {
        return None;
    }
    Some(segments)
}

/// `cat <path>` — one path, no flags. `cat a b` concatenates and `cat -n` numbers differently;
/// neither is the slice's answer, so neither is served.
fn cat_from_words(words: &[String]) -> Option<Redirect> {
    let [_, path] = words else {
        return None;
    };
    if path.is_empty() || path.starts_with('-') {
        return None;
    }
    Some(Redirect::Slice {
        targets: vec![path.clone()],
    })
}

/// `sed -n 'A,Bp' <path>` and `sed -n 'Ap' <path>` — one simple line range on one path. A
/// substitution, a second script, an in-place flag or a missing `-n` is a different command.
fn sed_from_words(words: &[String]) -> Option<Redirect> {
    let [_, quiet, script, path] = words else {
        return None;
    };
    if !matches!(quiet.as_str(), "-n" | "--quiet" | "--silent") {
        return None;
    }
    if path.is_empty() || path.starts_with('-') {
        return None;
    }
    let range = sed_range(script)?;
    Some(Redirect::Slice {
        targets: vec![format!("{path}:{range}")],
    })
}

fn sed_range(script: &str) -> Option<String> {
    let script = script.strip_suffix('p')?;
    let parts: Vec<&str> = script.split(',').collect();
    if parts.is_empty() || parts.len() > 2 {
        return None;
    }
    if !parts
        .iter()
        .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    Some(parts.join("-"))
}

/// `head <path>`, `head -N <path>`, `head -n N <path>` and `head --lines=N <path>` — one path,
/// from the top, as a slice. `head -c` is bytes and is left alone.
fn head_from_words(words: &[String]) -> Option<Redirect> {
    let (count, path) = match words {
        [_, path] => (10, path),
        [_, flag, count, path] if flag == "-n" || flag == "--lines" => {
            (count.parse::<usize>().ok()?, path)
        }
        [_, flag, path] => {
            if let Some(value) = flag.strip_prefix("--lines=") {
                (value.parse::<usize>().ok()?, path)
            } else {
                let digits = flag.strip_prefix('-')?;
                if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
                    return None;
                }
                (digits.parse::<usize>().ok()?, path)
            }
        }
        _ => return None,
    };
    if count == 0 || path.is_empty() || path.starts_with('-') {
        return None;
    }
    Some(Redirect::Slice {
        targets: vec![format!("{path}:1-{count}")],
    })
}

fn git_from_words(words: &[String]) -> Option<Redirect> {
    let subcommand = words.get(1)?.as_str();
    let args = &words[2..];
    match subcommand {
        "status" => git_status(args),
        "log" => git_log(args),
        "diff" => git_diff(args),
        _ => None,
    }
}

/// The flags that ask the same status question. `-uall`, `-uno`, `--porcelain` and `--ignored`
/// ask for a shape the verb does not produce, and a pathspec is a narrower question; all are left
/// to the shell.
const STATUS_FLAGS: &[&str] = &[
    "--short",
    "-s",
    "-sb",
    "-b",
    "--branch",
    "-unormal",
    "--untracked-files=normal",
];

fn git_status(args: &[String]) -> Option<Redirect> {
    if args.iter().all(|arg| STATUS_FLAGS.contains(&arg.as_str())) {
        Some(Redirect::State)
    } else {
        None
    }
}

/// `git log`, `git log --oneline`, and a `-n`/`-N`/`--max-count` limit. A format flag, a graph or
/// a date filter changes the answer, so it is not served.
fn git_log(args: &[String]) -> Option<Redirect> {
    let mut limit = None;
    let mut positionals = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--oneline" => {}
            "-n" | "--max-count" => {
                index += 1;
                limit = Some(args.get(index)?.parse::<usize>().ok()?);
            }
            _ if arg.starts_with("--max-count=") => {
                limit = Some(arg["--max-count=".len()..].parse::<usize>().ok()?);
            }
            _ if arg.starts_with('-')
                && arg.len() > 1
                && arg[1..].chars().all(|c| c.is_ascii_digit()) =>
            {
                limit = Some(arg[1..].parse::<usize>().ok()?);
            }
            _ if arg.starts_with('-') => return None,
            _ => positionals.push(arg.clone()),
        }
        index += 1;
    }
    if limit == Some(0) {
        return None;
    }
    Some(Redirect::Log {
        args: positionals,
        limit,
    })
}

/// `git diff`, `--stat`, `-p` and `-w`. `--cached`, `--name-only` and a word diff are different
/// questions; they are left alone.
fn git_diff(args: &[String]) -> Option<Redirect> {
    let mut patch = false;
    let mut ignore_space = false;
    let mut positionals = Vec::new();
    let mut paths_only = false;
    for arg in args {
        match arg.as_str() {
            "--patch" | "-p" => patch = true,
            "--stat" => {}
            "--no-color" | "--color=never" => {}
            "-w" | "--ignore-all-space" => ignore_space = true,
            "--" => paths_only = true,
            _ if paths_only => positionals.push(arg.clone()),
            _ if arg.starts_with('-') => return None,
            _ => positionals.push(arg.clone()),
        }
    }
    Some(Redirect::Diff {
        args: positionals,
        patch,
        ignore_space,
    })
}

/// The search behind a shell segment, when the segment is one this module can serve without
/// guessing.
///
/// Accepted: `rg [--line-number] PATTERN [PATH…]` and `grep -r[n] PATTERN [PATH…]`. Rejected: any
/// flag that changes the answer (`-i`, `-c`, `-l`, `-e`, `-f`, `--glob`, `--hidden`) and every
/// other program. The segment has already been split from its compound and has no operator left.
fn search_from_words(words: &[String]) -> Option<(String, Vec<String>)> {
    let program = words.first()?.as_str();
    if !matches!(program, "rg" | "grep") {
        return None;
    }

    let recursive = words
        .iter()
        .any(|word| matches!(word.as_str(), "-r" | "-rn" | "-nr" | "--recursive"));
    if program == "grep" && !recursive {
        return None;
    }

    let mut pattern = None;
    let mut paths = Vec::new();
    for word in &words[1..] {
        if word.starts_with('-') {
            let allowed = match program {
                "rg" => matches!(word.as_str(), "-n" | "--line-number"),
                "grep" => matches!(
                    word.as_str(),
                    "-n" | "-r" | "-rn" | "-nr" | "--line-number" | "--recursive"
                ),
                _ => false,
            };
            if !allowed {
                return None;
            }
            continue;
        }
        match &pattern {
            None => pattern = Some(word.clone()),
            Some(_) => paths.push(word.clone()),
        }
    }

    Some((pattern?, paths))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redirected(command: &str) -> Option<Redirect> {
        redirect_from_shell(command)
    }

    fn search(pattern: &str, paths: &[&str]) -> Redirect {
        Redirect::Search {
            pattern: pattern.to_owned(),
            paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        }
    }

    #[test]
    fn a_plain_rg_is_a_search() {
        assert_eq!(
            redirected("rg -n 'render_claim'"),
            Some(search("render_claim", &[]))
        );
        assert_eq!(
            redirected("rg render_claim src docs"),
            Some(search("render_claim", &["src", "docs"]))
        );
    }

    #[test]
    fn a_recursive_grep_is_a_search() {
        assert_eq!(
            redirected("grep -rn foo src"),
            Some(search("foo", &["src"]))
        );
        assert_eq!(redirected("grep foo src"), None);
    }

    #[test]
    fn an_alternation_inside_quotes_survives() {
        assert_eq!(
            redirected("rg 'alpha|beta' ."),
            Some(search("alpha|beta", &["."]))
        );
    }

    #[test]
    fn a_pipeline_or_redirection_is_not_redirected() {
        for command in [
            "rg foo | head -20",
            "rg foo > out.txt",
            "rg foo; rm -rf .",
            "rg foo && cargo test",
            "rg $(cat pattern.txt)",
            "git status | head -3",
            "cat src/lib.rs > /tmp/x",
        ] {
            assert_eq!(redirected(command), None, "`{command}`");
        }
    }

    #[test]
    fn a_flag_that_changes_the_answer_is_not_redirected() {
        for command in [
            "rg -i foo .",
            "rg -c foo .",
            "rg -l foo .",
            "rg -e foo .",
            "rg --glob '*.rs' foo",
            "rg --hidden foo",
            "rg -- foo",
        ] {
            assert_eq!(redirected(command), None, "`{command}`");
        }
    }

    #[test]
    fn another_program_is_not_redirected() {
        for command in ["wc -l src/lib.rs", "tail -n 5 Cargo.toml", "ls -la", "pwd"] {
            assert_eq!(redirected(command), None, "`{command}`");
        }
    }

    #[test]
    fn an_unbalanced_quote_is_not_redirected() {
        assert_eq!(redirected("rg 'foo"), None);
        assert_eq!(redirected("rg \"foo"), None);
        assert_eq!(redirected("cat 'foo"), None);
    }

    #[test]
    fn a_plain_cat_is_a_slice() {
        assert_eq!(
            redirected("cat src/lib.rs"),
            Some(Redirect::Slice {
                targets: vec!["src/lib.rs".to_owned()]
            })
        );
        assert_eq!(redirected("cat -n src/lib.rs"), None);
        assert_eq!(redirected("cat a b"), None);
    }

    #[test]
    fn a_simple_sed_range_is_a_slice() {
        assert_eq!(
            redirected("sed -n '40,60p' src/lib.rs"),
            Some(Redirect::Slice {
                targets: vec!["src/lib.rs:40-60".to_owned()]
            })
        );
        assert_eq!(
            redirected("sed -n '40p' src/lib.rs"),
            Some(Redirect::Slice {
                targets: vec!["src/lib.rs:40".to_owned()]
            })
        );
        assert_eq!(redirected("sed '40,60p' src/lib.rs"), None);
        assert_eq!(redirected("sed -n 's/a/b/p' src/lib.rs"), None);
    }

    #[test]
    fn a_head_is_a_slice_from_the_top() {
        for (command, target) in [
            ("head Cargo.toml", "Cargo.toml:1-10"),
            ("head -20 Cargo.toml", "Cargo.toml:1-20"),
            ("head -n 5 Cargo.toml", "Cargo.toml:1-5"),
            ("head --lines=5 Cargo.toml", "Cargo.toml:1-5"),
        ] {
            assert_eq!(
                redirected(command),
                Some(Redirect::Slice {
                    targets: vec![target.to_owned()]
                }),
                "`{command}`"
            );
        }
        assert_eq!(redirected("head -c 20 Cargo.toml"), None);
        assert_eq!(redirected("head -n 0 Cargo.toml"), None);
    }

    #[test]
    fn a_plain_git_status_is_state() {
        for command in [
            "git status",
            "git status --short",
            "git status -s",
            "git status -sb",
            "git status --untracked-files=normal",
        ] {
            assert_eq!(redirected(command), Some(Redirect::State), "`{command}`");
        }
        for command in [
            "git status --ignored",
            "git status src",
            "git status --short src",
            "git status -uall",
            "git status --porcelain=v1",
            "git -c core.pager=cat status",
        ] {
            assert_eq!(redirected(command), None, "`{command}`");
        }
    }

    #[test]
    fn a_plain_git_log_is_a_log() {
        assert_eq!(
            redirected("git log"),
            Some(Redirect::Log {
                args: Vec::new(),
                limit: None
            })
        );
        assert_eq!(
            redirected("git log --oneline -5"),
            Some(Redirect::Log {
                args: Vec::new(),
                limit: Some(5)
            })
        );
        assert_eq!(
            redirected("git log --max-count=5 HEAD"),
            Some(Redirect::Log {
                args: vec!["HEAD".to_owned()],
                limit: Some(5)
            })
        );
        for command in ["git log --graph", "git log --author=damo", "git log -- ."] {
            assert_eq!(redirected(command), None, "`{command}`");
        }
    }

    #[test]
    fn a_plain_git_diff_is_a_diff() {
        assert_eq!(
            redirected("git diff"),
            Some(Redirect::Diff {
                args: Vec::new(),
                patch: false,
                ignore_space: false
            })
        );
        assert_eq!(
            redirected("git diff --stat"),
            Some(Redirect::Diff {
                args: Vec::new(),
                patch: false,
                ignore_space: false
            })
        );
        assert_eq!(
            redirected("git diff -p -w"),
            Some(Redirect::Diff {
                args: Vec::new(),
                patch: true,
                ignore_space: true
            })
        );
        assert_eq!(
            redirected("git diff HEAD -- src"),
            Some(Redirect::Diff {
                args: vec!["HEAD".to_owned(), "src".to_owned()],
                patch: false,
                ignore_space: false
            })
        );
        for command in [
            "git diff --cached",
            "git diff --staged",
            "git diff --name-only",
            "git diff -U3",
        ] {
            assert_eq!(redirected(command), None, "`{command}`");
        }
    }

    #[test]
    fn a_semicolon_compound_is_one_redirect() {
        assert_eq!(
            redirected("git status --short; git log --oneline -1; git diff"),
            Some(Redirect::Compound(vec![
                Redirect::State,
                Redirect::Log {
                    args: Vec::new(),
                    limit: Some(1)
                },
                Redirect::Diff {
                    args: Vec::new(),
                    patch: false,
                    ignore_space: false
                },
            ]))
        );
        assert_eq!(
            redirected("cat a; cat b"),
            Some(Redirect::Compound(vec![
                Redirect::Slice {
                    targets: vec!["a".to_owned()]
                },
                Redirect::Slice {
                    targets: vec!["b".to_owned()]
                },
            ]))
        );
    }

    #[test]
    fn a_compound_with_a_segment_we_cannot_serve_is_left_alone() {
        for command in [
            "git status; echo done",
            "cat a; wc -l b",
            "git status; ls",
            "cat a; git status | head -3",
        ] {
            assert_eq!(redirected(command), None, "`{command}`");
        }
    }

    #[test]
    fn a_conditional_or_background_command_is_not_decomposed() {
        for command in [
            "git status && git log",
            "git status || git log",
            "git status & git log",
        ] {
            assert_eq!(redirected(command), None, "`{command}`");
        }
    }

    #[test]
    fn a_quoted_semicolon_does_not_split() {
        assert_eq!(
            redirected("cat 'a;b'"),
            Some(Redirect::Slice {
                targets: vec!["a;b".to_owned()]
            })
        );
    }

    #[test]
    fn an_empty_segment_is_skipped() {
        assert_eq!(redirected("git status;"), Some(Redirect::State));
        assert_eq!(redirected("; git status"), Some(Redirect::State));
        assert_eq!(
            redirected("git status;; git log"),
            Some(Redirect::Compound(vec![
                Redirect::State,
                Redirect::Log {
                    args: Vec::new(),
                    limit: None
                },
            ]))
        );
    }
}
