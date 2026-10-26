# tui-shot

Drive a terminal UI in a PTY this tool owns, then write the screen and a typed report.

`tui-shot` replaces the `tmux new-session` / `send-keys` / `sleep` / `capture-pane` loop. It is the
sibling of `browser-shot`: it starts the process it drives, keeps no session, and attaches to
nothing.

## Why this exists

`tmux` is a generic executor. Approving the prefix approves `new-session <shell command>`,
`send-keys` into any pane (including one of yours), `source-file`, `run-shell`, and attaching to
sessions it did not start. `tui-shot`'s whole grammar is:

- a **binary target name** built in this workspace, resolved under `target/<profile>/`
- a size, key names, literal text, waits and output paths
- argv after `--`, passed directly to that one child

There is no command string, no shell, no socket, no attach, and no process that outlives the run.
The child is `exec`'d with `portable-pty` and killed by the handle that spawned it.

The one free-form field is the child's argv. It is never interpreted by a shell, and the child is
workspace code that `cargo build` already runs.

## Usage

Build first. `tui-shot` does not build, so a compile error stays a compile error.

```sh
direnv exec . cargo build -p factotum-tui
```

One capture:

```sh
tui-shot factotum-tui 120x42 .tmp/factotum \
  --wait-for "factotum" \
  --key i --type "second question" --key enter \
  --wait-for "second question" \
  -- --base ws://127.0.0.1:7100 --session s-1 --token demo
```

A scenario:

```sh
tui-shot run --scenario .tmp/factotum.dsl
tui-shot check --scenario .tmp/factotum.dsl
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

key app q
wait app exit=true
```

The grammar and every field are in [`SCENARIO.md`](SCENARIO.md).

## What a capture writes

`capture app out=01-arrival` writes two files under the artifacts directory:

| File | Contents |
| --- | --- |
| `01-arrival.txt` | the screen, right-trimmed. **This is the review.** |
| `01-arrival.json` | the typed report: program and sha256, size, cursor, alternate-screen, per-row lengths, styled runs, warnings, exit. |

`run` also writes `run.txt` and `run.json` for the whole scenario. A failed wait writes the screen
it saw as `<tui>-failure-<line>.txt` before failing.

The report is the second check, for what a glance misses:

- rows whose last cell is prose, not a border or a block — a candidate for a line cut at the edge
- each row's used columns; `rows[].full` marks a row that uses every column, which a border and a
  cut word both do
- wide characters at the final column
- control characters and bells
- a blank screen
- inverse/bold/colour runs that a plain text capture loses (a selection or a permission sheet)
- whether the child exited, and whether it exited while still on the alternate screen
- whether the screen was still changing when the capture was taken

## Waiting, not sleeping

`wait` conditions are `contains`, `absent`, `idle` and `exit=true`. A capture settles on a quiet
screen (default 40 ms) with a bound, so there is no fixed sleep in the loop. A wait that has held
for the settle window returns even if the app keeps drawing — the report then says it was still
changing.

## Sizes

`120x42` is **columns x rows**, the order `tmux -x … -y …` uses. `stty size` prints the reverse, so
the report names both fields explicitly.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | ran and every wait/assertion passed |
| `2` | ran, artefacts written, a wait or assertion failed |
| `1` | operational error — a binary is not built, a file cannot be written, the grammar is invalid |

## It does not

- attach to a session, or keep one
- run a shell, a command string, or a path outside this workspace's build output
- build the binary
- kill anything by name
- send mouse events, bracketed paste or scrollback; the capture is the visible screen and `key`/`type` are the input

## Build and test

```sh
direnv exec . cargo test -p tui-shot
direnv exec . cargo clippy -p tui-shot --all-targets -- -D warnings
```

The integration tests drive `tui-shot-demo`, a small framework-free TUI in `src/bin/`. It is built
with the crate and is not installed by the flake.

## Why this is not an `at-*` tool

Every `at-*` binary is read-only: no write path, and (except `at-recall`'s fixed `git` verbs) no
subprocess. `tui-shot` starts a process and writes captures, so it sits beside `browser-shot` as
its own package rather than inside a namespace whose promise is that the whole grammar is safe to
allowlist.
