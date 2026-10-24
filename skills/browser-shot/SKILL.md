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
direnv exec . cargo run --manifest-path ~/code/defrag/agent-playbook/Cargo.toml -p browser-shot -- \
  "<url>" 390 844 .tmp/shot.png --settle-ms 4000
```

Run it from the target repo's root: `out` paths and `.tmp/` are relative to the current directory.
The first invocation builds `~/code/defrag/agent-playbook/target/debug/browser-shot`; call that
binary directly afterwards. Every run writes `<out>.png` and `<out>.txt`.

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

## Single shot

```sh
direnv exec . cargo run --manifest-path ~/code/defrag/agent-playbook/Cargo.toml -p browser-shot -- \
  "http://127.0.0.1:8095/?nav=0#/wallet-button" 390 844 .tmp/wallet-390.png \
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
- **`file://` cannot exercise network emulation.** Use a local server for HTTP/WebSocket checks.
- **`--select` is a CSS selector**, not a frame-aware locator; cross-origin iframes are not
  traversed.
- **The browser is Brave on macOS by default**; `--browser` or a peer's `browser=` picks another.

Full reference: `~/code/defrag/agent-playbook/tools/browser-shot/README.md`, the DSL in
`SCENARIO.md` beside it, and the contract in
`~/code/defrag/agent-playbook/docs/browser-automation.md`.
