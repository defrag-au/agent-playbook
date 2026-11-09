# rig-playbook

The agent-playbook, as a rig plugin. Two halves, both on the kernel's public API:

- **Seven `at-*` verbs as native tools.** Typed arguments, a schema derived from the argument type,
  a `Reads` footprint, and the agent's own directory as the root. Descriptions come from the verbs'
  catalogue, so the text the model reads and the grammar the parser enforces cannot drift.
- **One of them reads in process.** `at_peek_search` calls `at_peek::verbs::search::collect` and
  publishes its facts, so the answer is typed rather than scraped from stdout. The other six still
  spawn their binary until the toolkit split reaches them.
- **A Gate.** A `shell` call that is really a read is refused, and the refusal names the tool that
  replaces it.

**Status 2026-11-09.** Compiles against `rig-harness` 0.46.0 and Bevy 0.20; `cargo test` passes 17
tests, including one deterministic harness test (`tests/redirect.rs`: a scripted model asks for
`shell rg`, and the app is asserted without a provider). A generated agent builds with it
(`rig plugin check --build`, 26 plugins, no conflicting systems in a stage), from the local fork at
`~/code/defrag/rig` (`c8b20d3d7`). It has run headless on `deepseek/deepseek-flash` over this
repository:

- a search task: **one turn**, one `at_peek_search` call, the answer correct, and the facts
  (`total = 4`, `walked = 443`) present in `effects.jsonl`;
- a forced `shell rg`, **redirected**: answered in process by `at_peek_search` for **2 model calls
  and 1 tool call — the same cost as the ungated baseline** — with `agent_playbook.redirection`
  beside `agent_playbook.search` in the log and no `decided_by`;
- the same call under the earlier refusal: 3 model calls and 2 tool calls;
- a paired baseline with `ToolsOnlyPlugin` (same tools, no Gate): 2 model calls and 1 shell call.

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

The tools spawn `at-peek` and `at-recall`, so those must be on `PATH` — every defrag devshell has
them — or named by `AT_PEEK_BIN` and `AT_RECALL_BIN`. `at_peek_search` is the exception: it links
`at-peek` and reads in process, so it needs neither.

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

Each passes the agent's `CallPlace` base as `--root`, so a subagent with its own directory reads that
tree rather than the process's.

### The search answer is typed

`at_peek_search` builds `Opts` with the same `Opts::resolve` the CLI uses, calls
`at_peek::verbs::search::collect`, and returns `facts.report(&opts).render()` — the CLI's own bytes.
It also publishes the facts under `agent_playbook.search`, so `effects.jsonl` records the pattern,
the matching files, the true total and where the walk stopped, rather than a rendered string. The
same code serves the redirect: a redirected `rg` publishes `agent_playbook.redirection` naming the
command and the tool that answered it.

The other six verbs still spawn. Their typed reports are steps 3 and 4 in
[`docs/design/toolkit-core.md`](../../docs/design/toolkit-core.md); until then the spawn is honest,
because those tools have no library entry point that does not print.

## What happens to a shell read

A plain `rg`/`grep` is **redirected, not refused**: `Prepare` replaces the call's handler with
`at-peek search`, so the model gets the bounded answer as the shell's result and spends no call on
a refusal. The redirect is deliberately conservative — `rg [--line-number] PATTERN [PATH…]` and
`grep -r[n] PATTERN [PATH…]`, with no pipeline, redirection or answer-changing flag.

Everything else is refused by the Gate, naming the tool:

| Shell read | Replaced with |
| --- | --- |
| `cat`, `head`, `tail`, `sed -n` | `at_peek_slice` |
| `wc` | `at_peek_stat` |
| `git status` | `at_recall_state` |
| `git diff` | `at_recall_diff` |
| `git log` | `at_recall_log` |

Only the **first** command of a pipeline is judged, so `cargo test | grep FAIL` is left alone: it
filters a build's output rather than reading the working tree. `sed -i` is a write and is left alone
too. Anything not in the table — `git add`, `cargo build`, `node` — is untouched.

## The baseline

`rig_playbook::ToolsOnlyPlugin` registers the same seven tools and adds no Gate. Load it instead of
`PlaybookPlugin` in a second `RIG_HOME`, run the same prompt, and the effect logs give the cost of
enforcement directly. It exists for that comparison, not for daily use; the one-prompt result is
in [`docs/design/rig.md`](../../docs/design/rig.md), and the scenario runner and its first result
in [`integrations/rig-eval`](../rig-eval) and [`docs/design/rig-eval.md`](../../docs/design/rig-eval.md).

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
