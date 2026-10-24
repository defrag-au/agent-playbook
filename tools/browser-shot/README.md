# browser-shot

The single-peer half of `docs/browser-automation.md`, implemented only where a process needs it
today: the widget storybook at compact width, a deployed-page check, and a web game capture.

It launches its own browser, drives one page, and writes `<name>.png` + `<name>.txt` (and the
same data as typed JSON with `--json`). It does **not** run multiple peers, emulate network
conditions, or inject WebRTC loss; those belong to the process that will run the two-peer spike.

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

## What the sidecar looks like

```
url          file:///…/browser-shot-canvas.html
captured     2026-10-24T05:05:35Z
peer         0 (of 1 process)
browser      Chrome/155.0.8059.85
mode         headless (new)
renderer     WebGL 2.0 (… SwiftShader …) — SOFTWARE
viewport     390x844 dpr 3 touch
image        1170x2532 (element canvas)
css box      390x844 at 0,0
ready        fonts.ready = loaded; window.__ready === true = true; window.__frames = 71 (waited 10); frame.stable = 3 samples, 2 identical after 2270 ms
waited       4 predicates, 2904 ms
console      0 errors, 0 warnings, 0 uncaught
requests     0 failed (1 total)
network      not emulated
overlay      none
input        click (coordinates) at 200,400 · 8 steps
focus        canvas#game
scroll       none
locator      coordinates
expect       navigator.maxTouchPoints > 0 = true (pass)
warnings     renderer is software: …
profile      .tmp/browser-shot-94930 (removed on exit)
screenshot   .tmp/canvas-390.png
result       pass
```

## What is covered, and what is not

| Requirement | State |
| --- | --- |
| 2. Deterministic readiness (`wait_until`, network idle, frames, `window.__ready`) | **Covered.** Timeouts name the predicate and its last value. |
| 3. Element capture at real pixel dimensions | **Covered.** Reports the CSS box and the PNG size; a DPR mismatch fails. |
| 4. True device emulation (viewport, DPR, touch, UA) | **Covered.** Touch is verified via `navigator.maxTouchPoints`; a mobile UA is the caller's `--user-agent`. |
| 5. Evaluate before the document's scripts | **Covered** with `--preload`. |
| 6. Console, exceptions and failed requests | **Covered**, timestamped relative to the run. |
| 9. Image + text sidecar | **Covered**, with typed JSON alongside it. |
| 10. Unique handle and cleanup | **Covered** by the profile path and `Drop`; SIGINT/SIGTERM kill the browser through a handler. The PID is not exposed in the sidecar. |
| 11. Machine-readable output and exit codes | **Covered** with `--json` and the 0/2/1 split. |
| 12. Warnings channel | **Covered** for software rasterisation, DPR mismatch, canvas upscale, clipped/overflowing content, readiness and settle failures, touch not applied, uncaught exceptions. |
| 13. Input | **Partial.** Locators, coordinate fallback, interpolated moves, explicit buttons; keyboard down/up, touch dispatch and frame-relative timing are **deferred**. |
| 1. N isolated browsers | **Deferred** to the two-peer process. |
| 7. Network emulation | **Deferred**; the sidecar says `not emulated` rather than implying it. |
| 8. In-page loss hook | **Deferred** to the two-peer process. |
| Pinned browser | **Deferred** to the flake/packaging decision; `--browser` takes a path today. |

Acceptance items 3, 4, 5, 6 (SIGINT/SIGTERM) and 11 have been exercised locally; 1, 2, 7, 8,
9 and 10 need the multi-peer/keyboard work.

## Known limits

- **Frame stability is byte equality of PNGs.** A static page settles; a looping animation never
  does. Use `--no-settle`, or give the app a `window.__ready`/frame counter.
- **A settled blank page still settles.** Nothing here decodes the image; looking at it is still
  the review.
- **`--select` is a CSS selector**, not a frame-aware locator. Cross-origin iframes are not
  traversed.
- **`--no-settle` captures immediately.** It does not wait for `--settle-ms`; that value is only
  the minimum observation window for the stability loop.
- **A hard kill bypasses `Drop`.** SIGINT and SIGTERM kill the browser through a signal
  handler but skip the profile removal; SIGKILL or a crash can leave both. A normal exit or an
  error removes both.
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
./target/debug/browser-shot "file://$PWD/.tmp/browser-shot-canvas.html" 390 844 .tmp/canvas.png \
  --select canvas --dpr 3 --ready "window.__ready === true"

# Acceptance 4: an overflowing page fails loudly at 1440x900 DPR 2
./target/debug/browser-shot "file://$PWD/.tmp/browser-shot-overflow.html" 1440 900 .tmp/overflow.png \
  --dpr 2 --no-settle \
  --expect "document.documentElement.scrollWidth <= document.documentElement.clientWidth"

# Acceptance 5: an exception fails the run and is named in the sidecar
./target/debug/browser-shot "file://$PWD/.tmp/browser-shot-errors.html" 390 844 .tmp/errors.png --no-settle

# Acceptance 11: a coordinate click lands where asked
./target/debug/browser-shot "file://$PWD/.tmp/browser-shot-canvas.html" 390 844 .tmp/click.png \
  --select canvas --dpr 3 --ready "window.__ready === true" --click 200,400 --steps 8
```
