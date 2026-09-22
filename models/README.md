# models/

Compose-time overlays. A **target** is whatever consumes the rules — in practice a
model+harness pair, sometimes just one of the two.

Each target is a directory with an `overlay.conf` and an optional `addenda/`.

| Target | Kind | Applies to |
| --- | --- | --- |
| `generic/` | baseline | Any target with no specific overlay. Emits rules unmodified |
| `claude-code/` | model + harness | Claude via the Claude Code harness |
| `deepseek-flash/` | model | DeepSeek Flash, whichever harness |
| `zed/` | harness | Zed's agent, whichever model. Composes *with* a model overlay |
| `zed-personal/` | harness | Zed's **personal** instructions file, `~/.config/zed/AGENTS.md`. Carries the `memory/` layer and **no rules** |

### The personal target

`zed-personal` writes `~/.config/zed/AGENTS.md`, which Zed loads for **every** project —
including repositories the playbook knows nothing about.

```sh
playbook install --project personal --target zed-personal --repo ~/.config/zed
playbook check   --project personal --target zed-personal --repo ~/.config/zed
```

It carries **no rules**, and that is the point. An agent reads the personal file *and* the
project's file, so the two are meant to be complementary, not overlapping. The universal rules
ship in each repository's block — a repo is bootstrapped with them and its `AGENTS.md` is
committed and shared — so repeating them here would be duplication with no reader. The
mechanism is `exclude_activation: always`.

What is left is the `memory/` layer: facts about the person and the machine that are true
regardless of repository. That is the content this file exists for.

Verified 2026-09-21 that both files load — see `models/zed/addenda/zed-mechanics.md`.
`the_personal_target_carries_no_rules_and_the_memory_layer` and
`repo_targets_do_not_carry_the_memory_layer` hold the line.

`zed/` and a model overlay are not mutually exclusive — a Zed session running DeepSeek Flash
wants both. Where that matters, the model overlay names the harness in `model:`/`harness:`
and the harness overlay carries the mechanics.

## `overlay.conf`

Flat `key: value`, same parser as rule frontmatter. Blank lines and `#` comments are ignored.

```conf
target: claude-code
model: claude
harness: claude-code
include:
exclude:
emphasis: working-first, never-make-things-up, test-preservation
addenda: no-privately-preamble.md, tool-names.md
default_file: CLAUDE.md
# last reviewed: 2026-09-21
```

| Key | Effect |
| --- | --- |
| `title` | The block's heading. Defaults to `Agent rules`; the personal target uses `Personal instructions` |
| `include` | Path prefixes to add. Only needed for `activation: manual` rules |
| `exclude` | Rule ids or path prefixes to drop |
| `exclude_activation` | Activation kinds to drop, e.g. `always` — for a target whose rules are delivered somewhere else |
| `emphasis` | Rule ids repeated verbatim in a `## Non-negotiable` preamble |
| `addenda` | Files in `models/<target>/addenda/`, appended after the memory layer |
| `memory` | Files in `memory/`, emitted after the rules and before the addenda |
| `default_file` | What `playbook install` writes into when `--file` is not given |
| `instruction_files` | The harness's instruction-file priority order, most significant first. Used to warn when an existing file outranks the one being written |

Full semantics in [`../docs/precedence.md`](../docs/precedence.md).

## What belongs in an addendum

Harness mechanics. Things that are true of the *tool*, not of how I want work done:

- The names of the tools an agent has, and which to reach for
- How a harness phrases its nudges, and what it actually means
- What a sandbox refuses, and the invocation that works instead
- How output is rendered — what the editor supports, what it silently drops

## What does not

- **Behavioural rules.** Those go in `rules/`, even when only one model needs them — put it
  in `rules/core/` with `targets: <that model>` so it is visible as a rule rather than
  hidden in an overlay.
- **Anything that contradicts a rule.** If a target needs to beat a project rule, the rule is
  wrong.
- **Org or project specifics.** `models/zed/` applies to every repo here.

## Install both targets, not one

A repo with only one instruction file is read through whichever target generated it. Zed
reads the first match at the worktree root, so a `CLAUDE.md`-only repo gives **Zed** the
`claude-code` target's addenda — which name Claude Code's tools — and a `AGENTS.md`-only repo
leaves Claude Code with nothing at all.

```sh
playbook install --project shared-crates --target zed         --repo <repo>   # AGENTS.md
playbook install --project shared-crates --target claude-code --repo <repo>   # CLAUDE.md
```

Two blocks is not duplication to be avoided: each is generated from the same rule set, and
`playbook check` verifies both, so they cannot drift. The hand-maintained duplication this
repository exists to remove was two files that nothing kept in sync.

## Model overlays make claims

`emphasis` and any behavioural addendum are assertions about a specific model. They should
carry a date and be revised when observed behaviour changes — an overlay written for one
model release and never revisited is a rule that silently stops being true. The
`# last reviewed:` line in each conf is the reminder.

Do not write an overlay from reputation. If you have not watched a model fail in a particular
way, you do not have a tweak for it — you have a guess, and a guess in this directory costs
tokens on every single turn.
