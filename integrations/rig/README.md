# rig-playbook

The agent-playbook, as a rig plugin. Two halves, both on the kernel's public API:

- **Seven `at-*` verbs as native tools.** Typed arguments, a schema derived from the argument type,
  a `Reads` footprint, and the agent's own directory as the root. Descriptions come from the verbs'
  catalogue, so the text the model reads and the grammar the parser enforces cannot drift. Every
  tool calls its verb's `collect` in process and publishes the typed answer, so there is no `PATH`
  or subprocess dependency.
- **A shell policy, and a redirect.** A plain `rg`/`grep` call is answered in `Prepare` by
  `at-peek search`; every other shell command runs through the classifier, which permits reads and
  builds and refuses writes. `GatedPlugin` keeps the earlier read-refusal policy, because the
  measurement that retired it is worth reproducing.

**Status 2026-11-10.** Compiles against `rig-harness` 0.46.0 and Bevy 0.20; `cargo test` passes 23
tests, including two deterministic harness tests: one scripts a `shell rg` and asserts the redirect,
the other scripts all seven read tools and asserts each published fact. A generated agent builds
with it (`rig plugin check --build`, 26 plugins, no conflicting systems in a stage), from the local
fork at `~/code/defrag/rig`. It has run headless on `deepseek/deepseek-flash` over this repository,
and a live `pre-edit-state` run recorded `agent_playbook.state` (branch, HEAD, pending, changed
paths) in `effects.jsonl` alongside the text the model saw.

- a search task: **one turn**, one `at_peek_search` call, the answer correct, and the facts
  (`total = 4`, `walked = 443`) present in `effects.jsonl`;
- a forced `shell rg`, **redirected**: answered in process by `at_peek_search` for **2 model calls
  and 1 tool call — the same cost as the ungated baseline** — with `agent_playbook.redirection`
  beside `agent_playbook.search` in the log and no `decided_by`;
- the same call under the earlier refusal (`GatedPlugin`): 3 model calls and 2 tool calls;
- a paired baseline with `ToolsOnlyPlugin` (same tools, no policy): 2 model calls and 1 shell call.

What is still unmeasured is a prose-only baseline (no tools at all) on a task that needs the rules.
Do not cite this as a turn-count win until that run exists.

## Load it

Point `[[plugins.extra]]` at the checkout. **Do not symlink the crate into `plugins/`:** Cargo
resolves the plugin's own relative dependencies from the path it is given, so a link at
`RIG_HOME/plugins/rig-playbook` turns `../../tools/at-peek` into `RIG_HOME/tools/at-peek` and the
build fails. A path that walks to the real crate works — relative to `RIG_HOME`, or absolute.

`~/.rig/settings.toml` (or the path under your `RIG_HOME`):

```toml
[[plugins.extra]]
crate = "rig-playbook"
# relative to RIG_HOME; `~/.rig/../code/defrag/agent-playbook/integrations/rig` for the usual layout
path = "../code/defrag/agent-playbook/integrations/rig"
plugin = "rig_playbook::PlaybookPlugin"
```

Check it before starting an agent, which needs no model and no key:

```sh
rig plugin check          # settings and the extra crate are valid
rig plugin check --build  # generates the agent and compiles it with the plugin
```

`/reload` rebuilds and restarts with it; the next start also picks it up. `rig plugin list` shows it.

### The rig source

This crate tests against the fork at `~/code/defrag/rig` through its own `[patch.crates-io]`, and
the agent gets that same checkout because `rig` is installed from it:

```sh
cd ~/code/defrag/rig
cargo install --path crates/rig-cli --locked --force
```

The plugin's `[patch]` is ignored when it is built as a dependency; the generated project's
`[patch.crates-io]`, written by the launcher, supplies the checkout there. Both builds therefore
use one copy of every rig crate. Keep the fork's version at `0.46.0`: the plugin's runtime
requirement is `=0.46.0`, and the launcher's path patch must satisfy it.

The tools link the verb crates, so they need neither `PATH` nor an `AT_PEEK_BIN` override. That is
what makes the plugin self-contained: a call is a read in the agent's process, not a child whose
environment has to be found first.

## The tools

