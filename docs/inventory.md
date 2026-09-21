# Inventory — what came from where

Every rule in this repository exists because something went wrong without it. This file
records the source and the incident, so that a rule's prose can be read as a *conclusion* and
this can be read as the argument for keeping it.

Written 2026-09-21, when the corpus below was consolidated. Nothing has been installed into a
consuming repository yet — see *Not yet done* at the end.

## Sources

| Source | What it was | Destination |
| --- | --- | --- |
| `~/.claude/CLAUDE.md` | Global Claude memory | Split: universal rules → `rules/core/`, Claude quirks → `models/claude-code/` |
| `~/.agents/skills/*` (8 files) | Standing constraints filed as skills | `rules/core/` (5), `rules/rust/` (1), `skills/` (1), superseded (1) |
| `~/.claude/crate-refs/*.md` (2 files) | Crate cheat sheets | `references/crates/` |
| `cnft.dev-workers/CLAUDE.md` | Per-repo notes | `projects/cnft-dev-workers/`, `rules/org/defrag/`, `rules/rust/` |
| `cnft.dev-workers/AGENTS.md` | The same notes, plus devshell knowledge | Superseded by the above |
| `shared-crates/CLAUDE.md` | Per-repo notes | `projects/shared-crates/`, `rules/org/defrag/`, `rules/rust/` |
| `shared-crates/AGENTS.md` | The same notes, plus devshell knowledge | Superseded by the above |
| `shared-crates/ui/egui-widgets/CLAUDE.md` | Widget library notes | `rules/org/defrag/egui-icons-only`, `references/` |
| `archivist/CLAUDE.md` | Per-repo notes | `projects/archivist/` |

## The eight mis-filed skills

All eight carried `description: (no description)` and `disable-model-invocation: true`. A
skill that cannot be described cannot be discovered, and one that cannot be invoked cannot
fire — so none of them was ever loaded by anything. They were rules wearing a skill's file
layout.

| Skill | Went to | Why |
| --- | --- | --- |
| `the-working-first-engineering-rule` | `rules/core/working-first` | Constrains every response |
| `never-make-things-up` | `rules/core/never-make-things-up` | Constrains every response |
| `dont-rush-new-features` | `rules/core/dont-rush-new-features` | Constrains every response |
| `conservative-package-changes` | `rules/core/conservative-package-changes` | Constrains every response |
| `build-often` | `rules/rust/build-often` | Constrains every Rust response |
| `mcp-constraint-rule-ban-serdejsonvalue` | `rules/core/typed-json-only` | Constrains every response |
| `perform-useful-crate-research` | `skills/crate-research` | Genuinely procedural — has a trigger and steps, and now a real `description` |
| `rust-environment-verification` | **Superseded** — see below | |

### `rust-environment-verification` was actively wrong

It said:

> Always verify Rust toolchain availability with `source ~/.bash_profile && cargo --version`
> … Never attempt to install Rust toolchain automatically.

The second sentence is still right and is preserved in `rules/rust/devshell-first`. The first
is now wrong twice over: it picks up an **unpinned** toolchain rather than the devshell's, and
it makes the failure mode depend on whether the shell was warm. It is the exact advice that
`devshell-first` exists to replace.

It is recorded rather than deleted, in
`projects/archivist/rules/devshell-commands.md` under "The advice this replaced", because the
symptom it was written for still occurs: in a sandbox, `nix develop` fails with
`Operation not permitted`, which *reads* like a missing environment, and a shell profile is
the first place to look. The correct answer to that symptom is `direnv exec .`, not a profile.

## From `~/.claude/CLAUDE.md`

| Item | Destination |
| --- | --- |
| "Never begin a response with 'Privately,'" | `models/claude-code/addenda/no-privately-preamble` — a harness nudge, not a universal rule |
| Planning stays in thinking | `rules/core/planning-stays-in-thinking` — generalised; the Claude nudge is cited as the worked example |
| Git is the user's domain | `rules/core/git-is-the-users-domain` |
| Edit files with editor tools, never `sed`/`python` | `rules/core/edit-via-editor-tools` — including the incident that motivated it |
| Inline format args | `rules/rust/inline-format-args` |
| Never `serde_json::json!` | `rules/core/typed-json-only` |
| Macroquad / wasm-bindgen runtime pairs | `rules/org/defrag/runtime-pairs` |
| `~/.claude/crate-refs/` pointer | `models/claude-code/addenda/tool-names` + `references/crates/` |
| Black Flag dark-mode rules | **Not migrated** — see below |
| "Check the most recent file in `~/Desktop` for screenshots" | `memory/preferences.md` |

## Conflicts found, and how they were resolved

**1. `cnft.dev-workers` disagreed with itself about the toolchain.** `CLAUDE.md` said lint with
`cargo clippy --fix --allow-dirty --all-targets --all-features -- -D warnings`;
`AGENTS.md` beside it said `nix develop -c cargo clippy …`. Both are read by agents and only
one is correct outside a devshell. Resolved by `rules/rust/devshell-first` (the wrapper) plus
`rules/rust/tooling-handles-grunt-work` (the `--fix` mode) — the two facts were never in
conflict, they were just written into two files that had no way to know about each other.

**2. Rules at the wrong layer.** Several items appeared in a repo file but were true
ecosystem-wide (`patch-blocks`, the widget catalogue, the theme tokens). Applying the layer
test — *is this true of more than one repo?* — moved them up to `rules/org/defrag/`.

**3. Rules at the wrong layer in the other direction.** `d1-write-economics` and
`wasm-safe-serde` came from `cnft.dev-workers` and would have been false positives on
`shared-crates`, which uses neither D1 nor third-party JSON. They were moved down to
`projects/cnft-dev-workers/rules/`, each with a note saying when to move them back up.

**4. The override that wasn't needed.** An override was drafted for `archivist`'s devshell
rule, then deleted: `archivist` agrees with `rules/rust/devshell-first` and needs no override,
only an addition. Shipping an override to demonstrate the mechanism would have been a second
copy of a rule that was already correct. No rule in the tree overrides another today.

## Not migrated

- **Black Flag dark-mode rules.** `/Users/damo/code/defrag/blackflag/CLAUDE.md` is not in this
  Zed workspace, so its contents were not read. It is a candidate for `projects/blackflag/`
  when that repo is added. The pointer rule in the global memory is a symptom of the problem
  this repository solves — one agent file telling an agent to go and read a different one.
- **`shared-crates/ui/_storybook-egui` conventions.** The screenshot workflow became
  `skills/widget-screenshot`; the registration detail became a project rule. The storybook's
  own `CLAUDE.md` conventions beyond that were not read.

## Not yet done

- **Nothing is installed.** No consuming repository has a managed block yet. `playbook check`
  against `shared-crates`, `cnft.dev-workers` and `archivist` currently reports "no
  agent-playbook block", which is correct but means the old files are still the live
  instructions.
- **The old files still exist.** `AGENTS.md` and `CLAUDE.md` in the consuming repos are
  untouched. When a block is installed, the repo's own file should be reduced to what is
  genuinely local — and in the two repos where `AGENTS.md` and `CLAUDE.md` duplicate each
  other, one should become a pointer.
- **The Zed workspace instructions still carry the old per-project rules.** They were the
  source material for `projects/*/project.conf` and are now redundant with it.
- **The `deepseek-flash` overlay is provisional.** It was written from the shape of the rule
  set, not from observed failures. See `models/deepseek-flash/overlay.conf`.
