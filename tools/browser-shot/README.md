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
  --dpr 3 --touch --settle-ms 6000
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
  --rclick x,y            move there and right-click (repeatable)
  --drag x1,y1,x2,y2     press, move to x2,y2, release (repeatable)
  --hover x,y             move there without clicking (repeatable)
  --click-selector CSS    click the centre of this element (repeatable)
  --hover-selector CSS    hover the centre of this element (repeatable)
  --steps N               interpolate each pointer move over N steps (default 1)
  --settle-ms N           minimum observation window for frame stability (default 1200)
  --timeout-ms N          timeout for every wait, navigation included (default 30000)
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
`--expect`, an uncaught exception, touch emulation that did not take, or a navigation that did not
complete. Console errors and failed requests are reported but do not fail the run on their own.

### When navigation does not complete

Navigation is complete at whichever comes first, bounded by `--timeout-ms`:

- the main frame's `networkAlmostIdle` lifecycle event (two or fewer connections for 500 ms), or
- the main frame's `load` event, followed by 500 ms with no request in flight and none starting or
  ending.

The second exists because Chrome can withhold `networkAlmostIdle` from a page that has loaded and
has nothing in flight: a WebGL canvas rendering every frame on a software renderer at `--dpr 3`
timed out with `load` fired and an empty network. The `ready` line of a passing sidecar names the
one that applied: `navigation = complete (networkAlmostIdle)` or
`navigation = complete (load + 500 ms network quiet)`. Navigation completing is not the page being
ready; `--ready` and the other readiness forms below are still the gate for that.

"In flight" is what `browser-shot` itself tracks: a request counts from `Network.requestWillBeSent`
until `loadingFinished` or `loadingFailed`. An open `EventSource` or a long poll is therefore always
in flight and holds the quiet window open; such a page completes through `networkAlmostIdle`, which
tolerates two connections, or times out. A WebSocket never counts: Chrome reports it through
`webSocketCreated`, not as a request.

When navigation does not complete, the sidecar is still written, with what was collected up to that
point, and the run exits 2:

- `result` reads `FAIL — navigation crashed: …`, `FAIL — navigation timed out: …` or
  `FAIL — navigation failed: …` (`Page.navigate` refused, e.g. `net::ERR_CONNECTION_REFUSED`).
- **A crash** is `Inspector.targetCrashed` (or `Inspector.detached`), reported as soon as it
  arrives rather than after the timeout.
- **A timeout** says whether the main thread is hung — `Runtime.evaluate("1")` gets two seconds to
  answer — lists the main frame's lifecycle events with their times, and names up to three
  requests still in flight. A page that busy-loops reads `main thread hung`; a page waiting on a
  slow request reads `main thread responsive` and names the request.
- `ready` lists `navigation` first, then each configured predicate as `not reached`.
- Console messages, uncaught exceptions and failed requests are the same lines a passing run
  records.
- **A best-effort screenshot** is taken when the page reported a first paint and the renderer is
  still there: the `--select` element if it can be found, else the viewport, within five seconds.
  A hung main thread usually cannot produce one; the `screenshot` line says why not.

`headless_chrome`'s own `wait_until_navigated` uses the tab's default timeout, which is 20 seconds
unless set. That is why a navigation used to fail after about 21 seconds whatever `--timeout-ms`
said. `browser-shot` now does its own wait, for either completion above, against `--timeout-ms`.
A scenario's initial navigation and `goto` use the same wait against the run timeout, and fail the
run with the same reason; they do not yet write a per-peer sidecar.

## Scenarios: more than one peer

```sh
direnv exec . cargo run -p browser-shot -- \
  run --scenario .tmp/two-peer.dsl --artifacts .tmp/two-peer-run --json
```

Each peer is a separate browser process with its own profile, storage and WebRTC stack. Peers are
launched in parallel; a `wait` that names several peers dispatches to all of them concurrently.
The run writes `run.json` / `run.txt`, plus one image and sidecar per peer.

