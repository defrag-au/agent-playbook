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
step       = wait | eval | key | pointer | capture | burst | assert | network
           | spawn | wait_for | goto ;
wait       = "wait" peer-list { field } ;
eval       = "eval" peer expression { flag } { field } ;
key        = "key" peer code state { field } ;
pointer    = "pointer" peer kind { field } ;
capture    = "capture" peer { field } ;
burst      = "burst" peer { field } ;
assert     = "assert" peer expression { field } ;
network    = "network" peer { field } ;
spawn      = "spawn" name { field } ;
wait_for   = "wait_for" { field } ;
goto       = "goto" peer url ;

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
| `args="--use-fake-ui-for-media-stream …"` | extra browser flags for this process, space-separated; `--user-data-dir`, `--remote-debugging-port` and `--headless` are rejected because the run sets them |
| `network=latency:80,loss:2` | HTTP/WebSocket emulation; accepts `latency`, `download`, `upload`, `loss`, `jitter` (`jitter` is rejected because CDP cannot apply it) |
| `loss_hook=rate:0.02,seed:1` | in-page `RTCDataChannel.send` drop hook |

`wait` — one condition is required; at least one peer is required.

| Field | Meaning |
| --- | --- |
| `predicate="…"` | wait until the expression is truthy |
| `frames=60` | wait for N frames of the peer's counter |
| `rate=30` | wait until the page sustains at least this many frames per second over a one-second sample |
| `network_idle=500` | wait for no in-flight requests for this window |
| `timeout=15s` | override the run timeout |

A failed `rate` wait names the rate it last measured and fails the peer, so a scenario can require
30 fps rather than asserting a frame count. The sidecar's `frame rate` line uses the same
measurement and, when it cannot take one, names the precondition that failed — for example
`not measured: frame counter "window.__frames" is not a finite number`.

`eval`

| Field | Meaning |
| --- | --- |
| `await` / `await=true` | await the returned promise |
| `capture=name` | store the result for `${name}` in later expressions |
| `out=samples.json` | write the value, JSON-serialised, under the artifacts directory |
| `timeout=5s` | bound an awaited promise; requires `await` |

An awaited expression is bounded by this timeout, or by the run/step timeout when it is omitted. The
bound is a page-side `Promise.race`, so a promise that never settles fails the step rather than
hanging the run. A rejected promise or a thrown exception fails the step too, naming the page error.

`out=` is a measurement artefact, not the interpolation binding: `capture=` keeps a value in memory
for `${name}`, `out=` writes a file. The value is serialised in the page and written exactly as the
page produced it, so nested arrays and objects and fractional numbers survive. A `Map`, a cyclic
object, a `BigInt`, a function, `undefined` or a non-finite number fails the step, naming the type
and path (`cannot encode a cyclic object at $.self`). The file is written when the step runs, so a
later assertion failure (exit 2) does not discard it. `run.json`'s `outputs` lists the files
written.

`${name}` is substituted in `eval`, `assert` **and `wait predicate=`** expressions, and the value
arrives as a JSON literal — so a captured number becomes a **string**. Compare it as one, or coerce
it: `window.__bt.received > Number(${recvBefore}) + 20`. The common shape this exists for is "wait
until this counter has moved past the value it had before the step", which needs the substitution
in the predicate and not only in the assertion after it.

Every tool-side event carries a wall-clock epoch millisecond timestamp: `input` and pointer
events, `eval`, the `ready` predicates and `burst` (`t=1750.123`), and `captured` is ISO with
milliseconds. A page timestamping itself with `performance.timeOrigin + performance.now()` can be
ordered against them directly.

`key` — `peer`, `code`, `down|up` are positional.

| Field | Meaning |
| --- | --- |
| `hold=12` | on a down step, wait twelve frames before the key-up |

`pointer` — `peer` and `kind` are positional. Kinds: `click`, `hover`, `drag`, `tap`, `swipe`.

| Field | Meaning |
| --- | --- |
| `selector=canvas` | resolve the target from the DOM |
| `at=200,400` | use explicit viewport coordinates |
| `steps=8` | interpolate the move (or drag/swipe) over N steps |
| `button=left` | `left`, `right` or `middle`; belongs on `click` and `drag` |
| `from=60,200` | start of a `drag` or `swipe`; defaults to the last pointer position |

A `drag` is press, interpolated moves, release — not a `dragstart`/`drop` pair. `tap` and
`swipe` dispatch `Input.dispatchTouchEvent` and require `touch` on the peer, because a page binds
touch handlers only when touch is available. Held modifiers (from `key ShiftLeft down` etc.) are
applied to pointer events; the sidecar records the frame each pointer event landed on.

`capture`

| Field | Meaning |
| --- | --- |
| `out=a.png` | output image, relative to the artifact directory |
| `select=canvas` | capture the element instead of the viewport |

`burst` — `out` and `frames` are required. Writes `<out>-000.png`, `<out>-001.png`, … plus a
`<out>.txt` report; the step's result is the frame count and rate.

