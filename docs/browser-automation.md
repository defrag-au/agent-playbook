# Browser automation for agents

**Status:** requirements, 2026-10-24. **Built:** [`tools/browser-shot`](../tools/browser-shot/README.md)
implements these, and its README carries a requirement-by-requirement coverage table. Three
deviations are recorded in *Where the implementation differs* below. The follow-up gaps in
[`browser-automation-followup.md`](browser-automation-followup.md) — persisting an `eval` result,
millisecond event timestamps, and reporting a frame rate on a page that never settles — are
implemented in the same tool as of 2026-10-24. What still
blocks the two-peer spike is client-side: `island-funk` exposes no frame counter, readiness flag or
channel handles for a scenario to drive.

## The problem

An agent's visual claims currently rest on artefacts it cannot produce. `rules/org/defrag/look-at-what-you-built`
says "render it and look at it", and `look-at-the-game` says a capture writes an image **and a
report**, because the report catches what a glance misses. The native macroquad client has
`--shot` and `--sheet` for exactly that. The web build has nothing.

Two jobs are blocked on this today:

- `skills/widget-screenshot` can serve the storybook, but the compact-width shot it exists for is
  the one a plain one-shot screenshot tool cannot take.
- The P2P overlay's go/no-go is a two-peer measurement under induced loss. There is no way to run
  it.

And one is blocked less obviously: every claim of the form "the deployed page renders" currently
costs a human. That is the same failure `inspection-tools.md` names — a capability exists, but
reaching it costs an approval whose intent has to be reconstructed from syntax.

## The jobs

| Job | Where it comes from | What it needs |
| --- | --- | --- |
| Two-peer overlay spike | `P2P_OVERLAY_DESIGN.md` §12 | two isolated peers, induced loss, per-peer JS, simultaneous capture |
| Widget storybook at compact width | `widget-screenshot` skill | real device emulation, element capture, deterministic readiness |
| Web game capture | `look-at-the-game`, browser half | canvas capture plus the text report |
| Deployed page check | `look-at-what-you-built` | a real URL, console and failed-request capture |

## Must

1. **N isolated browsers, driven concurrently.** Separate storage (cookies, `localStorage`,
   IndexedDB, cache) and separate WebRTC stacks per peer. **Decided: separate browser processes,
   not contexts inside one browser.** Two tabs share storage, so two players collide on identity;
   and contexts in one process inherit the renderer's backgrounding rules, so a peer that is not
   focused drops to roughly 1 Hz for no reason the scenario asked for. Separate processes also make
   per-process WebRTC flags available — `--use-fake-device-for-media-stream`,
   `--use-fake-ui-for-media-stream` — which is the route to synthetic media streams if that is ever
   wanted. The cost is startup time and per-process pipe management, and that is accepted. The
   layer reports how many processes it started.
   Peers must be addressed independently and evaluated against **simultaneously**, not in sequence —
   a two-peer measurement is meaningless if one is frozen while the other runs.
2. **Deterministic readiness, and no sleeps.** The layer provides `wait_until(predicate)`,
   `wait_for_network_idle`, and `wait_for_frames(n)`; it never resolves a "ready" by waiting a
   fixed time. On timeout it must report **which predicate was still false and what its value was**,
   not that the wait elapsed. A wasm client has no DOM signal for "instantiated and drew", so the
   layer must support a page-provided hook (`window.__ready`) that the app sets.
3. **Element-targeted capture, at real pixel dimensions.** Capture a selector, not the viewport,
   and report both the CSS box and the device pixels produced. A capture whose DPR differs from
   what was asked is a warning, not a silent success — a soft, upscaled shot is indistinguishable
   from a correct one until someone measures it.
4. **True device emulation.** Viewport, device scale factor, touch and mobile UA through
   `Emulation.setDeviceMetricsOverride`. **`--window-size` is not emulation** and must not be
   offered as the way to get a narrow shot; that is the specific trap `widget-screenshot` records.
5. **Evaluate in the page, including before the document's own scripts run.** Pre-document
   scripting is not a convenience: instrumenting `RTCPeerConnection` after the app has constructed
   one is too late, and the loss hook in (8) only works if it is installed first.
