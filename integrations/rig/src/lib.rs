//! The agent-playbook, enforced in rig.
//!
//! Three parts, all on the kernel's public API:
//!
//! - **Tools.** The `at-*` verbs as native rig tools: typed arguments, a schema derived from the
//!   argument type, a `Footprint` that says they only read, and the agent's own directory as the
//!   root. A call is an effect, so it is recorded with its turn, its parent and its outcome.
//! - **A redirect.** A plain `rg`/`grep` shell call is answered in `Prepare` by `at-peek search`
//!   in process. The model sees the bounded answer as the shell's result, with no refusal to
//!   read and no second call.
//! - **A Gate.** Any other shell read is refused, and the refusal names the tool that replaces
//!   it. The model reads the refusal and retries with the tool.
//!
//! The playbook's rule stays in `AGENTS.md` as the human-readable copy. This is the same rule the
//! harness applies.

mod redirect;
mod spawn;
mod tools;

pub use redirect::Redirection;
pub use tools::PublishedSearch;

use rig_harness::prelude::*;

/// A failure from the toolkit's own contract: a usage error reaches the model as a usage error and
/// a refusal as a refusal, rather than everything becoming "other".
pub(crate) fn fail(failure: at_peek::contract::Fail) -> ToolExecutionError {
    match failure.exit {
        at_peek::contract::Exit::Usage => ToolExecutionError::invalid_args(failure.message),
        at_peek::contract::Exit::Refused => ToolExecutionError::refused(failure.message),
        _ => ToolExecutionError::other(failure.message),
    }
}

/// The plugin: the tools, and the gate that points at them.
#[derive(Default)]
pub struct PlaybookPlugin;

impl Plugin for PlaybookPlugin {
    fn build(&self, app: &mut App) {
        tools::add(app);
        redirect::add(app);
        app.add_systems(AgentPass, refuse_shell_reads.in_set(Stage::Gate));
    }
}

/// The same tools with no gate: the baseline for measuring what enforcement costs.
///
/// Load it under `[[plugins.extra]] plugin = "rig_playbook::ToolsOnlyPlugin"` and run the same
/// prompt as [`PlaybookPlugin`]. It exists for the comparison, not for daily use.
#[derive(Default)]
pub struct ToolsOnlyPlugin;

impl Plugin for ToolsOnlyPlugin {
    fn build(&self, app: &mut App) {
        tools::add(app);
    }
}

/// Refuse a `shell` call that is really a read, and say which tool to use instead.
fn refuse_shell_reads(mut calls: Query<(&ToolRequest, &mut Decision), (ReadyToStart, ToolCalls)>) {
    for (request, mut decision) in &mut calls {
        if request.call.function.name.as_str() != "shell" {
            continue;
        }

        let command = request.call.function.arguments["command"]
            .as_str()
            .unwrap_or_default();

        if let Some(why) = refusal(command) {
            decision.decide(Decider::plugin("playbook"), Outcome::refused(why));
        }
    }
}

/// The message a refused read gets. It names the tool, and says what the model would otherwise
/// do next: a retry through `shell`, or the same command with `; echo "exit=$?"` appended to
/// recover the status it thinks it lost. The replacement answers the same question and states
/// the total, so there is nothing to recover.
fn refusal(command: &str) -> Option<String> {
    // A plain search is answered in Prepare with the toolkit, so there is nothing to refuse.
    if redirect::search_from_shell(command).is_some() {
        return None;
    }
    let tool = replacement(command)?;
    Some(format!(
        "`{}` is a read the toolkit replaces · call `{tool}`. It answers the same question with the \
         matches and their true total, so a retry through `shell` gains nothing; appending to the \
         command does not change what it is judged on.",
        program(command).unwrap_or("that command"),
    ))
}

/// The tool that replaces `command`, when the command is a read.
///
/// Only the first segment of a pipeline is considered: a read that filters another command's
/// output (`cargo test | grep FAIL`) is not a read of the working tree.
fn replacement(command: &str) -> Option<&'static str> {
    let first = command.split(['|', '&', ';']).next().unwrap_or_default();
    let mut words = first.split_whitespace();
    let program = words.next()?;

    match program {
        "rg" | "grep" => Some("at_peek_search"),
        "cat" | "head" | "tail" => Some("at_peek_slice"),
        // `sed -i` is a write. The read form is the one that has a replacement.
        "sed" if !words.any(|word| word.starts_with("-i") || word.starts_with("--in-place")) => {
            Some("at_peek_slice")
        }
        "wc" => Some("at_peek_stat"),
        "git" => match words.next()? {
            "status" => Some("at_recall_state"),
            "diff" => Some("at_recall_diff"),
            "log" => Some("at_recall_log"),
            _ => None,
        },
        _ => None,
    }
}

/// The program named by the first segment, for the refusal's text.
fn program(command: &str) -> Option<&str> {
    command
        .split(['|', '&', ';'])
        .next()?
        .split_whitespace()
        .next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_are_refused_with_the_tool_that_replaces_them() {
        for (command, tool) in [
            ("rg 'render_claim'", "at_peek_search"),
            ("grep -rn TODO src", "at_peek_search"),
            ("cat src/lib.rs", "at_peek_slice"),
            ("sed -n '40,60p' src/lib.rs", "at_peek_slice"),
            ("head -20 Cargo.toml", "at_peek_slice"),
            ("tail -n 5 Cargo.toml", "at_peek_slice"),
            ("wc -l src/*.rs", "at_peek_stat"),
            ("git status --short", "at_recall_state"),
            ("git diff --stat", "at_recall_diff"),
            ("git log --oneline -5", "at_recall_log"),
        ] {
            assert_eq!(replacement(command), Some(tool), "`{command}`");
        }
    }

    #[test]
    fn writes_and_builds_are_left_alone() {
        for command in [
            "git add src/lib.rs",
            "git commit -m 'x'",
            "sed -i 's/a/b/' src/lib.rs",
            "cargo test",
            "cargo build --workspace",
            "node scripts/build.mjs",
        ] {
            assert_eq!(replacement(command), None, "`{command}`");
        }
    }

    #[test]
    fn only_the_first_command_of_a_pipeline_is_judged() {
        assert_eq!(replacement("cargo test | grep FAIL"), None);
        assert_eq!(replacement("rg foo && cargo test"), Some("at_peek_search"));
        assert_eq!(replacement("  git   log  "), Some("at_recall_log"));
    }

    #[test]
    fn the_refusal_names_the_program_and_the_tool() {
        // A command the redirect does not serve, so it still reaches the Gate.
        let message = refusal("cat src/lib.rs").expect("a read");
        assert_eq!(
            message,
            "`cat` is a read the toolkit replaces · call `at_peek_slice`. It answers the same \
             question with the matches and their true total, so a retry through `shell` gains \
             nothing; appending to the command does not change what it is judged on."
        );
    }

    #[test]
    fn a_redirected_read_is_not_refused() {
        // The shape is answered in Prepare, so the Gate must leave it alone; a refusal here would
        // be a second answer to a call that already has one.
        assert_eq!(refusal("rg -n 'render_claim' ."), None);
        assert_eq!(refusal("grep -rn foo src"), None);
    }

    #[test]
    fn a_command_that_is_not_a_read_has_no_refusal() {
        assert_eq!(refusal("cargo test"), None);
        assert_eq!(refusal("git commit -m 'x'"), None);
        assert_eq!(refusal("node scripts/build.mjs"), None);
    }
}
