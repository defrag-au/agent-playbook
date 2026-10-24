# browser-shot

A spike, not the final tool. One command that launches a browser, waits for a frame that has
actually stopped changing, and writes a screenshot plus a report beside it.

The point is to test whether `headless_chrome` is a good enough base for the browser loop the
`widget-screenshot` skill currently describes in Node prose. It is deliberately narrow: one
image, one page, no scenario file yet.

## Usage

```sh
direnv exec . cargo run -p browser-shot -- \
  "http://127.0.0.1:8095/?nav=0#/wallet-button" 390 844 .tmp/wallet-390.png
```

```
browser-shot <url> <width> <height> <out.png> [options]

  --click x,y         move the pointer there and click (repeatable)
  --hover x,y         move the pointer there without clicking (repeatable)
  --settle-ms N       minimum time before a frame may be accepted (default 1200)
  --timeout-ms N      give up waiting for a stable frame after N ms (default 30000)
  --no-settle         wait --settle-ms, then capture once
  --report PATH       report path (default: the image with a .txt extension)
  --brave PATH        browser executable (default: Brave on macOS)
  --keep-profile      leave .tmp/browser-shot-<pid> behind for inspection
```

Every run writes `out.png` and `out.txt`. The report is the second thing to read, after the
image: title, final URL, viewport, how long the frame took to settle, and console errors and
uncaught exceptions.

## What the spike proves

- **The tool owns the browser.** `headless_chrome`'s `TemporaryProcess` kills the child on drop;
  a unique profile under `.tmp/browser-shot-<pid>` is removed afterwards. There is no `pkill`
  step, and a failed run leaves nothing listening on a port.
- **The viewport is real.** `Emulation.setDeviceMetricsOverride` at 390×844 produces a layout
  at 390, not a 620px layout cropped to 390. The test page wraps at the width it is given.
- **Readiness is a condition.** It captures every 250 ms and accepts after two consecutive
  byte-identical PNGs, never before `--settle-ms`. A page that never stops changing exits
  non-zero with `settled: NO` and still writes the image.
- **Input works on a canvas-like page.** `--hover` and `--click` go through
  `Input.dispatchMouseEvent`, moving the pointer before pressing — the same trick the Node
  helper needed for egui.
- **Console failures are visible.** `Runtime.consoleAPICalled`, `Log.entryAdded` and
  `Runtime.exceptionThrown` are collected; a blank screenshot with a `Uncaught` line is not a
  pass.

## Known limits

- **Settling is byte equality of PNGs.** A static page settles; a looping animation never does.
  `--no-settle` is the escape hatch. A future version could compare decoded pixels with a
  tolerance, or wait for an app-supplied ready flag.
- **A settled blank page still settles.** Nothing here decodes the image. The review is the
  image, which is why the report does not pretend to replace it.
- **Clicks are coordinates.** Selector-based clicks and typing are the next slice; they are
  what `--click` cannot express.
- **Launch discovery still goes through the crate.** `headless_chrome` reads the WebSocket URL
  from the browser's stderr. If that ever proves fragile, launch with
  `--remote-debugging-port=0` ourselves, poll `DevToolsActivePort`, and use `Browser::connect`.
- **No sessions yet.** One process, one browser, one page. A scenario runner can keep one
  browser for many shots, which is where the cold-start cost stops mattering.
- **A hard kill bypasses `Drop`.** SIGKILL, or a crash of `browser-shot` itself, can leave the
  child and its profile behind; a normal exit, an error, or a settled timeout removes both.

## Build and check

```sh
direnv exec . cargo build -p browser-shot
direnv exec . cargo test -p browser-shot
direnv exec . cargo clippy -p browser-shot --all-targets -- -D warnings
```

The first build compiles the generated CDP protocol and takes noticeably longer than the rest
of the workspace; it is cached afterwards.