6. **Console, uncaught exceptions and failed requests, per peer, timestamped, into the sidecar.**
   A capture that quietly swallowed a 404 is worse than no capture, because it looks like evidence.
7. **Network emulation, and an honest statement of its scope.** Latency, jitter, bandwidth and
   connection loss per peer. The layer must say in its output that this **does not apply to
   WebRTC** — the knob is `Network.emulateNetworkConditions`, and it governs HTTP and WebSocket,
   not SCTP/DTLS. Do not let an agent read "2% loss" and believe it applied to a data channel.
8. **An in-page loss hook for the WebRTC case.** Because (7) cannot reach a data channel, the layer
   must provide a first-class way to install a drop rate into `RTCDataChannel.prototype.send` (or
   hand the app a hook it calls). This measures the **receiver's resilience**, which is what the
   spike actually asks — a thin buffer must not stall when a packet is dropped. The artefact must
   label it as injected, not as a network condition.
9. **Every capture writes an image and a text sidecar.** Same contract as native `--shot`: the
   image is the review, the sidecar is the second check. Fixed fields, so a later reader can diff
   two runs.
10. **A unique handle per run, and cleanup by handle only.** `--user-data-dir` under the repo's
    `.tmp/<run-id>`, the PID exposed, and no pattern matching on process names. The user's own
    browser is running; `pkill -f Chrome` is the failure mode this exists to prevent
    (`rules/core/kill-only-what-you-started`).
11. **Machine-readable output and meaningful exit codes.** `--json` on every verb. Non-zero when a
    capture succeeded but its assertions did not — a shot that was taken is not a shot that passed.
12. **A warnings channel the agent reads before it reports.** "WebGL context is software",
    "capture is a viewport, not the element", "the page never signalled ready, this was taken
    anyway". Each of these changes what the agent may claim, so each has to be in the output rather
    than in the agent's optimism.
