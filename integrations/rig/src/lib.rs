//! The agent-playbook, enforced in rig.
//!
//! Three parts, all on the kernel's public API:
//!
//! - **Tools.** The `at-*` verbs as native rig tools: typed arguments, a schema derived from the
//!   argument type, a `Footprint` that says they only read, and the agent's own directory as the
//!   root. Every tool calls its verb's `collect` in process and publishes the typed answer, so a
//!   call is an effect with facts rather than a rendered string. Neither `PATH` nor a subprocess is
//!   involved.
//! - **A shell policy.** A read, a build, or a build's output captured to scratch runs; a write or
//!   an unclassifiable command is refused by the classifier.
//! - **A redirect.** A plain `rg`/`grep` shell call is answered in `Prepare` by `at-peek search`
//!   in process. The model sees the bounded answer as the shell's result, with no refusal to
//!   read and no second call.
//!
//! [`PlaybookPlugin`] is the daily driver. [`AdvertisedPlugin`] adds the advertised descriptions and
//! a prompt-level tool map, for measuring whether advertising changes which tool the model reaches
//! for. [`SlimPlugin`] leaves the lightweight reads to the redirect, to measure whether a shell habit
//! served by the toolkit beats a tool the model has to be told about. [`GatedPlugin`] keeps the
//! earlier policy — a read refused unless it is a redirection or a tool — and exists for the
//! measurement that showed the gate cost more than it bought.
//!
//! The playbook's rule stays in `AGENTS.md` as the human-readable copy. This is the same rule the
//! harness applies.

mod advertise;
mod redirect;
mod tools;

pub use redirect::{PublishedBatch, Redirection, ServedFact};
pub use tools::{
    PublishedDiff, PublishedLog, PublishedOutline, PublishedPr, PublishedSearch, PublishedSlice,
    PublishedStat, PublishedState,
};

use command_classify::{classify, Analysis, Effect as ShellEffect, Writes as ShellWrites};
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

/// The plugin: the tools, the shell policy, and the redirect that answers a search without a
/// round-trip.
#[derive(Default)]
pub struct PlaybookPlugin;

impl Plugin for PlaybookPlugin {
    fn build(&self, app: &mut App) {
        tools::add(app);
        redirect::add(app);
        app.add_systems(AgentPass, apply_shell_policy.in_set(Stage::Gate));
    }
}

/// [`PlaybookPlugin`] with the toolkit advertised: each tool's description names its typed call and
/// what the shell does not give, and a short map of all seven is added to the system prompt.
///
/// The tools, the policy and the redirect are identical to [`PlaybookPlugin`], so a comparison
/// between the two measures the advertising and nothing else.
#[derive(Default)]
pub struct AdvertisedPlugin;

impl Plugin for AdvertisedPlugin {
    fn build(&self, app: &mut App) {
        tools::add_advertised(app);
        redirect::add(app);
        app.add_systems(AgentPass, apply_shell_policy.in_set(Stage::Gate));
    }

    fn finish(&self, app: &mut App) {
        advertise::add_prompt(app);
    }
}

/// [`PlaybookPlugin`] with the lightweight reads left to the redirect.
///
/// `at_peek_slice`, `at_peek_stat`, `at_recall_state` and `at_recall_log` are not registered. The
/// plain `cat`, `sed -n`, `head`, `git status`, `git log` and `git diff` forms are served in process
/// by the same verbs, so the model keeps its shell habit and still gets the bounded answer. Only
/// `at_peek_search`, `at_peek_outline`, `at_recall_diff` and `at_recall_pr` remain as explicit
/// tools, because their answers are not a rename of a shell command.
#[derive(Default)]
pub struct SlimPlugin;

impl Plugin for SlimPlugin {
    fn build(&self, app: &mut App) {
        tools::add_with(app, tools::Toolset::slim());
        redirect::add(app);
        app.add_systems(AgentPass, apply_shell_policy.in_set(Stage::Gate));
    }
}

/// The same tools with no shell policy: the baseline for measuring what enforcement costs.
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

/// The earlier policy: a `shell` call that is really a read is refused, and the refusal names the
/// tool that replaces it.
///
/// Kept because the measurement that retired it is reproducible: the same prompt under this plugin
/// and [`ToolsOnlyPlugin`] gives the cost of the gate directly.
#[derive(Default)]
pub struct GatedPlugin;

impl Plugin for GatedPlugin {
    fn build(&self, app: &mut App) {
        tools::add(app);
        redirect::add(app);
        app.add_systems(AgentPass, refuse_shell_reads.in_set(Stage::Gate));
    }
}

/// [`PlaybookPlugin`] with a terse `at_peek_stat`: the rows without the header, the bound or the
/// `# next:` line. Everything else is identical, so the two arms differ in one tool's answer and
/// nothing else.
#[derive(Default)]
pub struct TerseStatPlugin;

impl Plugin for TerseStatPlugin {
    fn build(&self, app: &mut App) {
        tools::add_with(app, tools::Toolset::terse());
        app.add_systems(AgentPass, apply_shell_policy.in_set(Stage::Gate));
    }
}

/// [`PlaybookPlugin`] with the two least-used read tools left out: `at_peek_stat` and
/// `at_recall_log`. The arm asks whether the model takes a cleaner path when the shell is the only
/// way to answer those two questions.
#[derive(Default)]
pub struct LeanPlugin;

impl Plugin for LeanPlugin {
    fn build(&self, app: &mut App) {
        tools::add_with(app, tools::Toolset::lean());
        app.add_systems(AgentPass, apply_shell_policy.in_set(Stage::Gate));
    }
}

