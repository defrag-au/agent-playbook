//! Answer a shell read with the toolkit, without a refusal round-trip.
//!
//! A Gate that refuses costs the model a call: it has to read the refusal and ask again. This
//! module replaces the call's handler in `Prepare` instead, so a plain `rg`/`grep` runs
//! `at-peek search` in process and the model sees the bounded answer as the shell's result.
//!
//! Only the shapes that can be mapped without guessing are redirected. Anything else — a pipeline,
//! an unknown flag, `cat`, `git status` — is left to the Gate, which still refuses it and names the
//! tool. The redirect is deliberately conservative: a wrong substitution is worse than a refusal.

use std::path::{Path, PathBuf};

use rig_harness::prelude::*;
use rig_harness::rig_core::serve::{adapters::ToolAdapter, ErasedHandler};
use rig_harness::rig_core::tool::{Scope, Touches};
use rig_harness::rig_ecs::effects::Handler;
use serde::Deserialize;

use at_peek::paths::Root;
use at_peek::verbs::search::{self, Mode, SearchFacts};
use at_peek::verbs::Opts;

use crate::fail;
use crate::tools::PublishedSearch;

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

/// Replace a `shell` call that is a plain search with the in-process one.
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
        if search_from_shell(command).is_none() {
            continue;
        }

        let base = agents
            .get(agent)
            .ok()
            .flatten()
            .map(|dir| dir.0.clone())
            .or_else(|| root.as_ref().map(|root| root.0.clone()))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        handler.0 = Handler(ErasedHandler::new(ToolAdapter::new(RedirectedSearch {
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

/// The handler that answers a shell search with `at-peek search`.
#[derive(Clone)]
struct RedirectedSearch {
    base: PathBuf,
}

impl Tool for RedirectedSearch {
    // The call being served is a `shell` call, and rig's adapter refuses to serve one tool as
    // another. The model still sees a shell result; the content is the search's.
    const NAME: &'static str = "shell";
    type Args = ShellCall;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "A shell read served by at-peek search.".to_owned()
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
        let (pattern, paths) = search_from_shell(&args.command).ok_or_else(|| {
            ToolExecutionError::invalid_args(format!(
                "`{}` is not a search this redirect serves",
                args.command
            ))
        })?;

        let base = match args.cwd.as_deref() {
            Some(cwd) if Path::new(cwd).is_absolute() => PathBuf::from(cwd),
            Some(cwd) => self.base.join(cwd),
            None => self.base.clone(),
        };

        let (facts, opts) = blocking(move || run_search(base, pattern, paths)).await?;
        let text = facts.report(&opts).render();

        context
            .insert_result(PublishedSearch(facts))
            .map_err(|error| {
                ToolExecutionError::other(format!("could not publish the search facts: {error}"))
            })?;
        context
            .insert_result(Redirection {
                command: args.command,
                tool: "at_peek_search".to_owned(),
            })
            .map_err(|error| {
                ToolExecutionError::other(format!("could not publish the redirection: {error}"))
            })?;
        Ok(text)
    }
}

fn run_search(
    base: PathBuf,
    pattern: String,
    paths: Vec<String>,
) -> Result<(SearchFacts, Opts), ToolExecutionError> {
    let root = Root::at(&base).map_err(fail)?;
    let opts = Opts::resolve(root, true, None, None, false, false).map_err(fail)?;
    let mut targets = vec![pattern];
    targets.extend(paths);
    let facts = search::collect(&targets, &opts, Mode::Matches).map_err(fail)?;
    Ok((facts, opts))
}

/// The search behind a shell command, when the command is one this module can serve without
/// guessing.
///
/// Accepted: `rg [--line-number] PATTERN [PATH…]` and `grep -r[n] PATTERN [PATH…]`. Rejected:
/// pipelines and redirections, any flag that changes the answer (`-i`, `-c`, `-l`, `-e`, `-f`,
/// `--glob`, `--hidden`), an unbalanced quote, and every other program. A rejected command still
/// goes to the Gate, so the model is refused rather than served something else.
pub(crate) fn search_from_shell(command: &str) -> Option<(String, Vec<String>)> {
    let words = shell_words(command)?;
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

/// Split a command the way a shell would for the quoting it accepts, and refuse anything with an
/// unquoted operator: a pipeline, a redirection, a substitution, or a second command. A `|` inside
/// quotes is a regex alternation and is kept.
fn shell_words(command: &str) -> Option<Vec<String>> {
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
            None if matches!(ch, '|' | ';' | '&' | '>' | '<' | '$' | '`') => return None,
            None => current.push(ch),
        }
    }
    if quote.is_some() {
        return None;
    }
    if !current.is_empty() {
        words.push(current);
    }
    Some(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn searched(command: &str) -> Option<(String, Vec<String>)> {
        search_from_shell(command)
    }

    #[test]
    fn a_plain_rg_is_a_search() {
        assert_eq!(
            searched("rg -n 'render_claim'"),
            Some(("render_claim".to_owned(), Vec::new()))
        );
        assert_eq!(
            searched("rg render_claim src docs"),
            Some((
                "render_claim".to_owned(),
                vec!["src".to_owned(), "docs".to_owned()]
            ))
        );
    }

    #[test]
    fn a_recursive_grep_is_a_search() {
        assert_eq!(
            searched("grep -rn foo src"),
            Some(("foo".to_owned(), vec!["src".to_owned()]))
        );
        assert_eq!(searched("grep foo src"), None);
    }

    #[test]
    fn an_alternation_inside_quotes_survives() {
        assert_eq!(
            searched("rg 'alpha|beta' ."),
            Some(("alpha|beta".to_owned(), vec![".".to_owned()]))
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
        ] {
            assert_eq!(searched(command), None, "`{command}`");
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
            assert_eq!(searched(command), None, "`{command}`");
        }
    }

    #[test]
    fn another_program_is_not_redirected() {
        for command in [
            "cat src/lib.rs",
            "git status",
            "wc -l src/lib.rs",
            "sed -n 1,5p x",
        ] {
            assert_eq!(searched(command), None, "`{command}`");
        }
    }

    #[test]
    fn an_unbalanced_quote_is_not_redirected() {
        assert_eq!(searched("rg 'foo"), None);
        assert_eq!(searched("rg \"foo"), None);
    }
}