| Field | Meaning |
| --- | --- |
| `out=frames/pulse` | output prefix, relative to the artifact directory |
| `frames=120` | number of frames to take |
| `mode=capture` | sequential `Page.captureScreenshot` — precise, but ~7.5 fps at 390x844 because each CDP call costs up to 100 ms |
| `mode=screencast` | `Page.startScreencast` — frame-paced, ~59 fps, suited to animation review |
| `interval=16ms` | in `capture` mode, a minimum gap between frames |
| `select=canvas` | in `capture` mode, capture the element instead of the viewport |

`screencast` captures the viewport, so combining it with `select` is a validation error. Each
frame is a real image with its own timestamp; the report gives interval, capture and write times
as min/median/max and a per-frame table.

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

`spawn` — the name is positional. Starts a process the run owns.

| Field | Meaning |
| --- | --- |
| `command="…"` | required; one shell command, run as `sh -c` |
| `port=8787` | the port this process listens on — enables the preflight and `wait_for ready=` |
| `dir=…` | working directory; defaults to where the tool was run |
| `env="A=1, B=2"` | comma-separated entries added to the child's environment |

The process starts in its own session, so every descendant joins its process group and one `killpg`
reaches the tree: `wrangler dev` is a script, a node CLI and `workerd`, and only the last of those
holds the port. The group is terminated when the run ends, however it ends — success, an assertion
failure, an error, SIGINT or SIGTERM. **SIGKILL or a crash still leaves it running**; the report
names the pgid, so `kill -- -<pgid>` cleans it up.

`port=` is checked by binding it before the launch. A port somebody else holds is refused:

```text
spawn "second": port 8792 is already in use — stop that process, or give this one a different port
```

That check cannot live in `wait_for`: by then a server that failed to bind has already died and the
squatter is what answers, so the wait would pass against the wrong build.

stdout and stderr go to `<artifacts>/<name>.log`, truncated at the start of the run. A process that
dies without being claimed by `wait_for exit=` stops the run at the next step, with its status and
the last lines of its log.

**A scenario with a `spawn` line runs a command, so treat the file like a script** — one you would
read before running, the way you would a `Makefile`, not a config file from someone else.

`wait_for` — exactly one of the five variants.

| Variant | Waits until |
| --- | --- |
| `ready=game` | the port that spawn declared accepts a connection |
| `port=8787` | any TCP accept on `host=` (default `127.0.0.1`) at that port |
| `http="http://…"` | a GET answers `status=` (default 200) |
| `log=game pattern="…"` | the log contains that substring, case-sensitive, in its first 256 KB |
| `exit=job code=0` | that process ends, with that status (default 0) |

| Field | Meaning |
| --- | --- |
| `host=127.0.0.1` | with `port=` |
| `status=200` | with `http=` |
| `pattern="…"` | with `log=`; required |
| `code=0` | with `exit=` |
| `timeout=120s` | how long the condition may take; default 30 s |

Every variant fails immediately if the process it names exits while the wait is in progress, and the
problem carries the last twenty lines of its log. Unlike `wait`, **a failed `wait_for` stops the
scenario**: everything after it assumed the condition held. It records a `problem` in `run.txt` and
the run exits 2.

`http=` accepts `http://` only. There is no TLS client in this tool, and silently not verifying a
certificate would be worse than saying so.

`goto` — `peer` and `url` are positional. Navigates the peer and waits for the load event.

```text
spawn site command="python3 -m http.server 8791" port=8791
wait_for ready=site
goto page "http://127.0.0.1:8791/page.html"
```

A peer fetches its `url=` before any step runs, so this is how a page served by a process the
scenario started gets loaded.

## Examples

### Pointer, touch and modifiers

```text
peer p url="file:///tmp/game.html" viewport=390x844

wait p frames=5
pointer p drag from=60,200 at=200,320 button=left steps=4
key p ShiftLeft down
pointer p click at=120,100
key p ShiftLeft up
capture p out=pointer.png

peer t url="file:///tmp/game.html" viewport=390x844 touch
pointer t tap at=150,150
pointer t swipe from=100,400 at=200,500 steps=5
```

A right click opens a context menu in the browser, and the page's frame counter is throttled while
it is open. Put a right click after any frame-timed input, or have the page call
`preventDefault()` on `contextmenu`.

### Reviewing an animation

`screencast` is the mode for judging motion: it asks the browser for every frame it paints, so the
interval is the page's frame pacing rather than the CDP round trip. Take a burst, then read the
frames as a sequence rather than one end state.

```text
run timeout=20s

peer anim url="file:///tmp/animation.html" viewport=390x844

wait anim frames=10
burst anim out=frames/pulse frames=120 mode=screencast
```

Measured on this machine at 390x844: `mode=screencast` gives ~58.8 fps with intervals of 15, 16
and 22 ms, while `mode=capture` gives ~130 ms per frame (~7.5 fps). Use `capture` when the image
matters more than the timing, and `screencast` when it is the other way around. A burst's report
is printed as a `burst` line in the sidecar and summarised in `run.json`.

### Recording a series

A run that measures something writes the samples, not a boolean. `out=` is relative to the
artifacts directory and survives an assertion failure:

```text
run timeout=30s

peer anim url="file:///tmp/game.html" viewport=390x844

wait anim frames=10
eval anim "window.__btSamples" out=samples.json
assert anim "window.__btSamples.length > 0" message="the page produced samples"
```

Each sample keeps its fractional precision; `run.json` lists `outputs: ["…/samples.json"]`.

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