/// Whether the policy lets a command run: every read and every run, and a scratch capture of a
/// build's output. Everything else is refused.
fn permits(analysis: &Analysis) -> bool {
    matches!(analysis.effect, ShellEffect::Read | ShellEffect::Run)
        || (analysis.effect == ShellEffect::Write && analysis.writes == ShellWrites::Capture)
}

/// Refuse a `shell` call that can write the working tree, or one the classifier cannot read.
fn apply_shell_policy(mut calls: Query<(&ToolRequest, &mut Decision), (ReadyToStart, ToolCalls)>) {
    for (request, mut decision) in &mut calls {
        if request.call.function.name.as_str() != "shell" {
            continue;
        }

        let command = request.call.function.arguments["command"]
            .as_str()
            .unwrap_or_default();

        // A call the redirect serves is answered in Prepare by a read-only verb; the policy has
        // nothing to decide about it.
        if redirect::is_redirect(command) {
            continue;
        }

        let analysis = classify(command);

        if permits(&analysis) {
            continue;
        }

        decision.decide(
            Decider::plugin("playbook"),
            Outcome::refused(policy_refusal(command, &analysis)),
        );
    }
}

/// The message a refused command gets: which rule it broke, and the way to do it instead.
fn policy_refusal(command: &str, analysis: &Analysis) -> String {
    if let Some(problem) = &analysis.parse_error {
        return format!(
            "`{command}` could not be parsed — {problem} — so `shell` will not run it. \
             Split it into single commands."
        );
    }

    if analysis
        .segments
        .iter()
        .any(|segment| segment.effect == ShellEffect::Write)
    {
        return format!(
            "`{command}` writes a file, so `shell` will not run it. Use `edit` or `write` to change \
             a file; only a build's output captured to /tmp is allowed."
        );
    }

    match analysis
        .segments
        .iter()
        .find(|segment| segment.effect == ShellEffect::Unknown)
    {
        Some(segment) => format!(
            "`{command}` runs `{}`, which this harness cannot classify, so `shell` will not run \
             it. Run one command at a time.",
            segment.program
        ),
        None => format!("`{command}` is not a command this harness will run."),
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
    // A shell read the redirect serves is answered in Prepare, so there is nothing to refuse.
    if redirect::is_redirect(command) {
        return None;
    }
    let tool = replacement(command)?;
    Some(format!(
        "`{}` is a read the toolkit replaces · call `{tool}`. It answers the same question with a \
         stated bound, so a retry through `shell` gains nothing; appending to the command does not \
         change what it is judged on.",
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
mod policy_tests {
    use super::*;

    fn refusal_for(command: &str) -> Option<String> {
        let analysis = classify(command);
        if permits(&analysis) {
            None
        } else {
            Some(policy_refusal(command, &analysis))
        }
    }

    #[test]
    fn reads_and_builds_are_not_refused() {
        for command in [
            "ls -la",
            "cat src/lib.rs",
            "sed -n '40,60p' src/lib.rs",
            "git status --short",
            "wc -l src/lib.rs",
            "cargo test",
            "cd integrations/rig && cargo test",
            "git status 2>&1 | head -5",
            "git check-ignore -v integrations/rig",
            "at-peek search 'fn' src",
            "at-recall state",
            "cargo test > /tmp/log 2>&1",
            "direnv exec . cargo test > /tmp/all.log",
        ] {
            assert_eq!(refusal_for(command), None, "`{command}`");
        }
    }

    #[test]
    fn a_write_is_refused() {
        for command in [
            "sed -i 's/a/b/' src/lib.rs",
            "rm -rf target",
            "git checkout -- src/lib.rs",
            "echo x > src/lib.rs",
            "echo 'rm -rf /' > /tmp/x.sh",
            "cargo test > out.log",
            "at-peek search 'fn' src && rm -rf target",
        ] {
            let message = refusal_for(command).unwrap_or_else(|| panic!("`{command}` was allowed"));
            assert!(message.contains("writes a file"), "`{command}`: {message}");
        }
    }

    #[test]
    fn an_unclassifiable_command_is_refused() {
        for command in ["./build.sh", "$CMD foo", "rg 'unbalanced"] {
            let message = refusal_for(command).unwrap_or_else(|| panic!("`{command}` was allowed"));
            assert!(
                message.contains("could not be parsed") || message.contains("cannot classify"),
                "`{command}`: {message}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_map_to_the_tool_that_replaces_them() {
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
        // `wc` has no redirect, so it still reaches the Gate.
        let message = refusal("wc -l src/lib.rs").expect("a read");
        assert_eq!(
            message,
            "`wc` is a read the toolkit replaces · call `at_peek_stat`. It answers the same \
             question with a stated bound, so a retry through `shell` gains nothing; appending to \
             the command does not change what it is judged on."
        );
    }

    #[test]
    fn a_redirected_read_is_not_refused() {
        // The shape is answered in Prepare, so the Gate must leave it alone; a refusal here would
        // be a second answer to a call that already has one.
        for command in [
            "rg -n 'render_claim' .",
            "grep -rn foo src",
            "cat src/lib.rs",
            "sed -n '40,60p' src/lib.rs",
            "head -20 Cargo.toml",
            "git status --short",
            "git log --oneline -5",
            "git diff --stat",
        ] {
            assert_eq!(refusal(command), None, "`{command}`");
        }
        assert!(refusal("git diff --cached").is_some());
    }

    #[test]
    fn a_command_that_is_not_a_read_has_no_refusal() {
        assert_eq!(refusal("cargo test"), None);
        assert_eq!(refusal("git commit -m 'x'"), None);
        assert_eq!(refusal("node scripts/build.mjs"), None);
    }
}