A scenario may declare up to **16 peers**. The cap exists because the signal handler keeps one slot
per process; it was eight, which capped a WebRTC mesh at seven links per peer — less than the twelve
a game's interest set may name. Sixteen leaves room for a thirteen-peer mesh, the first world where
every page holds twelve links.

**`run` prints one line per step on stderr as it starts it**, `[12/44] wait p01,p02 frames=200`.
A mesh scenario spends minutes between its first and last step and has no other output until the
run finishes; `--json` still writes only the report to stdout.

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
| `wait` | `peer` or `peers`, and one or more of `predicate`, `frames`, `rate`, `network_idle`, `selector` (+ optional `state`), `text`, `url` / `url_contains`, `stable_ms`; `timeout_ms` |
| `sleep` | `peer`, `duration_ms` — explicit pause; prefer a condition wait |
| `eval` | `peer`, `expression`, `await_promise`, `capture` (stores the result for `${name}` substitution), optional `out` (writes the value as JSON), optional `timeout_ms` on an awaited expression |
| `key` | `peer`, `code`, `state` (`down`/`up`), optional `hold_frames` on a down step |
| `pointer` | `peer`, `kind` (`click`/`hover`/`drag`/`tap`/`swipe`), `x`+`y` or `selector`, `steps`, optional `button`, `from_x`+`from_y` |
| `capture` | `peer`, `out`, optional `select` |
| `burst` | `peer`, `out`, `frames`, optional `interval_ms`, `mode` (`capture`/`screencast`), `select` |
| `assert` | `peer`, `expression`, optional `message` |
| `network` | `peer`, and at least one of `latency_ms`, `download_kbps`, `upload_kbps`, `loss_percent` |

A key step with `hold_frames` sends down, waits on the page's counter, then sends up, all in one
step. Modifiers persist across steps until released. A failed `assert` sets exit 2 and names the
expression and its value. An awaited `eval` is bounded by its step timeout (or the run timeout) and
fails on a rejected promise or a thrown exception, instead of capturing the error as a value.

A peer may carry extra browser flags with `args` (`args="--use-fake-ui-for-media-stream"` in the
DSL, `args = ["--use-fake-ui-for-media-stream"]` in TOML). The run sets `--user-data-dir`,
`--remote-debugging-port` and the headless mode itself; an override is a validation error. The
sidecar repeats the extra flags, so a capture can be diffed against the process it came from.

## Waiting, not sleeping

Wait on what the page shows, not on a fixed delay:

```text
wait page selector="#wallet-button" state=enabled
pointer page click selector="#wallet-button"
wait page selector="#wallet-modal" state=visible
wait page text="Connect" selector="#wallet-modal"
wait page url~="#/wallet"
wait page stable=500ms selector="#wallet-modal"
```

- Prefer `wait … selector=` / `text=` / `url~=` / `stable=` over `eval "new Promise(r => setTimeout(r, N))" await`.
- Prefer `pointer … selector=` over click coordinates. A `click`/`tap` on a selector waits until the
  element is enabled before clicking, bounded by the step timeout.
- `sleep page 1s` exists for the rare case nothing else applies; it is named in the report so a
  fallback pause is visible. Do not use it as the default.

A wallet modal flow that an agent once wrote as coordinate clicks and `setTimeout` sleeps rewrites as:

```text
peer page url="http://127.0.0.1:8095/?nav=0#/wallet-button" viewport=1280x800

wait page selector="[data-testid=wallet-button]" state=enabled
pointer page click selector="[data-testid=wallet-button]"
wait page selector="[data-testid=wallet-modal]" state=visible
wait page text="Choose a wallet" selector="[data-testid=wallet-modal]"
pointer page click selector="[data-testid=wallet-option-injected]"
wait page url~="#/wallet/connected" timeout=10s
capture page out=wallet-connected.png
```

