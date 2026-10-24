# browser-shot

Browser automation for agents, implementing `docs/browser-automation.md` where a process needs it:
the widget storybook at compact width, a deployed-page check, a web game capture, and the two-peer
WebRTC overlay spike.

It launches its own browser processes, drives one page per process, and writes `<name>.png` +
`<name>.txt` (and the same data as typed JSON with `--json`). The crate is deliberately
self-contained — no paths into the rest of the repository — so it can move to a dedicated
`agent-toolbox` repository without changing its code.

## Usage

```sh
direnv exec . cargo run -p browser-shot -- \
  "http://127.0.0.1:8095/?nav=0#/wallet-button" 390 844 .tmp/wallet-390.png \
  --select canvas --dpr 3 --touch --ready "window.__ready === true"
```

```
browser-shot <url> <width> <height> <out.png> [options]

  --dpr N                 device scale factor (default 1)
  --touch                 enable touch emulation (with mobile metrics)
  --user-agent UA         override the user agent
  --select CSS            capture this element instead of the viewport
  --ready EXPR            wait until this JS expression is truthy
  --frames N              wait for N more frames of --frame-counter
  --frame-counter EXPR    frame counter expression (default window.__frames)
  --network-idle MS       wait for no in-flight requests for MS
  --preload PATH          run this JS before the document's own scripts
  --expect EXPR           assert this JS expression after readiness
  --click x,y             move there and click (repeatable)
  --hover x,y             move there without clicking (repeatable)
  --click-selector CSS    click the centre of this element (repeatable)
  --hover-selector CSS    hover the centre of this element (repeatable)
  --steps N               interpolate each pointer move over N steps (default 1)
  --settle-ms N           minimum observation window for frame stability (default 1200)
  --timeout-ms N          timeout for every wait (default 30000)
  --no-settle             skip the stability loop; capture once
  --headed                run with a window instead of headless
  --report PATH           sidecar path (default: the image with a .txt extension)
  --browser PATH          browser executable (default: Brave on macOS)
  --keep-profile          leave .tmp/browser-shot-<pid> behind
  --json                  print the typed report to stdout
  -h, --help              print this

Exit codes: 0 pass, 2 artifact written but an assertion failed, 1 operational error.
```

Exit 2 covers a failed readiness predicate, a frame-stability timeout, a DPR mismatch, a failed
`--expect`, an uncaught exception, or touch emulation that did not take. Console errors and
failed requests are reported but do not fail the run on their own.

## Scenarios: more than one peer

```sh
direnv exec . cargo run -p browser-shot -- \
  run --scenario .tmp/two-peer.dsl --artifacts .tmp/two-peer-run --json
```

Each peer is a separate browser process with its own profile, storage and WebRTC stack. Peers are
launched in parallel; a `wait` that names several peers dispatches to all of them concurrently.
The run writes `run.json` / `run.txt`, plus one image and sidecar per peer.

The line-oriented DSL is the preferred form; [`SCENARIO.md`](SCENARIO.md) is the grammar and field
reference. The older TOML form is still accepted, and this is the same scenario in TOML:

```toml
[run]
timeout_ms = 30000

[[peer]]
name = "a"
url = "http://127.0.0.1:8095/peer"
viewport = "390x844"
network = { latency_ms = 80, loss_percent = 2 }
loss_hook = { rate = 0.0, seed = 1 }

[[peer]]
name = "b"
url = "http://127.0.0.1:8095/peer"
viewport = "390x844"
dpr = 3
touch = true
user_agent = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1"
preload = ".tmp/instrument.js"

[[step]]
action = "wait"
peers = ["a", "b"]
predicate = "window.__ready === true"
timeout_ms = 15000

[[step]]
action = "eval"
peer = "a"
expression = "window.__createOffer()"
await_promise = true
capture = "offer"

[[step]]
action = "eval"
peer = "b"
expression = "window.__acceptOffer(${offer})"
await_promise = true

[[step]]
action = "key"
peer = "a"
code = "KeyW"
state = "down"
hold_frames = 12

[[step]]
action = "pointer"
peer = "b"
kind = "click"
selector = "canvas"

[[step]]
action = "network"
peer = "b"
latency_ms = 120
download_kbps = 500
loss_percent = 1

[[step]]
action = "capture"
peer = "a"
out = "a.png"
select = "canvas"

[[step]]
action = "assert"
peer = "a"
expression = "window.__frames > 0"
message = "the page kept ticking"
```

Steps (TOML field names; the DSL names are in `SCENARIO.md`):

| `action` | Fields |
| --- | --- |
| `wait` | `peer` or `peers`, and one or more of `predicate`, `frames`, `network_idle`; `timeout_ms` |
| `eval` | `peer`, `expression`, `await_promise`, `capture` (stores the result for `${name}` substitution) |
| `key` | `peer`, `code`, `state` (`down`/`up`), optional `hold_frames` on a down step |
| `pointer` | `peer`, `kind` (`click`/`hover`), `x`+`y` or `selector`, `steps` |
| `capture` | `peer`, `out`, optional `select` |
| `assert` | `peer`, `expression`, optional `message` |
| `network` | `peer`, and at least one of `latency_ms`, `download_kbps`, `upload_kbps`, `loss_percent` |

