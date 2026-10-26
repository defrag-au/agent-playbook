---
name: tui-shot
description: Use when you need to see or drive a terminal UI — capture a TUI screen, script key presses, wait for content, or check a narrow terminal — without tmux. Runs tui-shot, which owns the PTY, and reads the text screen plus its typed sidecar.
argument-hint: <binary target or scenario file, e.g. "factotum-tui">
---

# tui-shot

Terminal captures without a hand-started tmux session. `tui-shot` starts one child in a PTY it
owns, drives it with keys and resizes, and writes the screen plus a report. It does not attach to
an existing terminal, keep a session, or run a shell.

**Never use tmux for this.** `tmux` is a generic executor: approving it approves a command string,
`send-keys` into any pane, and attaching to sessions it did not start. `tui-shot` runs one binary
target built in this workspace and nothing else.

## Build first

`tui-shot` does not build. A stale binary is a stale capture, and a compile error hidden behind a
run is worse than a failed build.

```sh
direnv exec . cargo build -p <crate>
```

`tui-shot` is packaged by agent-playbook: `nix build ~/code/defrag/agent-playbook#tui-shot`. Once
agent-playbook is re-locked into `defrag-nix` it is on `PATH` in every defrag devshell; until then
run `~/code/defrag/agent-playbook/result/bin/tui-shot` from the repository you are capturing.

## Run it from the repo that owns the artefacts

```sh
tui-shot factotum-tui 120x42 .tmp/factotum \
  --wait-for "factotum" \
  --key i --type "second question" --key enter \
  --wait-for "second question" \
  -- --base ws://127.0.0.1:7100 --session s-1 --token demo
```

Everything after `--` goes to the child's argv. The capture writes `.tmp/factotum.txt` (the
screen) and `.tmp/factotum.json` (the report).

## A scenario, when there is more than one moment

```sh
tui-shot run --scenario .tmp/factotum.dsl
tui-shot check --scenario .tmp/factotum.dsl   # parse and resolve, launch nothing
```

```text
run artifacts=.tmp/tui/factotum settle=40ms

tui app bin=factotum-tui size=120x42 args="--base ws://127.0.0.1:7100 --session s-1 --token demo"

wait app contains="factotum"
capture app out=01-arrival

key app i
type app "second question"
key app enter
wait app contains="second question"
capture app out=02-sent

key app 1
wait app contains="deploy"
capture app out=03-permission

resize app 80x24
wait app idle=80ms
capture app out=04-narrow
assert app contains="draft"

key app q
wait app exit=true
```

The grammar and fields are in `tools/tui-shot/SCENARIO.md`.

## Read both files

**The `.txt` is the review.** It is the screen, right-trimmed.

The `.json` is the second check, for what a glance misses:

| Field | What it catches |
| --- | --- |
| `warnings` | wide char at the edge, control characters, a bell, a blank screen, still-changing |
| `styles` | inverse/bold/colour runs that plain text loses — a selected row, a permission sheet |
| `edge_rows` | rows whose last cell is prose, not a border — a candidate for text cut at the edge |
| `screen.alternate` | the app never switched to the alternate screen, or never left it |
| `exit` | the child died before the capture, and how |
| `program.sha256` | the capture is attributable to one build |

A successful run with a warning is not a clean run. Read the warning and decide.

## Wait, never sleep

`wait` conditions are `contains`, `absent`, `idle` and `exit=true`. There is no `sleep` in the DSL
and no fixed settle beyond the 40 ms quiet window. If the app keeps drawing, a wait that has held
for the settle window returns and the capture says `still changing`.

## Sizes and keys

- `120x42` is **columns x rows**. Narrow checks are real: `80x24`, `40x12`.
- Keys: `enter`, `esc`, `tab`, `up`/`down`/`left`/`right`, `pageup`/`pagedown`, `home`/`end`,
  `ctrl-c`, `alt-x`, or one character.
- `type` sends literal text; `key … enter` sends the newline.
- `resize app 80x24` changes the PTY and the app is notified.

## Exit codes

`0` pass · `2` artifacts written but a wait or assertion failed · `1` operational error. Exit `2`
is not a pass, and the failed wait's screen is written as `<tui>-failure-<line>.txt`.

## Do not

- start tmux, or reuse a session
- run a shell command as the child; pass a binary target name and its argv after `--`
- kill the child by name — `tui-shot` owns it and kills it
- claim a screen rendered without reading the `.txt`
