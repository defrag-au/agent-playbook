# The scenario corpus

One JSON file per scenario. A scenario is a prompt, the working directory it runs in, and the
checks the answer is scored or reported against. [`rig-eval`](../src/main.rs) loads every `*.json` here, validates
it against a snapshot of this repository, and runs it under every target.

The corpus is removed from every commit in a run's snapshot and denied at runtime, so an agent
cannot read its own gold. Adding or editing a file changes the corpus fingerprint recorded in
`manifest.json`.

## Shape

```json
{
  "name": "trace-redirect",
  "why": "the thread the scenario is cut from",
  "prompt": "…",
  "workdir": ".",
  "gold": ["integrations/rig/src/redirect.rs:119"],
  "extra_paths": ["integrations/rig/src/redirect.rs"],
  "must_say": ["at_peek_search"],
  "tool_says": ["agent_playbook.search"],
  "git": "status"
}
```

| Field | Required | What it is |
| --- | --- | --- |
| `name` | yes | The scenario's id; `--scenario` selects it. Unique in the corpus. |
| `why` | no | Where the question came from, printed by `--dry-run`. |
| `prompt` | yes | The task, verbatim, as one user message. |
| `workdir` | no | A directory inside the snapshot; `"."` by default. |
| `gold` | no | `path:line` citations the answer must contain. |
| `extra_paths` | no | Paths the answer must name, where a line is not required. |
| `must_say` | no | Literals the answer must contain. |
| `tool_says` | no | Literals the tool output must contain — evidence the work happened, reported separately and **not scored**. |
| `git` | no | `"status"`: the branch and changed paths are read from the snapshot at load time. |

A scenario needs at least one **scored** check; tool evidence alone is not enough. A file with no
gold, paths, literals or git expectation is an error. Every `gold` line must exist in the snapshot
— a stale line fails validation rather than scoring as a model miss. Every `extra_paths` entry must
exist.

## How a check is scored or reported

- **Gold.** A citation matches when its path is the same path (or one ends with `/` plus the other,
  so `src/lib.rs:82` matches `integrations/rig/src/lib.rs:82`) and its line, or the range it is
  part of, contains the gold line.
- **Path.** Named if the answer contains it, cites a matching path, or names a directory ending in
  `/` that contains it. `git status` collapsing an untracked tree to its top directory counts.
- **Answer.** A literal substring of the answer.
- **Tool.** A literal substring of the concatenated tool output, where each result is its tool name
  and its text. This is **evidence, not score**: it says the mechanism ran. It is reported in the
  run's tool-evidence table; it never changes the pass rate or `Cost/solved`.

A scenario whose expectation comes from the working tree uses `"git": "status"`: the runner reads
the branch and every changed path from the snapshot's `git status --branch -uall`. The snapshot
always has one uncommitted file (`docs/design/rig-eval.md`), so the answer is deterministic.

## Contamination

- The corpus is absent from the snapshot's working tree and from every commit reachable in its
  history; the branch commits survive for scenarios that name them.
- The macOS seatbelt profile denies reads of the real corpus directory, the output directory and
  the real checkout's `.git` to the agent.
- A tool call that names the corpus path is tagged `InspectedCorpus`; a tool output that contains a
  scored result (`"gold": [` or `"checks": [` on a line) is tagged `Contaminated` and excluded from
  every table and median.

## The scenarios

| File | Asks |
| --- | --- |
| `recover-investigation` | What the rig integration is building, what is built, what is still unmeasured |
| `trace-redirect` | How a shell `rg`/`grep` call is answered in process |
| `draft-pr` | Summarise the branch for a pull request |
| `devshell-tests` | Run the plugin's tests and report the totals |
| `pre-edit-state` | What is dirty, and on which branch |
| `rig-vs-factotum` | Whether rig replaces factotum, and where the boundary is |
| `otel-or-duckdb` | Whether OTel is required, and what DuckDB answers first |
| `eval-contamination` | How the gold is sealed from the agent |
| `facts-without-subprocess` | How a read reaches the agent without a CLI subprocess |
| `redirect-declines` | Which shell habits the redirect serves, and which it refuses to guess at |
| `advertising-over-gating` | Why the redirect replaced the refusal |
| `zed-thread-tools-not-at` | Why the thread tools sit beside the `at-*` toolkit |
| `the-skipped-rule` | The secrets rule when the default search hides the file that states it |

## Adding a scenario

1. Copy the nearest existing file; keep the prompt a real question, not a description of the check.
2. Compute the gold with the tools, not from memory: the line must be the sentence or definition
   the answer should cite, and it must exist in the snapshot.
3. `cargo run --manifest-path integrations/rig-eval/Cargo.toml -- --out .tmp/dry --dry-run` — this
   validates every scenario and prints the plan without spending anything.
4. Run one target once before adding the scenario to a published corpus; a check that no harness can
   hit is a corpus bug, not a model failure.

## Versioning

A published run names `corpus.fingerprint` and each file's `git hash-object` in `manifest.json`.
Editing any file here creates a new corpus: the next run's results are a different experiment, and
the report must say so.