A key step with `hold_frames` sends down, waits on the page's counter, then sends up, all in one
step. Modifiers persist across steps until released. A failed `assert` sets exit 2 and names the
expression and its value.

## Scenario DSL

The compact line-oriented front-end is documented in [`SCENARIO.md`](SCENARIO.md). `run` and `check`
both accept it, chosen by extension; `.toml` remains supported for existing scenarios.

```text
run timeout=20s

peer focused url="file:///tmp/keys.html" viewport=390x844
peer unfocused url="file:///tmp/keys.html" viewport=390x844

pointer focused click selector="#game"
key focused KeyW down hold=12
assert focused "window.__keys[1].frame - window.__keys[0].frame === 12" message="held twelve frames"
capture focused out=focused.png
```

```sh
./target/debug/browser-shot run --scenario .tmp/keys.dsl --artifacts .tmp/keys-run
./target/debug/browser-shot check --scenario .tmp/keys.dsl
./target/debug/browser-shot check --scenario .tmp/keys.toml --json
./target/debug/browser-shot fmt --scenario .tmp/keys.dsl            # print formatted
./target/debug/browser-shot fmt --scenario .tmp/keys.dsl --write    # rewrite in place
./target/debug/browser-shot fmt --scenario .tmp/keys.dsl --check    # exit 2 if it would change
```

`run` and `check` validate peer references and every action's required fields; a parse error names
the line and column, and a semantic error names the line and step. `check` exits 0 valid, 2 invalid,
1 operational. `fmt` preserves comments, orders fields canonically, quotes values containing `#`
(so the tree-sitter grammar can treat `#` as a comment), and prints unless `--write` is given.

The tree-sitter grammar and highlight query live in
[`tree-sitter-scenario/`](tree-sitter-scenario/README.md). Generated files are checked in; run
`bunx tree-sitter-cli generate --js-runtime native` there after changing `grammar.js`.

## What the sidecar looks like

```
url          file:///…/browser-shot-canvas.html
captured     2026-10-24T05:47:58Z
peer         0 (of 1 process)
browser      Chrome/155.0.8059.85
pid          69541
mode         headless (new)
renderer     WebGL 2.0 (… SwiftShader …) — SOFTWARE
viewport     390x844 dpr 3 touch
image        1170x2532 (element canvas)
css box      390x844 at 0,0
ready        fonts.ready = loaded; window.__ready === true = true; frame.stable = 3 samples, 2 identical after 2343 ms
waited       3 predicates, 2681 ms
frame rate   59.3 Hz (64 -> 124 frames in 1011 ms)
console      0 errors, 0 warnings, 0 uncaught
requests     0 failed (1 total)
network      latency 80 ms, loss 2% — HTTP/WebSocket only, NOT WebRTC
overlay      drop 2.00% injected in send() (seed 1, 5/100 dropped)
input        KeyW down @ frame 42 · focus canvas#game · scroll none; KeyW up @ frame 54 (held 12 frames) · focus canvas#game · scroll none
focus        canvas#game
scroll       none
locator      canvas
expect       window.__ready === true = true (pass)
warnings     renderer is software: …
profile      .tmp/browser-run-94930/peer-0 (removed on exit)
screenshot   .tmp/canvas-390.png
result       pass
```

The last lines are the ones that change what an agent may say: the renderer, whether the network
emulation applied, whether the loss was injected, which input form was used, and what the page did
with it.

## Coverage

| Requirement | State |
| --- | --- |
| 1. N isolated browser processes, driven concurrently | **Covered.** One process and profile per peer; launches and shared waits run in parallel. Verified with two peers. |
| 2. Deterministic readiness, no fixed sleeps | **Covered.** `wait_until`, `frames` and `network_idle`; a timeout names the predicate and its last value. Frame rate is measured over one second and reported per peer. |
| 3. Element capture at real pixel dimensions | **Covered.** Reports the CSS box and PNG size; a DPR mismatch fails. |
| 4. True device emulation | **Covered.** Viewport, DPR, touch and UA through CDP; touch is verified via `navigator.maxTouchPoints`. |
| 5. Evaluate before the document's scripts | **Covered** with `--preload` and the per-peer `preload`. The loss hook installs before the document runs. |
| 6. Console, exceptions and failed requests | **Covered**, per peer, timestamped relative to the run. |
| 7. Network emulation, with honest scope | **Partial.** Latency, bandwidth and loss work for HTTP/WebSocket; the sidecar says `NOT WebRTC`. Jitter has no CDP knob and is rejected with an actionable error rather than silently ignored. |
| 8. In-page WebRTC loss hook | **Covered.** Wraps `RTCDataChannel.prototype.send` with a seeded PRNG; the rate can be changed at runtime; the sidecar labels it injected and reports drops/sends. |
| 9. Image + text sidecar | **Covered**, with typed JSON alongside it. |
| 10. Unique handle, PID, cleanup by handle | **Covered.** Profile at `.tmp/browser-run-<pid>/peer-N`, browser PID in the sidecar, signals kill only recorded PIDs. SIGINT leaves the profile directories behind (no browser process). |
| 11. Machine-readable output and exit codes | **Covered.** `--json`, 0/2/1. |
| 12. Warnings channel | **Covered** for software rasterisation, DPR mismatch, canvas upscale, clipped/overflowing content, readiness and settle failures, touch not applied, keys landing on `BODY`, uncaught exceptions, and a measured hold that missed its request. |
| 13. Input | **Partial.** Locators first, coordinates second, interpolated moves, keyboard by `code` with down/up and persistent modifiers, frame-relative holds. **Touch dispatch, right-click and drag are not implemented.** |

