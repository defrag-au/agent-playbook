//! How the toolkit advertises itself to the model.
//!
//! The catalogue says what a verb is for; this module says why a model should reach for the native
//! tool instead of the shell command it already knows. Each description is three sentences — the
//! question, the typed call, and what the shell does not give — and [`TOOL_MAP`] is the same list at
//! prompt level, so a model that reads only the system prompt still knows the seven names.
//!
//! The question comes from the verb's catalogue entry; the call and the advantage are here because
//! they are about this harness's tools, not the CLI's grammar.

use at_peek::catalogue::Verb;
use rig_harness::prelude::*;

struct Advert {
    tool: &'static str,
    call: &'static str,
    advantage: &'static str,
}

const ADVERTS: &[Advert] = &[
    Advert {
        tool: "at_peek_search",
        call:
            "Call with `pattern` and optional `paths`; `count` or `files_only` changes the shape, \
               and `summary` gives the count without the rows.",
        advantage: "Unlike `rg` piped into `head`, it walks the whole root, counts every file it \
                    considers, and states the true total and the directories it skipped.",
    },
    Advert {
        tool: "at_peek_slice",
        call: "Call with one or more `path:range` targets — `40-60`, or `40+20` for twenty lines \
               from 40.",
        advantage: "Unlike `sed -n`, every answer states the file's true line count and names the \
                    range that continues it.",
    },
    Advert {
        tool: "at_peek_stat",
        call: "Call with one or more `paths`.",
        advantage: "Unlike `wc -l` and `ls -l` together, one call returns lines, bytes, kind and \
                    modification time for each path.",
    },
    Advert {
        tool: "at_peek_outline",
        call: "Call with one or more `paths`; `limit` bounds the headings across the call.",
        advantage: "Unlike `grep -n '^#'`, it ignores fenced code blocks, numbers every heading, \
                    and states the file's true line count.",
    },
    Advert {
        tool: "at_recall_state",
        call: "Call with no arguments; `summary` gives the frame without the rows.",
        advantage:
            "Unlike `git status` + `git branch` + `git log -1`, one call returns the branch, \
                    HEAD, upstream, any merge or rebase in progress, and every changed path.",
    },
    Advert {
        tool: "at_recall_log",
        call: "Call with an optional `rev` and `paths`; paths filter the history.",
        advantage:
            "Unlike `git log --oneline | head`, the count under the listing is the true total \
                    for the same revision and paths.",
    },
    Advert {
        tool: "at_recall_diff",
        call: "Call with an optional `rev` and `paths`; `patch` adds the hunks and `ignore_space` \
               compares without whitespace.",
        advantage:
            "Unlike `git diff`, the default answer is a bounded per-file table with totals, \
                    and a secret-shaped path is withheld from the hunks by name.",
    },
    Advert {
        tool: "at_recall_pr",
        call: "Call with an optional `base` and `with` (`commits`, `diffstat`, `areas`).",
        advantage: "Unlike a chain of `merge-base`, `log` and `diff --stat`, it returns the whole \
                    branch in one read and says when tracked work is not in any commit.",
    },
];

/// The description an advertised arm shows: the catalogue question, the typed call, and what the
/// shell does not give. `None` when the tool has no advert, so the caller can fall back to the
/// catalogue's own description rather than show a worse one.
pub fn describe(tool: &str, verb: Option<&'static Verb>) -> Option<String> {
    let advert = ADVERTS.iter().find(|advert| advert.tool == tool)?;
    let question = verb?.question;
    Some(format!("{question}. {} {}", advert.call, advert.advantage))
}

/// The prompt-level map. Names every tool, the question it answers, and the shell command it
/// replaces, so a model that has not read a description still knows the set.
pub const TOOL_MAP: &str = concat!(
    "The playbook toolkit answers most read questions without a shell. Each tool states what it left out, and the counting tools state the true total:\n",
    "- at_peek_search — every mention, with the true total (instead of `rg | head`).\n",
    "- at_peek_slice — exact lines, the file's real size, and the range that continues (instead of `sed -n`).\n",
    "- at_peek_stat — lines, bytes, kind and modification time (instead of `wc -l` and `ls -l`).\n",
    "- at_peek_outline — every heading with its line number and the file's true line count (instead of `grep -n '^#'`).\n",
    "- at_recall_state — branch, HEAD, upstream, any operation in progress and changed paths (instead of `git status`).\n",
    "- at_recall_log — commits with the true total (instead of `git log --oneline | head`).\n",
    "- at_recall_diff — a bounded per-file table, `patch` for the hunks (instead of `git diff`).\n",
    "- at_recall_pr — commits, diffstat and areas for the branch in one read (instead of `merge-base`, `log` and `diff --stat`).\n",
    "A read-only shell command is still allowed, but it cannot say what it did not show. Prefer the tool when a count, a total or the next range matters.",
);

/// Add the map to every agent's system prompt. A static section, so it costs the prompt cache once.
pub fn add_prompt(app: &mut App) {
    app.world_mut().spawn((
        Name::new("prompt:playbook_tools"),
        PromptSection::new(PromptSection::ORDER_PROJECT + 1, "playbook_tools", TOOL_MAP),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_an_advertised_description() {
        for (tool, verb) in [
            ("at_peek_search", at_peek::catalogue::find("search")),
            ("at_peek_slice", at_peek::catalogue::find("slice")),
            ("at_peek_stat", at_peek::catalogue::find("stat")),
            ("at_peek_outline", at_peek::catalogue::find("outline")),
            ("at_recall_state", at_recall::catalogue::find("state")),
            ("at_recall_log", at_recall::catalogue::find("log")),
            ("at_recall_diff", at_recall::catalogue::find("diff")),
            ("at_recall_pr", at_recall::catalogue::find("pr")),
        ] {
            let text = describe(tool, verb).unwrap_or_else(|| panic!("`{tool}` has no advert"));
            let question = verb.expect("a catalogued verb").question;
            assert!(text.starts_with(question), "`{tool}`: {text}");
            assert!(text.contains("Call with"), "`{tool}`: {text}");
            assert!(text.contains("Unlike"), "`{tool}`: {text}");
        }
    }

    #[test]
    fn the_tool_map_names_every_tool() {
        for tool in [
            "at_peek_search",
            "at_peek_slice",
            "at_peek_stat",
            "at_peek_outline",
            "at_recall_state",
            "at_recall_log",
            "at_recall_diff",
            "at_recall_pr",
        ] {
            assert!(TOOL_MAP.contains(tool), "the map does not name `{tool}`");
        }
    }

    #[test]
    fn the_tool_map_is_a_list_not_one_line() {
        let lines: Vec<&str> = TOOL_MAP.lines().collect();
        assert!(lines.len() >= 9, "{TOOL_MAP}");
        for line in &lines[1..8] {
            assert!(line.starts_with("- at_"), "{line}");
        }
        let tools = lines
            .iter()
            .filter(|line| line.starts_with("- at_"))
            .count();
        assert_eq!(
            tools,
            ADVERTS.len(),
            "every advert needs a map line: {TOOL_MAP}"
        );
    }

    #[test]
    fn an_unknown_tool_falls_back_rather_than_inventing_a_description() {
        assert_eq!(
            describe("at_peek_tree", at_peek::catalogue::find("search")),
            None
        );
    }
}
