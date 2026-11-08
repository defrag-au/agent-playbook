//! The agent-playbook, enforced in rig.
//!
//! Two halves, both on the kernel's public API:
//!
//! - **Tools.** The `at-*` verbs as native rig tools: typed arguments, a schema derived from the
//!   argument type, a `Footprint` that says they only read, and the agent's own directory as the
//!   root. A call is an effect, so it is recorded with its turn, its parent and its outcome.
//! - **A Gate.** A `shell` call that is really a read is refused, and the refusal names the tool
//!   that replaces it. The model reads the refusal and retries with the tool.
//!
//! The playbook's rule stays in `AGENTS.md` as the human-readable copy. This is the same rule the
//! harness applies.

mod spawn;
mod tools;

use rig_harness::prelude::*;

/// The plugin: the tools, and the gate that points at them.
#[derive(Default)]
pub struct PlaybookPlugin;

impl Plugin for PlaybookPlugin {
    fn build(&self, app: &mut App) {
        tools::add(app);
        app.add_systems(AgentPass, refuse_shell_reads.in_set(Stage::Gate));
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

        if let Some(replacement) = replacement(command) {
            decision.decide(
                Decider::plugin("playbook"),
                Outcome::refused(format!(
                    "`{}` is a read the toolkit replaces · use {replacement}. \
                     The inspect-code skill maps the rest of the grammar.",
                    program(command).unwrap_or("that command"),
                )),
            );
        }
    }
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
        let command = "rg 'render_claim'";
        let tool = replacement(command).expect("a read");
        let message = format!(
            "`{}` is a read the toolkit replaces · use {tool}.",
            program(command).expect("a program"),
        );
        assert_eq!(
            message,
            "`rg` is a read the toolkit replaces · use at_peek_search."
        );
    }
}
