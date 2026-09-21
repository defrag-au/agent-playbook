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
| `include` | Path prefixes to add. Only needed for `activation: manual` rules |
| `exclude` | Rule ids or path prefixes to drop |
| `emphasis` | Rule ids repeated verbatim in a `## Non-negotiable` preamble |
| `addenda` | Files in `addenda/`, appended after all rules |
| `default_file` | What `install.sh` writes into when `--file` is not given |

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

## Model overlays make claims

`emphasis` and any behavioural addendum are assertions about a specific model. They should
carry a date and be revised when observed behaviour changes — an overlay written for one
model release and never revisited is a rule that silently stops being true. The
`# last reviewed:` line in each conf is the reminder.

Do not write an overlay from reputation. If you have not watched a model fail in a particular
way, you do not have a tweak for it — you have a guess, and a guess in this directory costs
tokens on every single turn.
