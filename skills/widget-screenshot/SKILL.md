---
name: widget-screenshot
description: Use when adding or changing an egui or macroquad widget and you need to see it rendered, at desktop or mobile width, before reporting it done. Serves the storybook, screenshots one story headlessly, and handles the narrow-width trap.
---

# Looking at a widget

Unit tests do not catch layout. Render the widget and look at it before reporting it done —
see `rules/org/defrag/look-at-what-you-built`.

## 0. One shot, kill it in the same command

**Every browser you start gets `--user-data-dir=<repo>/.tmp/<task>-profile`, and is killed in
the command that started it.** The profile path is the handle you kill by
(`rules/core/kill-only-what-you-started`), and nothing survives the command.

- `--user-data-dir` is **not optional**, even for a one-line `--screenshot` run. Without it a
  headless launch falls back to `~/Library/Application Support/BraveSoftware/Brave-Browser-headless/`
  — inside the reader's own Application Support, beside the profile their everyday browser uses.
- Start, shoot, kill. On 2026-10-16 four to nine headless instances were left alive for an
  hour of iteration; the session ended with the reader unable to restart their own browser and
  asking whether the automation had clobbered their profile. The flag was there, the cleanup
  was not. Kill by the handle, in the same command:

  ```sh
  pkill -f "shot-wallet-profile"
  ```

- Never `pkill -f Brave`, `pkill -f "Brave Browser"` or anything else that matches the app by
  name: that is the reader's browser too.

## 1. Serve the storybook

Every command in this loop — the server, the browser, the helper — is launched from the repo
root through the devshell with the same prefix:

```sh
direnv exec . env NIX_HARDENING_ENABLE= <cmd>
```

- `direnv exec .` reads the already-realised devshell out of `.direnv/` and needs no nix daemon
  socket, so it works in a sandboxed agent shell where `nix develop` cannot reach the socket.
- `NIX_HARDENING_ENABLE=` (empty) is required for a wasm build: without it nix's cc-wrapper
  injects `-fzero-call-used-regs=used-gpr`, which is not a wasm32 option.
- The same prefix on every launch keeps the environment consistent — a command that quietly ran
  outside it behaves differently.

`trunk` is only inside the devshell, and the storybook has no flake of its own:

```sh
cd ui/_storybook-egui
direnv exec . env NIX_HARDENING_ENABLE= trunk serve   # http://127.0.0.1:8095, per Trunk.toml
```

The same prefix serves a frontend's own trunk (`cd <frontend>; direnv exec . env
NIX_HARDENING_ENABLE= trunk serve …`). `direnv exec .` finds the repo's `.envrc` by walking up,
so it works from a subdirectory. It rebuilds on change, and it has been seen to die mid-session
with `error writing finalized HTML output`; when that happens `rm -rf dist` and restart it, then
check the port answers with `curl` before shooting anything. Take the port from what trunk
prints, not from this file.

## 2. Screenshot one story

Stories are addressable as `#/<slug>`, where the slug is derived from the story's `label()`, so
a new story is linkable with no extra registration. Brave is Chromium, so there is nothing to
install.

The route that works everywhere — including a sandbox, where `--screenshot` mode can hang on
`CVDisplayLinkCreateWithCGDisplay failed` — is a debugging port plus `tools/cdp-shot.mjs`,
which also settles, emulates a real viewport, and can click. Run it from the repo root, so
`.tmp/` is the repo's:

```sh
nohup direnv exec . env NIX_HARDENING_ENABLE= \
  "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser" --headless=new \
  --no-sandbox --disable-gpu --use-gl=swiftshader --enable-unsafe-swiftshader \
  --hide-scrollbars --no-first-run --disable-component-update \
  --remote-debugging-port=9222 --user-data-dir=.tmp/shot-wallet-profile \
  about:blank > .tmp/shot-brave.log 2>&1 &
sleep 20
direnv exec . env NIX_HARDENING_ENABLE= node ui/_storybook-egui/tools/cdp-shot.mjs \
  "http://127.0.0.1:8095/#/wallet-button" 1200 820 .tmp/wallet-button.png 7000
pkill -f "shot-wallet-profile"
```

- **The port is `9222`, and it is hardcoded in `cdp-shot.mjs`.** Starting Brave on any other port
  makes the helper fail with `connect ECONNREFUSED 127.0.0.1:9222`, which reads like Brave is
  not up when it is.
- **A cold profile takes 15–20 seconds** to create and answer on that port. A fixed `sleep 8`
  races it; poll `curl -s http://127.0.0.1:9222/json` if you would rather not guess.
- **Write screenshots to `.tmp/` in the repo**, not `/tmp` — they stay easy to open and to
  point someone at. `.tmp/` is gitignored.
- `--use-gl=swiftshader --enable-unsafe-swiftshader` is **required**: egui renders to a WebGL
  canvas and headless has no GPU. `GPU stall due to ReadPixels` on stderr is noise, not
  failure.
- The settle argument (ms) is the wait *before* the capture: 7000 for wasm boot and the remote
  font fetch, longer for a live app. Too low gives a blank canvas.
- A shot of the same URL twice does not re-navigate — Chromium treats a hash-only change as a
  fragment jump, so the wasm app never re-reads it. Vary a query parameter per shot
  (`?nav=0&x=2#/story`) or the second screenshot is the first one again.
- `cdp-shot.mjs` takes click coordinates after the settle, and `hover:X,Y` to move without
  clicking. This is how a modal is shot at all: `--screenshot` mode delivers no events, so a
  modal widget captures as the button that opens it.

## 3. Narrow widths — `--window-size` cannot do this

Chromium headless **floors the window near 620px**, lays out at the floor, then **crops the
capture** to the width you asked for. A "390px" shot is therefore a wide layout with its right
edge sliced off — which reads as an overflow bug that is not there, and hides the real one.

Two things are needed:

1. **`?nav=0`** drops the storybook sidebar, so the story gets the whole viewport instead of
   `viewport − 180`.
2. **CDP `Emulation.setDeviceMetricsOverride`** sets a real layout viewport — the same
   `cdp-shot.mjs` above, with the width and height you want:

```sh
direnv exec . env NIX_HARDENING_ENABLE= node ui/_storybook-egui/tools/cdp-shot.mjs \
  "http://127.0.0.1:8095/?nav=0#/activity-feed" 390 844 .tmp/activity-feed-390.png 7000
```

`--user-data-dir` is required alongside `--remote-debugging-port`, or Brave refuses to expose
the port. `trunk serve` does rebuild on change, so a screenshot taken while it is rebuilding
captures the previous build — check its log for `✅ success` first.

## 4. Check it against real data

The same helper points at a **deployed app** — pass a longer settle for socket and first
payload. This is the only way to check a widget against real data at real width, and real data
is wider than fixture data: that is how a wrapped 60-character stake address shipped.

## Native builds

No address bar. Use `STORYBOOK_STORY=flow-ledger` instead of the hash.