Acceptance items 1–11 from `docs/browser-automation.md` were exercised on this machine: the
two-peer DataChannel exchange with and without injected loss; two peers at 59.3 Hz; a 390x844
DPR 3 canvas producing 1170x2532 with no DPR warning; an overflowing page failing at 1440x900 DPR
2; a throwing page exiting 2 and naming `uncaught: Error: kaboom`; SIGINT with two peers leaving
no browser process; a sidecar stating the network emulation does not reach WebRTC; a `KeyW` hold
of exactly 12 frames; an unfocused canvas reporting `BODY`; an arrow key scrolling the document;
and a coordinate click landing at 200,400.

## Known limits

- **Every CDP call costs about 100 ms on this transport.** `headless_chrome` 1.0.22 holds its
  websocket lock for up to its 100 ms read timeout, so a call can wait a full timeout before it is
  sent. Frame holds are scheduled from a counter read taken *before* the keydown, which cancels two
  of those round trips and makes a 12-frame hold exact. Holds shorter than roughly two round trips
  (~12 frames at 60 Hz) land on the next command boundary and produce a warning with the measured
  value.
- **Network emulation does not touch WebRTC.** `Network.emulateNetworkConditions` governs HTTP and
  WebSocket; SCTP/DTLS is not covered. Use the loss hook for a data channel.
- **The loss hook drops calls to `send()`,** not packets. It measures the receiver's resilience to
  a missing message, not the network's behaviour.
- **Frame stability is byte equality of PNGs.** A looping animation never settles. Use
  `--no-settle`, or give the app a `window.__ready`/frame counter.
- **A settled blank page still settles.** Nothing here decodes the image; looking at it is still
  the review.
- **`--select` is a CSS selector**, not a frame-aware locator. Cross-origin iframes are not
  traversed.
- **Touch emulation is not touch input.** The viewport reports touch and `maxTouchPoints`, but
  pointer steps still dispatch mouse events.
- **The browser is not pinned by this crate.** It launches Brave on macOS by default and takes
  `--browser` otherwise; pinning belongs to the flake/packaging decision.
- **A hard kill bypasses `Drop`.** SIGINT and SIGTERM kill the browser processes through a signal
  handler but skip profile removal; SIGKILL or a crash can leave both. A normal exit or an error
  removes the profiles and the run directory.
- **The generated bindings have a bug:** `auto_generate_cdp` 0.4.6 swaps the parameters of
  `Emulation.setTouchEmulationEnabled` and the deprecated `Page.setTouchEmulationEnabled`. This
  crate sends the correct shape with a small custom `Method`; see the cheat sheet.

## Build and check

```sh
direnv exec . cargo build -p browser-shot
direnv exec . cargo test -p browser-shot
direnv exec . cargo clippy -p browser-shot --all-targets -- -D warnings
```

Manual checks, all against pages under `.tmp/`:

```sh
# Acceptance 3: element capture at DPR 3
./target/debug/browser-shot "file://$PWD/.tmp/browser-shot-canvas.html" \
  390 844 .tmp/canvas.png --select canvas --dpr 3 --ready "window.__ready === true"

# Acceptance 4: an overflowing page fails loudly at 1440x900 DPR 2
./target/debug/browser-shot "file://$PWD/.tmp/browser-shot-overflow.html" \
  1440 900 .tmp/overflow.png --dpr 2 --no-settle \
  --expect "document.documentElement.scrollWidth <= document.documentElement.clientWidth"

# Acceptance 5: an exception fails the run and is named in the sidecar
./target/debug/browser-shot "file://$PWD/.tmp/browser-shot-errors.html" \
  390 844 .tmp/errors.png --no-settle

# Acceptance 11: a coordinate click lands where asked
./target/debug/browser-shot "file://$PWD/.tmp/browser-shot-canvas.html" \
  390 844 .tmp/click.png --select canvas --dpr 3 --ready "window.__ready === true" \
  --click 200,400 --steps 8
```
