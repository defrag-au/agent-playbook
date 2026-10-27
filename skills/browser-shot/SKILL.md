---
name: browser-shot
description: Use when you need a screenshot, a scripted browser scenario, or a frame-by-frame animation capture from a web page — a widget story at a narrow width, a macroquad web build, a deployed page, or a two-peer WebRTC check. Runs browser-shot, which owns the browser process, and reads its text sidecar.
argument-hint: <url or scenario file, e.g. "http://127.0.0.1:8095/?nav=0#/wallet-button">
---

# browser-shot

Browser automation without a hand-launched browser. `browser-shot` starts its own browser process
per peer, keeps the profile under `.tmp/browser-shot-<pid>`, and removes it on exit. It does not
need a remote-debugging port, a warm profile, or a sleep.

## Run it from the repo that should own the artefacts

```sh
browser-shot "<url>" 390 844 .tmp/shot.png --settle-ms 4000
```

Run it from the target repo's root: `out` paths and `.tmp/` are relative to the current directory.
`browser-shot` is on `PATH` in every defrag devshell — agent-playbook packages it and `defrag-nix`
wires it into the base set — so there is nothing to build first. Every run writes `<out>.png` and
`<out>.txt`.

Never start Brave yourself, reuse a debugging port, or kill a browser by name — the tool owns the
process it started.

## Pick the shape

| Job | Command |
| --- | --- |
| One page, one image | the single-shot form above |
| Clicks, keys, waits, assertions, two peers | `run --scenario <file.dsl> --artifacts .tmp/<run>` |
| Check a scenario without launching | `check --scenario <file.dsl>` |
| Lay out a scenario | `fmt --scenario <file.dsl> --write` (or `--check`) |
| Animation quality | `burst <peer> out=frames/<name> frames=120 mode=screencast` |
| Verify a frame rate | `wait <peer> rate=30` |
| A series over time (latency samples etc.) | `eval <peer> "<expression>" out=<file.json>` |
| Start the server it tests | `spawn …`, then `wait_for …`, then `goto` |

## Single shot

```sh
browser-shot "http://127.0.0.1:8095/?nav=0#/wallet-button" 390 844 .tmp/wallet-390.png \
  --dpr 3 --touch --settle-ms 6000
```

- `--select CSS` captures the element; without it the viewport is captured.
- Width and height are positional. **`--window-size` is not emulation** and cannot produce a
  narrow shot.
- `--ready EXPR`, `--frames N` and `--network-idle` are the readiness forms; `--settle-ms` is a
  floor under the frame-stability loop. **`--ready` only works on a page that provides the
  expression.** The egui storybook and the macroquad clients do not set `window.__ready` — use
  `--settle-ms` there, and `--no-settle` for a screen that never stops animating.
- `--touch` enables touch emulation and mobile metrics. Touch *input* is `tap`/`swipe` in a
  scenario.
- Exit `0` passed, `2` artefact written but an assertion failed, `1` operational. Exit 2 is not a
  pass.
- A page that never finishes navigating (crashed renderer, hung main thread, requests that never
  end) still exits `2` with a sidecar: its `result` line reads `FAIL — navigation crashed: …` or
  `FAIL — navigation timed out: …`, and the console, exceptions and requests collected before that
  are in it. Read the sidecar before reaching for a hand-launched browser.
- Navigation completes at `networkAlmostIdle` or at `load` followed by 500 ms with no request in
  flight, whichever comes first; the sidecar's `ready` line names which. An open `EventSource` or
  long poll keeps a page off the second path. Completing navigation is not readiness: `--ready`
  still decides that.

For a macroquad web build, capture the canvas at real device pixels:
`--select canvas --dpr 3 --touch --settle-ms 5000 --no-settle`, then read `image` and `layout` in
the sidecar. A game that sets `window.__ready` can use `--ready` instead; the gamekit's native
`Game::ready` is not visible to the browser.

## Scenarios

The line-oriented DSL is the field reference in
`~/code/defrag/agent-playbook/tools/browser-shot/SCENARIO.md`. A two-peer shape:

```text
run timeout=30s

peer a url="http://127.0.0.1:8095/peer" viewport=390x844 network=latency:80,loss:2
peer b url="http://127.0.0.1:8095/peer" viewport=390x844

eval a "window.__createOffer()" await capture=offer
eval b "window.__acceptOffer(${offer})" await capture=answer
eval a "window.__acceptAnswer(${answer})" await
wait a,b predicate="window.__channelState === 'open'" timeout=15s
pointer a click selector="canvas"
key a KeyW down hold=12
assert b "window.__received === 100" message="all messages arrive"
capture a out=a.png select=canvas
```

Each peer is a separate browser process with its own storage and WebRTC stack. `check` validates
peer names and required fields and names the line; `fmt` only lays the file out.

## Starting the server it tests

A scenario can run the origin it captures. Starting and being ready are separate steps because they
fail differently:

