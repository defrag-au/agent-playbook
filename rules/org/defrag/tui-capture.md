---
id: defrag-tui-capture
title: Never drive a terminal UI with tmux — use tui-shot or the crate's harness
layer: org
activation: org:defrag
priority: 48
overrides:
targets:
---

## Directive

**Never drive a TUI with tmux.** `tmux` is a generic executor: approving it approves `send-keys`
into any pane and `attach` to sessions it did not start, so it cannot be granted per-prefix.

| Want | Use |
| --- | --- |
| a live screen, keys, a resize, a narrow terminal | `tui-shot` — one PTY, one workspace binary target, no shell |
| a frame while iterating on the renderer | the crate's own `TestBackend` harness |
| the second check | `tui-shot`'s `<out>.json` · the harness's `run.json` |

```sh
tui-shot run --scenario .tmp/factotum.dsl
tui-shot check --scenario .tmp/factotum.dsl      # parse only, launch nothing
./target/release/<tui> --state fixtures/x.json --plan examples/x.dsl
```

- One plan file drives both: `tui-shot` through the PTY, the harness against a captured view. The
  grammar is `tools/tui-shot/SCENARIO.md` in `agent-playbook`.
- A harness-only field (`at=`, `status=`) is refused by `tui-shot` by name — omit it in a plan
  that is meant to run under both.
- Wait for content, never sleep: `wait … contains=`, `absent=`, `idle=`, `exit=true`.
- Not: a hand-started tmux session, `send-keys`, `capture-pane`, or a fixed `sleep` between a key
  and a screen read.

## Why

The factotum agent thread drove `factotum-tui` with `tmux new-session`, `send-keys`, a sleep, then
`capture-pane` for every state. The loop's speed was the sleeps, and every approval covered an
executor that can type into any pane on the machine.

Measured on 2026-10-26 against the same TUI: a `tui-shot` run is 650–1100 ms depending on how many
waits it has; five captures through the crate's `TestBackend` harness are 2 ms in total, at
180–280 µs a frame. The harness needs no daemon, so a renderer can be reviewed before the service
it talks to exists.

`tui-shot` refuses a path and a shell command by construction; the harness refuses to write
outside the repository. That is why either can be approved where tmux cannot.
