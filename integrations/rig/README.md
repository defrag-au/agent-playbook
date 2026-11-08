# rig-playbook

The agent-playbook, as a rig plugin. Two halves, both on the kernel's public API:

- **Seven `at-*` verbs as native tools.** Typed arguments, a schema derived from the argument type,
  a `Reads` footprint, and the agent's own directory as the root. Descriptions come from the verbs'
  catalogue, so the text the model reads and the grammar the parser enforces cannot drift.
- **A Gate.** A `shell` call that is really a read is refused, and the refusal names the tool that
  replaces it.

**Status 2026-11-08.** Compiles against `rig-harness` 0.46.0 and Bevy 0.20; `cargo test` passes six
tests on the classifier, the argv mapping and the catalogue link. It has **not** been loaded by a
running agent yet — `rig` is not installed on this machine. Whether the model retries with the tool,
and whether that costs fewer turns than prose in `AGENTS.md`, is the experiment. Do not cite this as
working enforcement until it has run.

## Load it

```sh
mkdir -p ~/.rig/plugins
ln -s ~/code/defrag/agent-playbook/integrations/rig ~/.rig/plugins/rig-playbook
```

`~/.rig/settings.toml` (or the path under your `RIG_HOME`):

```toml
[[plugins.extra]]
crate = "rig-playbook"
path = "plugins/rig-playbook"
plugin = "rig_playbook::PlaybookPlugin"
```

`/reload` rebuilds and restarts with it; the next start also picks it up. `rig plugin list` shows it.

The tools spawn `at-peek` and `at-recall`, so those must be on `PATH` — every defrag devshell has
them — or named by `AT_PEEK_BIN` and `AT_RECALL_BIN`.

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

## What the Gate refuses

| Shell read | Replaced with |
| --- | --- |
| `rg`, `grep` | `at_peek_search` |
| `cat`, `head`, `tail`, `sed -n` | `at_peek_slice` |
| `wc` | `at_peek_stat` |
| `git status` | `at_recall_state` |
| `git diff` | `at_recall_diff` |
| `git log` | `at_recall_log` |

Only the **first** command of a pipeline is judged, so `cargo test | grep FAIL` is left alone: it
filters a build's output rather than reading the working tree. `sed -i` is a write and is left alone
too. Anything not in the table — `git add`, `cargo build`, `node` — is untouched.

## Build and test

The crate is its own workspace, on purpose: the playbook's composer builds with no dependencies and
`cargo test` at the repository root must not start building Bevy.

```sh
cd integrations/rig
direnv exec . cargo test
direnv exec . cargo clippy --all-targets -- -D warnings
```

The classifier and the argv mapping are pure and tested without a Bevy app. The tools and the Gate
are one `Tool` impl each and a query; the live run is their test.