```text
peer page url=about:blank viewport=390x844

spawn site command="direnv exec . wrangler dev --env dev --port 8787 --ip 127.0.0.1" \
      port=8787 dir=games/island-funk/worker
wait_for ready=site timeout=120s
goto page "http://127.0.0.1:8787/?offline=1"
```

- `spawn` owns a **process group**, because one logical server is usually several processes and only
  the last of them holds the port. The group is killed when the run ends, however it ends — success,
  an assertion failure, an error or Ctrl-C. stdout and stderr go to `<artifacts>/<name>.log`.
- `port=` is checked by binding it before launch, so a stale server from an earlier run is refused
  rather than silently tested.
- `wait_for` variants: `ready=<name>` (the port that spawn declared), `port=`, `http=`, `log=<name>
  pattern=`, `exit=<name> code=`.
- **A failed `wait_for` stops the scenario**, unlike `wait`. Everything after it assumed the condition
  held. It shows up as a `problem` in `run.txt`, with the last twenty lines of that process's log.
- A process that dies on its own stops the run at the next step, with its status and log tail.

A scenario containing `spawn` executes a command: treat the file as a script, not as config from a
stranger.

Acceptance runs for the feature, and what each should exit with, are listed in
`tools/browser-shot/README.md` under *Starting what the scenario tests*.

## Animation bursts

```text
peer anim url="file:///tmp/animation.html" viewport=390x844
wait anim frames=10
burst anim out=frames/pulse frames=120 mode=screencast
```

`mode=screencast` is frame-paced by the browser (~59 fps at 390x844) and is the mode for judging
motion. `mode=capture` takes sequential screenshots and is precise but limited to ~7.5 fps by the
CDP round trip. A burst writes `<out>-000.png`, `<out>-001.png`, … and `<out>.txt` with interval,
capture and write times per frame. Look at the frames as a sequence; the report says what the
browser did.

## A series over time

```text
peer anim url="file:///tmp/game.html" viewport=390x844
wait anim frames=10
eval anim "window.__btSamples" out=samples.json
```

`eval … out=` writes the value as JSON under the artifacts directory, at the precision the page
produced. It is separate from `capture=`, which only binds a value for `${name}` interpolation.
A `Map`, a cyclic object, a `BigInt`, a function, `undefined` or a non-finite number fails the
step with the type and path rather than being silently stringified. The file survives a later
assertion failure (exit 2), and `run.json`'s `outputs` lists the files written.

Every tool-side event carries fractional epoch milliseconds: `input` and pointer events, `eval`,
each `ready` predicate and `burst` have `t=1750.123`, and `captured` is ISO with milliseconds. A
page timestamping itself with `performance.timeOrigin + performance.now()` joins directly.

## Read the sidecar before reporting

The image is the review; `<out>.txt` is the second check. It leads with what a glance misses:

- `warnings` — software rasteriser, DPR mismatch, clipping, a capture with no readiness wait.
- `renderer … SOFTWARE` — headless WebGL may be SwiftShader; do not claim it is what a GPU drew.
- `layout` vs `viewport` — a page without a viewport meta lays out at `980` wide and warns.
- `image` and `css box` — compare CSS pixels with device pixels.
- `frame rate` — the measured rate, or `not measured: …` naming the counter or clock that failed.
  A rate under 20 Hz carries a warning that the capture is not representative; use `--headed`
  for animation or a two-peer run.
- `input` — every event is stamped against the page's frame counter.
- `network` — the emulation says `HTTP/WebSocket only, NOT WebRTC`.

## Traps

- **A right click opens a context menu, and the page's frame counter is throttled while it is open.**
  Put a right click after frame-timed input, or `preventDefault()` on `contextmenu`.
- **`screencast` captures the viewport only**; `select` with it is a validation error.
- **Every CDP call costs ~100 ms on this transport**, so a frame hold is scheduled before the
  keydown. Use `hold=N` frames, not a millisecond sleep.
- **A peer fetches its `url=` before any step runs.** A page served by a process the scenario
  started needs `goto <peer> <url>` after the wait; pointing `peer url=` at it just shows a
  connection error.
- **`spawn` declares the port; `wait_for` waits on it.** The port cannot be checked at the wait,
  because by then a server that failed to bind has died and whatever squats the port answers — the
  wait would pass against the wrong build.
- **`file://` cannot exercise network emulation.** Use a local server for HTTP/WebSocket checks.
- **`--select` is a CSS selector**, not a frame-aware locator; cross-origin iframes are not
  traversed.
- **The browser is Brave on macOS by default**; `--browser` or a peer's `browser=` picks another.

Full reference: `~/code/defrag/agent-playbook/tools/browser-shot/README.md`, the DSL in
`SCENARIO.md` beside it, and the contract in
`~/code/defrag/agent-playbook/docs/browser-automation.md`.
