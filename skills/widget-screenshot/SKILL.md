---
name: widget-screenshot
description: Use when adding or changing an egui or macroquad widget and you need to see it rendered, at desktop or mobile width, before reporting it done. Serves the storybook, screenshots one story headlessly, and handles the narrow-width trap.
---

# Looking at a widget

Unit tests do not catch layout. Render the widget and look at it before reporting it done —
see `rules/org/defrag/look-at-what-you-built`.

## 1. Serve the storybook

`trunk` is only inside the devshell, and the storybook has no flake of its own:

```sh
cd ui/_storybook-egui
nix develop ../.. -c trunk serve          # http://127.0.0.1:8095, per Trunk.toml
```

## 2. Screenshot one story

Stories are addressable as `#/<slug>`, where the slug is derived from the story's `label()`,
so a new story is linkable with no extra registration. Brave is Chromium, so there is nothing
to install:

```sh
BRAVE="/Applications/Brave Browser.app/Contents/MacOS/Brave Browser"
"$BRAVE" --headless=new --disable-gpu --use-gl=swiftshader --enable-unsafe-swiftshader \
  --hide-scrollbars --window-size=1400,620 --virtual-time-budget=8000 \
  --screenshot=".tmp/flow-ledger.png" "http://127.0.0.1:8095/#/flow-ledger"
```

- **Write screenshots to `.tmp/` in the repo**, not `/tmp` — they stay easy to open and to
  point someone at. `.tmp/` is gitignored.
- `--use-gl=swiftshader --enable-unsafe-swiftshader` is **required**: egui renders to a WebGL
  canvas and headless has no GPU. `GPU stall due to ReadPixels` on stderr is noise, not
  failure.
- `--virtual-time-budget=8000` lets wasm boot and the remote font fetch settle. Too low gives
  a blank canvas.
- Size the window to the content; the sidebar is about 180px.

## 3. Narrow widths — `--window-size` cannot do this

Chromium headless **floors the window near 620px**, lays out at the floor, then **crops the
capture** to the width you asked for. A "390px" shot is therefore a wide layout with its right
edge sliced off — which reads as an overflow bug that is not there, and hides the real one.

Two things are needed:

1. **`?nav=0`** drops the storybook sidebar, so the story gets the whole viewport instead of
   `viewport − 180`.
2. **CDP `Emulation.setDeviceMetricsOverride`** sets a real layout viewport. Helper:
   `ui/_storybook-egui/tools/cdp-shot.mjs`. Node has a global `WebSocket`, so there is nothing
   to install.

```sh
BRAVE="/Applications/Brave Browser.app/Contents/MacOS/Brave Browser"
"$BRAVE" --headless=new --disable-gpu --use-gl=swiftshader --enable-unsafe-swiftshader \
  --hide-scrollbars --remote-debugging-port=9222 --user-data-dir=/tmp/brave-cdp-profile \
  about:blank &
node ui/_storybook-egui/tools/cdp-shot.mjs \
  "http://127.0.0.1:8095/?nav=0#/activity-feed" 390 844 .tmp/narrow.png
```

`--user-data-dir` is required alongside `--remote-debugging-port`, or Brave refuses to expose
the port.

## 4. Check it against real data

The same helper points at a **deployed app** — pass a longer settle for socket and first
payload. This is the only way to check a widget against real data at real width, and real data
is wider than fixture data: that is how a wrapped 60-character stake address shipped.

## Native builds

No address bar. Use `STORYBOOK_STORY=flow-ledger` instead of the hash.
