---
id: augminted-macroquad-client-bundles
title: A macroquad client is bundled by one script, and served by its own worker
layer: project
activation: project:augminted-bots
priority: 55
overrides:
targets:
---

## Directive

**Every macroquad web bundle is made by `scripts/stage-macroquad-client.sh`.** Never hand-write a
bundler, an `index.html`, or a copy step for `gl.js`.

```sh
scripts/stage-macroquad-client.sh --package <crate> --wasm <bin>.wasm \
    --title "<Title>" --dest <dir> --web <crate>/web
```

It is a wrapper over `cargo run -p mq-bundle` (`tools/mq-bundle`, over shared-crates'
`macroquad-bundle`, which cnft.dev-workers runs too). That builds with `--allow-undefined`,
takes `gl.js` / `sapp_jsutils.js` / `quad-net.js` from the versions the package links, renders
the shared shell (`ui/macroquad-bundle/shell.html` in shared-crates), content-hashes the wasm into
`load()`, and fails the bundle if the wasm imports a function no staged script provides. Need
something it does not do → change `macroquad-bundle`, so every bundle in every repo learns it.

A client served by a worker — the default for anything that talks to one:

- **Plugins come from `build.rs`.** Stamp each plugin crate's `PLUGIN_JS` into the client's
  `web/` (`miniquad_platform::PLUGIN_JS` → `web/platform.js`); the script copies `web/*.js`.
  Never hand-copy a plugin file. Sound and gamepads come from macroquad-gamekit's features, and
  their JS from `macroquad_gamekit::web::stamp` in the same `build.rs`.
- **The worker's `build.sh` builds the worker first, then stages the client** into `dist/` —
  `build-worker.sh` regenerates `dist/`. `wrangler.toml` points `[build] command` at it.
- **`[assets]` with `run_worker_first`** listing every worker route, or the asset handler
  answers them with a 404 before the worker runs. `not_found_handling = "none"`.
- **Same origin, no configuration.** The client reaches its worker by a relative URL (a
  socket at `/ws?…`); launch settings come from the page query via
  `miniquad_platform::launch_query`, never baked in when they are per player.
- **A macroquad client is not a `frontend/`.** `build-worker.sh` runs `trunk` on any
  `frontend/` holding a `Cargo.toml`; put a macroquad client in `client/` (or keep it under
  `games/*/apps/`).

Copy: `workers/crawl/build.sh` + `wrangler.toml`, or `activities/missions/`.

## Rationale

Three bundlers (arcade, missions, crawler) each repeated the same staging, and each step was
learned by losing a debugging round: a CDN `gl.js` desyncing from the linked miniquad (blank
canvas, no error), an unversioned `load("x.wasm")` serving a stale build, a stale plugin left
beside a wasm that no longer imported it. Then the copies drifted: every one detected quad-net
by its HTTP import only, so socket-only clients (the crawler, the missions ui-flow probe) were
staged with no `quad-net.js`. miniquad's `gl.js` stubs a missing import instead of refusing to
instantiate, so the page booted and the first socket call quietly did nothing. One script means
that lesson was learned once, for all of them (2026-10-16). The same stubbing made island-funk
silent for want of quad-snd's `audio.js` (2026-10-17), which is why the script now checks every
import rather than detecting one plugin at a time; run over every bundle on disk, it also caught
two stale ones still missing `quad-net.js`.

Same origin is what a Discord Activity's CSP requires, and it removes the "where is my worker"
setting from the client entirely.