(Selectors are placeholders for the real page's hooks.)

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

## Starting what the scenario tests

A scenario can run the server it is testing. `spawn` owns a process group and `wait_for` decides
when it is up; they are separate steps because starting and being ready are separate questions.

```text
run timeout=90s

peer page url=about:blank viewport=390x844

spawn site command="python3 -m http.server 8791 --bind 127.0.0.1" dir=tools/browser-shot/scenarios port=8791
wait_for ready=site timeout=20s
goto page "http://127.0.0.1:8791/page.html"
assert page "document.title === 'serve fixture'"
```

The process is killed by its **group** when the run ends, however it ends — that is the point, since
one logical server is usually several processes and only the last of them holds the port. A process
that dies on its own stops the run at the next step, and a failed `wait_for` stops it with the last
twenty lines of that process's log.

Field tables are in [`SCENARIO.md`](SCENARIO.md). A scenario containing `spawn` executes a command,
so treat the file as you would a script.

`scenarios/` in this directory holds three acceptance runs for the feature:

```sh
./target/debug/browser-shot run  --scenario tools/browser-shot/scenarios/spawn-serve.dsl
./target/debug/browser-shot run  --scenario tools/browser-shot/scenarios/spawn-crash.dsl      # exit 2
./target/debug/browser-shot run  --scenario tools/browser-shot/scenarios/spawn-port-busy.dsl  # exit 1
lsof -ti tcp:8791 && echo "LEAK: 8791 still bound"   # prints nothing when teardown worked
```

`python3 -m http.server` there is a throwaway origin for the fixture, not part of the tool.

## Pointer, modifiers and touch

```text
peer p url="file:///tmp/game.html" viewport=390x844

wait p frames=5
pointer p drag from=60,200 at=200,320 button=left steps=4
key p ShiftLeft down
pointer p click at=120,100
key p ShiftLeft up

peer t url="file:///tmp/game.html" viewport=390x844 touch
pointer t tap at=150,150
pointer t swipe from=100,400 at=200,500 steps=5
```

Buttons are `left` (the default), `right` and `middle`. Modifiers are the ones held by `key` steps,
not a separate pointer field, so a shift-click is a `key ShiftLeft down`, the pointer step, then
`key ShiftLeft up`. `tap` and `swipe` require `touch` on the peer; without it the scenario fails
validation rather than dispatching a touch event the page never bound.

The sidecar stamps each pointer event against the page's own frame counter, for example
`drag (coordinates) 60,200 -> 200,320 · 4 steps · mousedown @ frame 150, mouseup @ frame 190`.

## Series over time: `eval out=`

A measurement is not a boolean. `eval <peer> "<expression>" out=<file>` writes the value as JSON
under the artifacts directory, beside the images:

```text
peer anim url="file:///tmp/game.html" viewport=390x844
wait anim frames=10
eval anim "window.__btSamples" out=samples.json
```

`capture=` and `out=` are separate on purpose: `capture=` binds a value for `${name}`
interpolation, `out=` writes an artefact, and a scenario can do either. The value is serialised in
the page and written exactly as produced — arrays, nested objects and fractional numbers keep their
precision, with no sample cap. A `Map`, a cyclic object, a `BigInt`, a function, `undefined` or a
non-finite number fails the step with the type and path (`cannot encode a cyclic object at $.self`),
rather than being silently stringified. The file is written when the step runs, so a later
assertion failure (exit 2) leaves it on disk; `run.json` has an `outputs` array naming every file.

Every tool-side event is stamped with fractional epoch milliseconds: `input` and pointer events,
`eval`, each `ready` predicate and `burst` carry `t=1750.123`, and `captured` is ISO with
milliseconds. A page that timestamps itself with `performance.timeOrigin + performance.now()` can
be joined to those directly, which is what a two-peer latency comparison needs.

## Animation review: bursts

One image cannot show whether an animation is smooth. A `burst` step takes a sequence of frames and
writes `<out>-000.png`, `<out>-001.png`, … plus a report of the timing. `mode=screencast` asks the
browser for every frame it paints and holds ~59 fps; `mode=capture` is a sequence of
`Page.captureScreenshot` calls and is precise but limited to ~7.5 fps by the CDP round trip.

```text
peer anim url="file:///tmp/animation.html" viewport=390x844
wait anim frames=10
burst anim out=frames/pulse frames=120 mode=screencast
```

```sh
./target/debug/browser-shot run --scenario .tmp/screencast.dsl --artifacts .tmp/screencast-run
```

`frames/pulse.txt` lists every frame with its interval, capture time, write time and size, and
summarises each as min/median/max; `run.txt` carries one `burst` line per burst and `run.json` has
the same data typed. Review the frames as a sequence — the report says what the browser did, the
images say what it looked like.

## What the sidecar looks like

```
url          file:///…/browser-shot-canvas.html
captured     2026-10-24T07:42:00.706Z
peer         0 (of 1 process)
browser      Chrome/155.0.8059.85
pid          69541
mode         headless (new)
renderer     WebGL 2.0 (… SwiftShader …) — SOFTWARE
viewport     390x844 dpr 3 touch
image        1170x2532 (element canvas)
css box      390x844 at 0,0
ready        fonts.ready = loaded · t=1792827720706.097; window.__ready === true = true · t=1792827720802.441; frame.stable = 3 samples, 2 identical after 2343 ms · t=1792827723145.002
waited       3 predicates, 2681 ms
frame rate   59.3 Hz (64 -> 124 frames in 1011 ms)
console      0 errors, 0 warnings, 0 uncaught
requests     0 failed (1 total)
network      latency 80 ms, loss 2% — HTTP/WebSocket only, NOT WebRTC
overlay      drop 2.00% injected in send() (seed 1, 5/100 dropped)
input        KeyW down @ frame 42 · t=1792827721103.221 · focus canvas#game · scroll none; KeyW up @ frame 54 (held 12 frames) · t=1792827721311.004 · focus canvas#game · scroll none
eval         window.__btSamples · t=1792827724520.550 out=samples.json
burst        screencast frames/pulse — t=1792827725001.332 — 120/120 frames, 58.8 fps …
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
| 2. Deterministic readiness, no fixed sleeps | **Covered.** `predicate`, `frames`, `rate`, `network_idle`, `selector`/`state`, `text`, `url`/`url~`, `stable`; a timeout names the condition and its last value. `sleep` exists but is for rare fallbacks. Frame rate is measured over one second and reported per peer, and a rate under 20 Hz warns that the capture is not representative. |
| 3. Element capture at real pixel dimensions | **Covered.** Reports the CSS box and PNG size; a DPR mismatch fails. |
| 4. True device emulation | **Covered.** Viewport, DPR, touch and UA through CDP; touch is verified via `navigator.maxTouchPoints`. |
| 5. Evaluate before the document's scripts | **Covered** with `--preload` and the per-peer `preload`. The loss hook installs before the document runs. |
| 6. Console, exceptions and failed requests | **Covered**, per peer, timestamped relative to the run. |
| 7. Network emulation, with honest scope | **Partial.** Latency, bandwidth and loss work for HTTP/WebSocket; the sidecar says `NOT WebRTC`. Jitter has no CDP knob and is rejected with an actionable error rather than silently ignored. |
| 8. In-page WebRTC loss hook | **Covered.** Wraps `RTCDataChannel.prototype.send` with a seeded PRNG; the rate can be changed at runtime; the sidecar labels it injected and reports drops/sends. |
| 9. Image + text sidecar | **Covered**, with typed JSON alongside it. A `burst` writes a numbered frame sequence and a per-frame timing report. |
| 10. Unique handle, PID, cleanup by handle | **Covered.** Profile at `.tmp/browser-run-<pid>/peer-N`, browser PID in the sidecar, signals kill only recorded PIDs, and SIGINT/SIGTERM still remove the profile directories. |
| 11. Machine-readable output and exit codes | **Covered.** `--json`, 0/2/1. |
| 12. Warnings channel | **Covered** for software rasterisation, DPR mismatch, canvas upscale, clipped/overflowing content, readiness and settle failures, a capture taken with no scenario wait, touch not applied, keys landing on `BODY`, uncaught exceptions, and a measured hold that missed its request. |
| 13. Input | **Covered.** Locators first, coordinates second, interpolated moves, `code` keyboard with down/up and persistent modifiers, frame-relative holds. Buttons are explicit (`left`/`right`/`middle`), drags are press-move-release, touch tap and swipe dispatch `Input.dispatchTouchEvent`, and every pointer event is stamped against the page's frame counter. |

Acceptance items 1–11 from `docs/browser-automation.md` were exercised on this machine: the
two-peer DataChannel exchange with and without injected loss; two peers at 59.3 Hz; a 390x844
DPR 3 canvas producing 1170x2532 with no DPR warning; an overflowing page failing at 1440x900 DPR
2; a throwing page exiting 2 and naming `uncaught: Error: kaboom`; SIGINT with two peers leaving
no browser process; a sidecar stating the network emulation does not reach WebRTC; a `KeyW` hold
of exactly 12 frames; an unfocused canvas reporting `BODY`; an arrow key scrolling the document;
and a coordinate click landing at 200,400. The follow-up requirements in
[`browser-automation-followup.md`](../../docs/browser-automation-followup.md) are covered:
`eval … out=` persists a series (R1), and every event line carries fractional epoch milliseconds
(R2).

## Known limits

- **A right click opens a context menu, and the page's frame counter is throttled while it is
  open.** On this machine a `wait frames=10` after a right click timed out at three seconds, while
  the same page with `preventDefault()` on `contextmenu` passed. Put a right click after any
  frame-timed input, or prevent the menu in the page.
- **A rate is a one-second sample, taken when the report is built.** A counter that a wasm app
  defines late is therefore measured after it exists, not at launch. `--no-settle` captures
  immediately, so on a page that has not defined its counter yet the sidecar says what was missing
  (`frame counter "window.__frames" is not a finite number`) rather than pretending it was zero.
- **A rate under 20 Hz is a warning, not a failure.** The sidecar names the rate and says a
  capture or interaction is not representative; animation review and a two-peer run should use
  `--headed`. The rate is also a scenario condition: `wait p rate=30` fails the peer when it is not
  met.
- **`screencast` only delivers frames the page actually paints.** A static page produces one frame
  and then waits out the timeout; an animation or a live canvas keeps the stream going. This is
  paint-driven, not clock-driven, so a screen that changes nothing is not measurable this way.
- **A burst's `screencast` mode captures the viewport, not an element.** It also forces
  `Emulation.setVisibleSize` to the device-pixel size; without that, the browser delivers frames
  at the window's backing size (`390x216` for a 390x844 viewport), not the emulated one.
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
- **Touch emulation and touch input are separate.** `tap` and `swipe` dispatch
  `Input.dispatchTouchEvent` and require `touch` on the peer; `click`, `hover` and `drag` still
  dispatch mouse events.
- **An `eval` without `await` cannot be timed out.** A `timeout` is a page-side `Promise.race` around
  an awaited expression; a synchronous infinite loop blocks the renderer and no CDP timeout can
  interrupt it. Such an eval must be awaited to be bounded.
- **The browser is not pinned by this crate.** It launches Brave on macOS by default and takes
  `--browser` otherwise; pinning belongs to the flake/packaging decision.
- **Profile removal waits for the browser to be gone.** Chrome's child processes (GPU, network,
  renderers) outlive the main process for a moment and keep writing into the profile, so removal
  first waits for every process whose command line names the profile (killing any still there
  after three seconds), then retries the delete briefly. A pass, a FAIL, a crash, a timeout, an
  error and SIGINT/SIGTERM all remove the profiles and the empty run directory; a second
  SIGINT exits at once. Only SIGKILL of `browser-shot` itself, or `--keep-profile`, leaves them.
  Waiting on the children uses `/proc`, so elsewhere it is the delete retry alone.
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
