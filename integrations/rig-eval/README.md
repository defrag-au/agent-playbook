# rig-eval

Scores the playbook's rig homes over real-work scenarios. It reads rig's `--json` stream, so it
needs no rig dependency; it is a separate package because a runner inside `integrations/rig` is
part of the thing the scenarios investigate.

The binary is split by job: `cli` (clap and the resolved paths), `scenario` (the corpus and its
validation), `stream` (the event stream and its facts), `score` (citations and checks), `sandbox`
(the seatbelt profile), `runner` (the process and one run) and `report` (the results and the
summary).

The design and the first result are in
[`docs/design/rig-eval.md`](../../docs/design/rig-eval.md).

## Run it

The corpus is [`scenarios/`](scenarios) in this crate, and `--scenarios` defaults to it. `--out` is
required: the streams and results are the artefact you keep.

```sh
cd ~/code/defrag/agent-playbook
cargo run --manifest-path integrations/rig-eval/Cargo.toml -- --out /tmp/rig-eval --dry-run
cargo run --manifest-path integrations/rig-eval/Cargo.toml -- --out /tmp/rig-eval
```

`--dry-run` validates every check, prints the plan and runs nothing. Add `--scenario <name>` to
filter, `--repeat <n>` for more runs, and `--home <label>=<path>` for another `RIG_HOME`. The homes
default to `.tmp/rig-home` (gated) and `.tmp/rig-home-baseline` (tools only); the model key is read
from `.tmp/rig-home/model.env` and never printed.

On macOS the runner wraps `rig` in `sandbox-exec` and denies the agent reads of the corpus, the
eval crate, the output directory and `.tmp/rig-eval`. A corpus inside the checkout is refused when
that sandbox is unavailable: the content is the answer key, and a model that reads it scores
itself. On another platform, copy `scenarios/` outside the checkout and pass `--scenarios`.

## A scenario

```json
{
  "name": "trace-redirect",
  "why": "the thread the scenario is cut from",
  "prompt": "…",
  "workdir": ".",
  "gold": ["path/to/file.rs:123"],
  "extra_paths": ["path/to/file"],
  "must_say": ["literal in the answer"],
  "tool_says": ["literal in the tool output"]
}
```

`gold` is `path:line`; `extra_paths`, `must_say` and `tool_says` are literals. Every check is
validated before a run, so a gold line past the end of its file is an error rather than a model
failure. The score is checks passed; precision is gold hits over all citations, recall is hits over
gold.

Each run keeps its raw stream, stderr, a copy of `effects.jsonl`, and the output directory gets
`results.json` and `summary.md`.

## Build and test

```sh
cd integrations/rig-eval
direnv exec . cargo test
direnv exec . cargo clippy --all-targets -- -D warnings
```