13. **Input that reaches a game, not just a form.** Locators for DOM elements, `code`-based keyboard
    with separate down/up and held modifiers, pointer movement as interpolated steps, touch through
    `Input.dispatchTouchEvent`, and every input step timestamped against the page's own frame
    counter rather than a wall clock. See [*Input*](#input).
14. **Start the origin the scenario tests, and stop it again.** A scenario that needs a server is two
    commands and a manual teardown otherwise, and one logical server is usually several processes
    with only the last holding the port. The layer owns the process **group** it started and
    terminates it on every exit path — success, assertion failure, error, signal — and refuses to
    start on a port somebody else holds rather than testing the wrong build. Readiness is its own
    step (`wait_for port`, `http`, `log`, `exit`), with its own timeout and the process's log tail
    on failure, because "the build failed" and "the build is slow" are different answers.

## Must not

- **Claim WebRTC loss emulation through the network knob.** See (7) and (8).
- **Claim a canvas capture is what the GPU drew.** Headless WebGL may be SwiftShader; the sidecar
  names the renderer and the layer flags software rasterisation.
- **Sleep to synchronise.** A sleep is a bound nobody read.
- **Treat a produced screenshot as evidence of correctness.** The layer produces artefacts; judging
  them is the agent's job, and `verify-before-claiming` says so.
- **Kill by process name.** See (10).

## Traps that will otherwise be rediscovered

Each of these fails quietly — a plausible artefact, a wrong claim.

- **A background context is throttled to about 1 Hz.** This is why Must 1 decides on separate
  browser processes rather than contexts in one: two peers in one browser, where one is not the
  focused tab, will not tick at 60 Hz — which invalidates exactly the measurement the spike exists
  to make, while looking like a latency result. Keep this note if anyone later decides one process
  is faster: that route has to pass the backgrounding flags, and the sidecar has to say so.
- **`canvas.width` is not the CSS size.** At DPR 3 these differ by a factor of three, and a capture
  that reports CSS pixels will make a crisp shot look blurry or a blurry one pass.
- **Text renders in a fallback face, then swaps.** A capture taken between the two is a wrong
  picture with no error. Wait for `document.fonts.ready` *and* a frame after it.
- **wasm instantiation is asynchronous with no DOM event.** Without an app hook there is nothing to
  wait for, so a capture races it. This is why (2) requires a page-provided signal.
- **A click before first paint lands on nothing.** Input must be preceded by a readiness predicate,
  not a delay.
- **Touch input needs `Emulation.setTouchEmulationEnabled` as well as metrics.** Metrics alone give
  a narrow desktop window, which is not the case being tested.
- **Element capture can be clipped by an ancestor with `overflow: hidden`.** Capture the element's
  own backing store where the renderer allows it, and warn when the box was clipped.

## Input

A game is not a form. Input has to reach a canvas that may hold no focus, at a frame the scenario
chose, with key identity that matches what the app bound to.

### Locators first, coordinates second

Every input step names either a locator or an explicit viewport coordinate, and the sidecar records
which. A locator resolves to a box; a click without coordinates lands on its centre. Coordinates are
for the canvas, where the game reads its own position out of the event and the DOM box says nothing
about where the player is standing.

A locator must **not** wait for "stable" before acting. A canvas is never stable — it is redrawn
every frame — so a stability wait is a hang dressed as a timeout.

### Keyboard

- **Send by `code`, not `key`.** `KeyW` is the physical position; `w` is what a US layout produces.
  A game binds to the former, and a scenario written against the latter breaks on an AZERTY machine.
  `key` stays available for the case that genuinely wants a character.
- **Down and up are separate steps**, with no auto-repeat between them. Holding a key is `down`,
  wait *n* frames, `up`. A layer that only offers `press()` cannot express a held control, and
  movement in a game is exactly that.
- **Modifiers persist across steps** until released, so shift-to-run or a ctrl modifier is two extra
  steps rather than a special case.
- **Events must be trusted.** Dispatch through the debugger protocol (`Input.dispatchKeyEvent`),
  never `dispatchEvent(new KeyboardEvent(…))`. `isTrusted` is `false` on the latter and an app is
  entitled to ignore it — which looks exactly like a broken binding.
- **Report the focused element at the time of the event**, and let a step focus a locator first. Key
  events go to the focused element; a canvas that was never focused receives nothing and raises no
  error.
- **Say whether the page scrolled.** Arrow keys and space scroll a document that did not call
  `preventDefault`. A game that forgot is broken in a way only this check finds.

### Pointer and touch

- **Moves are steps, not teleports.** A game that reads `mousemove` for aim or hover needs the
  intermediate positions; `click()` alone dispatches one move to the target and nothing between. The
  layer takes a step count and interpolates.
- **Buttons and modifiers are explicit** — left, right, middle, and held modifiers — because the
  native capture tool already distinguishes them (`--hover`, `--drag`, `--rclick`) and the web build
  should reproduce each.
- **Touch is `Input.dispatchTouchEvent`**, enabled together with
  `Emulation.setTouchEmulationEnabled`. Metrics alone give a narrow desktop window, not a phone.
- **Drag is down, move, move, up** — not a `dragstart`/`drop` pair. A canvas has no drag-and-drop
  semantics; it has pointer events.

### Frame-relative timing

Input expressed in milliseconds is not reproducible: a 200 ms hold at 57 fps is 11 frames on one run
and 12 on the next, and the capture lands between them. Every input step is therefore **timestamped
against the page's own frame counter**, and a scenario says "hold for 12 frames" rather than "hold
for 200 ms". The layer reads the counter the app exposes, and the sidecar records it beside every
event.

That is also what makes a two-peer scenario meaningful: both peers advance on their own frames, so a
step that must be simultaneous is dispatched against both counters rather than against a wall clock
one of them is not keeping.

## The sidecar

Every `<name>.png` gets `<name>.txt`, with a stable field order:

```text
url        https://…                      captured   2026-10-24T14:12:03Z
peer       0 (of 2 processes)             browser    141.0.7390.54
mode       headless (new)                 renderer   WebGL 2.0 (Mesa/llvmpipe) — SOFTWARE
viewport   390x844 dpr 3 touch            image      1170x2532 (element canvas)
ready      window.__game.frames = 42      waited     3 predicates, 1.8 s
console    0 errors, 2 warnings           requests   0 failed
network    loss 2% / 80 ms — HTTP ONLY, not WebRTC
overlay    drop 2% injected in send()     — injected, not a network condition
input      KeyW down @ frame 42, up @ 54  focus      canvas#game (focused at step 3)
scroll     none                           locator    canvas, coordinates 320,240
warnings   canvas captured through a software rasteriser
```

The last four lines are the ones that change what an agent may say. They are not diagnostics; they
are the difference between "it renders" and "it rendered, in software, with the network emulation
that did not apply to the thing under test".

## Interface, provisional

Verbs rather than flags with modes, for the same reason `at-` has verbs: one command, one question.

```sh
bt shot  --url <url> --select canvas --viewport 390x844 --dpr 3 --touch --out .tmp/x.png
bt run   --scenario .tmp/two-peer.toml --artifacts .tmp/run-<id> --json
bt check --url <url> --expect 'window.__game.frames > 1' --json

bt input --peer 0 --key KeyW --down --frames 12
bt input --peer 0 --key KeyW --up
bt input --peer 1 --locator canvas --click --at 320,240 --steps 8
```

A scenario file, because the spike is a script and not a single invocation: peers declared up front,
then ordered steps (`wait`, `eval`, `key`, `pointer`, `capture`, `assert`), each naming the peer it
applies to. Concurrency belongs in the file, not in shell job control.

## Environment

- Sandboxed shells here have no nix daemon and per-host network grants; the layer must work against
  loopback for a `trunk serve` on `127.0.0.1:8095` without a network grant.
- It should carry its own pinned browser rather than depending on whatever is installed, so a
  rebuild does not change the pixels.
- macOS, Apple silicon. Everything above has to hold on both headless and headed; the headed mode
  is the one that shows what the user will actually see, and `look-at-the-game` already treats
  "a capture is not the window" as a rule.

## Acceptance

The layer is done when these pass, and not before. Items 3, 8, 9 and 12 were re-run independently
against a purpose-built probe page; the rest are `browser-shot`'s own claim and have not been
re-verified here.

1. Two peers, each loading a page, open a DataChannel to the other, exchange 100 messages; all
   arrive; then with 2% injected drop the receiver never blocks and reports the gap count.
2. Both peers are ticking at 60 Hz simultaneously — measured, not assumed — and the sidecar says
   how many browser processes were started.
3. `--select canvas --viewport 390x844 --dpr 3` produces an image whose sidecar says `1170x2532`,
   with no warning.
4. The same page at `1440x900 dpr 2` reports `scrollWidth <= clientWidth`; a page that overflows
   fails loudly.
5. A page that throws on load produces a non-zero exit and a sidecar naming the exception.
6. Killing the run mid-capture leaves no browser process behind, and kills nothing it did not start.
7. A WebRTC run states, in the sidecar that the network emulation did not apply to it.
8. `KeyW` down, twelve frames, up: the app's own counter says twelve, and the player moved for
   exactly that long.
9. A key sent to a canvas that was never focused is reported as having reached the focused element
   instead, by name — not silently dropped.
10. Arrow keys on a page without `preventDefault` are reported as having scrolled the document.
11. A click at explicit coordinates on a canvas lands at those coordinates, not at the box centre,
    and the sidecar records which form was used.

## Where the implementation differs

- **Jitter is not emulated.** CDP has no jitter knob; `browser-shot` rejects it with an error rather
  than silently treating it as zero. Latency, bandwidth and loss are available; jitter is not, and
  the sidecar is the place to read which was actually applied.
- **The browser is not pinned.** It defaults to the browser installed on the machine (Brave on
  macOS), with `--browser PATH` and a peer's `browser=` to override. Two machines can therefore
  produce different pixels from the same scenario. Acceptable for review; not acceptable for a
  golden-image test, and the sidecar names the browser version so a difference is attributable.
- **No iframe traversal, and a synchronous `eval` cannot be timed out.** A `timeout` is a page-side
  `Promise.race`, so an un-awaited infinite loop blocks the renderer. Neither limit is on the path
  of the four jobs above.

## What this replaces, and what it does not

It replaces a one-shot screenshot invocation whose intent had to be reconstructed, and it makes the
compact-width and two-peer cases expressible at all. It does not replace looking at the image: the
whole point of the sidecar is that the agent still has to read both.
