# Scenario DSL

`run` and `check` accept this line-oriented format. It is the same shape as `browser-shot`'s:
one action per line, positional names first, then `key=value` fields. It is parsed in full before
anything is launched, so an undeclared name or a capture outside the artifacts directory fails
while the line is being read.

## Grammar

```ebnf
file       = { line } ;
line       = blank | comment | run | tui | step ;
run        = "run" { field } ;
tui        = "tui" name { field } ;
step       = wait | key | type | resize | capture | assert ;
wait       = "wait" name { field } ;
key        = "key" name key-name ;
type       = "type" name value ;
resize     = "resize" name size ;
capture    = "capture" name "out=" value ;
assert     = "assert" name assertion { "message=" value } ;
assertion  = "contains=" value | "absent=" value | "size=" size ;
field      = key "=" value ;
```

A line starts with the action. Every remaining token is either positional (the TUI name, a key,
a size) or a `key=value` field. Values may be single- or double-quoted; a quoted value may contain
spaces and `=`. Quote every value that contains whitespace or `=`. A `#` at the start of a token
begins a comment; a `#` inside an unquoted token is kept, so a URL fragment is safe.

Escapes inside a quoted value: `\n`, `\t`, `\\`, `\"`, `\'`. An unknown escape is an error rather
than a literal backslash.

Durations are milliseconds by default; `ms` and `s` suffixes are accepted (`timeout=15s`,
`settle=40ms`). Bare integers only. Sizes are **columns x rows** (`120x42`), capped at `500x200`.

## Fields

`run`

| Field | Meaning |
| --- | --- |
| `timeout` | default bound on a single wait (default `10s`) |
| `settle` | how long the screen must be quiet before a wait or capture is satisfied (default `40ms`) |
| `artifacts` | artifact directory, relative to the working directory (default `.tmp/tui-shot`) |

`tui` — the name is positional, `bin` is required.

| Field | Meaning |
| --- | --- |
| `bin` | a binary target name built in this workspace. No path, no arguments. |
| `size` | `cols x rows` (default `120x40`) |
| `profile` | `debug` or `release` (default `debug`) |
| `args` | the child's argv, space-separated with shell-style quoting. No expansion, no globbing, no substitution. |
| `term` | `TERM` for the child (default `xterm-256color`) |
| `timeout` | override the run's wait bound for this TUI |

`wait` — one or more conditions, all of which must hold.

| Field | Meaning |
| --- | --- |
| `contains="…"` | the screen contains this literal text |
| `absent="…"` | the screen does not contain this literal text |
| `idle=80ms` | no output for this window |
| `exit=true` | the child has exited |
| `timeout` | override the run/TUI bound |

A wait is satisfied once its conditions hold **and** the screen has been quiet for `settle`, or
once the conditions have held for `settle` even if the child keeps drawing. The capture then says
it was still changing. A child that exits while a non-`exit` wait is outstanding fails the wait
immediately, names the condition that was not met, and writes the screen it saw.

`capture`

| Field | Meaning |
| --- | --- |
| `out=01-arrival` | capture stem, relative to the artifacts directory. `.txt` and `.json` are appended. |

A capture waits for a quiet screen (bounded by the run/TUI timeout) before taking it.

`resize`

```text
resize app 80x24
```

The PTY is resized and the child receives the terminal's resize notification. The next wait or
capture observes the new frame.

`key`

```text
key app enter
key app ctrl-c
key app 1
```

Key names are `enter`, `esc`, `tab`, `backspace`, `delete`, `insert`, `home`, `end`, `pageup`,
`pagedown`, `up`, `down`, `left`, `right`, `space`, `f1`–`f12`, `ctrl-a`–`ctrl-z` (plus `ctrl-@`,
`ctrl-[`, `ctrl-\`, `ctrl-]`, `ctrl-^`, `ctrl-_`, `ctrl-?`), `alt-<character>`, or one literal
character. A lone `esc` is written and followed by 60 ms so the child does not read it as the start
of a sequence.

`type`

```text
type app "second question"
```

Literal bytes to the child's stdin. Use `key … enter` for a newline.

`assert`

| Field | Meaning |
| --- | --- |
| `contains="…"` | the current screen contains this literal text |
| `absent="…"` | the current screen does not contain it |
| `size=120x42` | the server's view of the PTY size |
| `message="…"` | a label printed with a failure |

Exactly one of `contains`, `absent` or `size` is required. An assertion does not wait; put a
`wait` before it. A failed assertion does not stop the scenario, so a run can report every failed
check. It sets exit code `2`.

## Complete example

```text
run artifacts=.tmp/tui/factotum settle=40ms timeout=10s

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
capture app out=frames/04-narrow
assert app contains="draft" message="the composer stays visible at 80 columns"

key app q
wait app exit=true
```

`frames/04-narrow` creates `frames/` under the artifacts directory and writes
`frames/04-narrow.txt` plus `frames/04-narrow.json`.

## Artifact containment

The artifacts directory is created if needed, then canonicalised and required to be inside the
working directory. A `capture out=` path must be relative, must not contain `..`, and is written
under that directory. The tool refuses rather than writing outside the repository.

## Checking without running

```sh
tui-shot check --scenario .tmp/factotum.dsl
```

`check` parses and validates the whole file, resolves every declared binary, and reports whether
each one is built. It launches nothing and writes nothing. Exit `0` means valid and built, `2`
means a target is missing or not declared, `1` means the file itself could not be read or parsed.

There is no `fmt` verb yet. The grammar is small enough to lay out by hand, and the parser rejects
an unknown field rather than silently accepting it.
