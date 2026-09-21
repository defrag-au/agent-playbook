# Zed mechanics

## Which instruction file is read

Zed uses **the first matching file** from this ordered list, at the worktree root:

```
.rules
.cursorrules
.windsurfrules
.clinerules
.github/copilot-instructions.md
AGENT.md
AGENTS.md
CLAUDE.md
GEMINI.md
```

Source: `https://zed.dev/docs/ai/instructions`, checked 2026-09-21.

Two consequences that matter here:

- **`AGENTS.md` outranks `CLAUDE.md`**, so a repo with both gets the `AGENTS.md` block and
  Zed never reads the `CLAUDE.md` one. That is the intended arrangement: `AGENTS.md` is
  canonical for Zed, `CLAUDE.md` is for Claude Code.
- **Anything above `AGENTS.md` in that list silently disables it.** A stray `.cursorrules`,
  `.rules` or `AGENT.md` (note: singular) and Zed reads that instead, with no error and no
  warning. `playbook install` and `playbook check` warn when they find one — see
  `instruction_files:` in `models/zed/overlay.conf`.

## Nested instruction files are not supported

Zed loads **one file, at the worktree root**. A nested `AGENTS.md` in a subdirectory is
ignored, regardless of which file is active.

This is not a configuration problem, and it is not something to work around:
`crates/agent/src/agent.rs` resolves the file with `entry_for_path(name)` against the
worktree root and takes only the first hit, and the field is typed
`WorktreeContext.rules_file: Option<RulesFileContext>` — one file by construction. The
behaviour is tracked as zed#60215 (July 2026, still open as of September 2026), where the
reporter notes that Codex CLI, Claude Code, Cursor and OpenCode all walk up from the active
file and concatenate, and Zed does not.

**So there is no path- or directory-scoped rule mechanism on Zed.** Area-of-focus guidance
has exactly one route: **skills**, which are loaded when the model invokes them or when I
invoke them by name. That means a domain rule either sits in the always-on block, or it
lives in a skill and is retrieved on a judgement call — there is no third option that gets
loaded automatically for a subtree.

Do not build on nested files, and do not write a rule that assumes one will be read.

## Personal instructions

`~/.config/zed/AGENTS.md` (Windows: `%APPDATA%\Zed\AGENTS.md`) is loaded as personal
instructions for **every** project. The docs say project instructions override personal
`AGENTS.md` when they conflict.

That makes it the place for rules that are true regardless of repository — but note the cost
model: personal and project instructions both load, so this reduces *duplication across
repos*, not the context a session pays for. Use it so a core rule is maintained in one place
rather than reinstalled in five.

**Unverified:** the docs' "when they conflict" does not say whether both files are loaded
with the project winning on a per-rule basis, or whether one displaces the other. Confirm
before relying on the merge, by putting a distinctive line in the personal file and checking
it survives alongside a project block.

## Rendering

- **Mermaid renders.** ` ```mermaid ` blocks are drawn as diagrams. Supported: flowchart,
  sequence, class, state, ER, gantt, pie, gitgraph, mindmap, timeline, quadrant, xy, journey.
  Anything else shows as a code block.
- **Inline HTML inside mermaid is not rendered** — it is better to skip the formatting than
  to write `<b>` and have it appear literally.
- **Images render from remote URLs, absolute paths, and paths relative to a workspace root.**
  This is what makes the screenshot workflow usable — write the screenshot into `.tmp/` in the
  repo and reference it.
- Do not include `%%{init}%%` directives or custom `classDef` in mermaid; the theme is
  supplied.

## Editing

Prefer the editor tools for edits, shell commands for reads and builds. Zed shows the diff
for an editor edit and does not for a `sed` — see
[`core/edit-via-editor-tools`](../../../rules/core/edit-via-editor-tools.md).
