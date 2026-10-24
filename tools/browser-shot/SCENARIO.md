# Scenario DSL

`run` and `check` both accept this line-oriented format, chosen by file extension. TOML remains
supported for existing scenarios. The syntax is still provisional; the grammar below is the
contract.

The point is not novelty. TOML's tagged union is a bag of optional fields, so it accepts a pointer
step with `hold_frames`, a network step with nothing to set, or a capture with both coordinates and
a selector; the runner only finds out at runtime. A grammar can reject those while the line is being
read, and can carry a line and column in the error.

## Grammar

```ebnf
file       = { line } ;
line       = blank | comment | run | peer | step ;
run        = "run" { field } ;
peer       = "peer" name { flag } { field } ;
step       = wait | eval | key | pointer | capture | assert | network ;
wait       = "wait" peer-list { field } ;
eval       = "eval" peer expression { flag } { field } ;
key        = "key" peer code state { field } ;
pointer    = "pointer" peer kind { field } ;
capture    = "capture" peer { field } ;
assert     = "assert" peer expression { field } ;
network    = "network" peer { field } ;

peer-list  = name { "," name } ;
field      = key "=" value ;
flag       = "touch" | "headed" | "await" ;
```

A line starts with the action. Every remaining token is either positional (`peer`, `KeyW`, `down`,
`click`) or a `key=value` field. Values may be single- or double-quoted; a quoted value may contain
spaces and `=`. Quote every expression that contains whitespace or `=`. A `#` at the start of a
token begins a comment; a `#` inside an unquoted token is kept, so a URL fragment is safe.

Durations are milliseconds by default; `ms` and `s` suffixes are accepted (`timeout=15s`,
`network_idle=500ms`). Bare numbers are milliseconds.

## Fields

`run`

| Field | Meaning |
| --- | --- |
| `timeout` | default step timeout |
| `artifacts` | artifact directory |

`peer` — `url` and `viewport` are required.

| Field | Meaning |
| --- | --- |
| `url="…"` | page URL |
| `viewport=390x844` | emulated viewport |
| `dpr=3` | device scale factor |
| `touch` / `touch=true` | touch emulation and mobile metrics |
| `user_agent="…"` | UA override |
| `headed` / `headed=true` | run with a window |
| `browser=/path` | per-peer browser executable |
| `preload=/path` | script before the document's own scripts |
| `frame_counter="window.__frames"` | counter used by frames and holds |
| `network=latency:80,loss:2` | HTTP/WebSocket emulation; accepts `latency`, `download`, `upload`, `loss`, `jitter` (`jitter` is rejected because CDP cannot apply it) |
| `loss_hook=rate:0.02,seed:1` | in-page `RTCDataChannel.send` drop hook |

`wait` — one condition is required; at least one peer is required.

| Field | Meaning |
| --- | --- |
| `predicate="…"` | wait until the expression is truthy |
| `frames=60` | wait for N frames of the peer's counter |
| `network_idle=500` | wait for no in-flight requests for this window |
| `timeout=15s` | override the run timeout |

`eval`

| Field | Meaning |
| --- | --- |
| `await` / `await=true` | await the returned promise |
| `capture=name` | store the result for `${name}` in later expressions |

`key` — `peer`, `code`, `down|up` are positional.

| Field | Meaning |
| --- | --- |
| `hold=12` | on a down step, wait twelve frames before the key-up |

`pointer` — `peer` and `click|hover` are positional.

| Field | Meaning |
| --- | --- |
| `selector=canvas` | resolve the target from the DOM |
| `at=200,400` | use explicit viewport coordinates |
| `steps=8` | interpolate the move over N steps |

`capture`

| Field | Meaning |
| --- | --- |
| `out=a.png` | output image, relative to the artifact directory |
| `select=canvas` | capture the element instead of the viewport |

`assert`

| Field | Meaning |
| --- | --- |
| `message="…"` | label for the check |

`network` — at least one field is required.

| Field | Meaning |
| --- | --- |
| `latency=80` | added latency |
| `download=500` | download limit in kbps |
| `upload=500` | upload limit in kbps |
| `loss=2` | packet loss percentage |

## Examples

The keys scenario:

```text
run timeout=20s

peer focused url="file:///tmp/keys.html" viewport=390x844
peer unfocused url="file:///tmp/keys.html" viewport=390x844

pointer focused click selector="#game"
key focused KeyW down hold=12
assert focused "window.__keys[1].frame - window.__keys[0].frame === 12" message="held twelve frames"
assert focused "window.__keys[0].focus === 'game'" message="the canvas held focus"
key unfocused ArrowDown down hold=1
assert unfocused "window.__keys[0].focus === 'BODY'" message="an unfocused canvas receives nothing"
assert unfocused "window.scrollY > 0" message="arrow keys scrolled the document"
capture focused out=focused.png
capture unfocused out=unfocused.png
```

The two-peer shape:

```text
run timeout=30s

peer a url="file:///tmp/webrtc.html" viewport=390x844 network=latency:80,loss:2 loss_hook=rate:0.0,seed:1
peer b url="file:///tmp/webrtc.html" viewport=390x844

eval a "window.__createOffer()" await capture=offer
eval b "window.__acceptOffer(${offer})" await capture=answer
eval a "window.__acceptAnswer(${answer})" await
wait a,b predicate="window.__channelState === 'open'" timeout=15s
wait a,b frames=60

eval a "window.__reset(); window.__lossHook.rate = 0.02"
eval b "window.__reset()"
eval a "window.__send(100)"
wait a predicate="window.__lossHook && window.__lossHook.sent === 100" timeout=15s
wait b frames=30
assert b "window.__missing() > 0" message="the injected drop produced a gap"
capture a out=peer-a.png
capture b out=peer-b.png
```

## Run, check or format it locally

```sh
./target/debug/browser-shot run --scenario .tmp/keys.dsl --artifacts .tmp/keys-run
./target/debug/browser-shot check --scenario .tmp/keys.dsl
./target/debug/browser-shot check --scenario .tmp/keys.toml --json
./target/debug/browser-shot fmt --scenario .tmp/keys.dsl            # print formatted
./target/debug/browser-shot fmt --scenario .tmp/keys.dsl --write    # rewrite in place
./target/debug/browser-shot fmt --scenario .tmp/keys.dsl --check    # exit 2 if it would change
```

`fmt` preserves comments and blank-line grouping, orders fields canonically, and quotes values
containing `#` so the tree-sitter grammar can treat `#` as a comment unambiguously. It is a syntax
operation: it can lay out a half-written scenario before its peers exist, so run `check` for
semantic validation. It formats the DSL only; TOML is not rewritten. The tree-sitter grammar and
highlights are in [`tree-sitter-scenario/`](tree-sitter-scenario/README.md).

`check` parses the file, validates peer references and every action's required fields, and prints a
summary. Exit codes are `0` valid, `2` invalid, `1` an operational failure. A parse error names the
line and column; a semantic error names the line and the step.

The grammar is deliberately line-oriented; `tree-sitter-scenario/grammar.js` is the same grammar
for editors, and the formatter's canonical output is what it parses. The Rust parser here remains
the semantic source of truth; an LSP that offers peer names and action fields is the next step.