| Tool | Verb | What it answers |
| --- | --- | --- |
| `at_peek_search` | `at-peek search` | every mention |
| `at_peek_slice` | `at-peek slice` | these exact lines |
| `at_peek_stat` | `at-peek stat` | how big, and when it changed |
| `at_recall_state` | `at-recall state` | what am I looking at |
| `at_recall_log` | `at-recall log` | what changed lately |
| `at_recall_diff` | `at-recall diff` | what is the difference |
| `at_recall_pr` | `at-recall pr` | the facts a PR description is written from |

Each resolves the agent's `CallPlace` base as its root, so a subagent with its own directory reads that
tree rather than the process's.

### Every answer is typed

Each tool builds its `Opts` with the same constructor the CLI uses, calls the verb's `collect`, and
returns `facts.report(&opts).render()` — the CLI's own bytes. It also publishes the facts under its
own key, so `effects.jsonl` records what the call found rather than only the text the model was
shown:

| Tool | Published key | What the facts hold |
| --- | --- | --- |
| `at_peek_search` | `agent_playbook.search` | pattern, rows, counted files, true total, walk |
| `at_peek_slice` | `agent_playbook.slice` | each target's range, lines shown, file size |
| `at_peek_stat` | `agent_playbook.stat` | path, lines, bytes, kind, modified |
| `at_recall_state` | `agent_playbook.state` | branch, HEAD, upstream, pending, changes |
| `at_recall_log` | `agent_playbook.log` | the commits and their true total |
| `at_recall_diff` | `agent_playbook.diff` | per-file counts, totals, withheld names, hunks |
| `at_recall_pr` | `agent_playbook.pr` | base, commits, diffstat, area breakdown |

The redirect publishes `agent_playbook.redirection` too, naming the command and the tool that
answered it. The same code serves both modes, so a fact and a rendered line cannot disagree.

## What happens to a shell read

A plain `rg`/`grep` is **redirected, not refused**, in both `PlaybookPlugin` and `GatedPlugin`:
`Prepare` replaces the call's handler with `at-peek search`, so the model gets the bounded answer as
the shell's result and spends no call on a refusal. The redirect is deliberately conservative —
`rg [--line-number] PATTERN [PATH…]` and `grep -r[n] PATTERN [PATH…]`, with no pipeline,
redirection or answer-changing flag.

Under `GatedPlugin`, everything else that is really a read is refused, naming the tool:

| Shell read | Replaced with |
| --- | --- |
| `cat`, `head`, `tail`, `sed -n` | `at_peek_slice` |
| `wc` | `at_peek_stat` |
| `git status` | `at_recall_state` |
| `git diff` | `at_recall_diff` |
| `git log` | `at_recall_log` |

`PlaybookPlugin` does not refuse those. Its policy is the classifier: a read or a build runs, and a
write or an unclassifiable command is refused. Only the **first** command of a pipeline is judged,
so `cargo test | grep FAIL` is left alone: it filters a build's output rather than reading the
working tree. `sed -i` is a write and is left alone too. Anything not in the table — `git add`,
`cargo build`, `node` — is untouched.

## The baseline

`rig_playbook::ToolsOnlyPlugin` registers the same seven tools and adds no policy. `GatedPlugin`
is the earlier read-refusal policy. Load one instead of `PlaybookPlugin` in a second `RIG_HOME`, run
the same prompt, and the effect logs give the cost of enforcement directly. They exist for that
comparison, not for daily use; the one-prompt result is in [`docs/design/rig.md`](../../docs/design/rig.md),
and the scenario runner and its first result in [`integrations/rig-eval`](../rig-eval) and
[`docs/design/rig-eval.md`](../../docs/design/rig-eval.md).

## Build and test

The crate is its own workspace, on purpose: the playbook's composer builds with no dependencies and
`cargo test` at the repository root must not start building Bevy. It needs the rig fork checkout at
`../../../rig` (beside `agent-playbook`): `[patch.crates-io]` resolves `rig-harness` there, and
`rig-harness-test-support` is a dev-dependency on the same checkout.

```sh
cd integrations/rig
direnv exec . cargo test
direnv exec . cargo clippy --all-targets -- -D warnings
```

The classifier and the argv mapping are pure and tested without a Bevy app. `tests/redirect.rs`
builds the app with one scripted model turn and asserts what the harness did with it: the
conversation keeps the model's call, the result is the search, and the log names the substitution.
