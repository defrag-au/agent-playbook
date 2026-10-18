---
name: game-spike
description: Use when starting a new game, a new game screen, or a visual pass on one, in a defrag repo that draws with macroquad. The procedure that produced fight-macroquad — spec first, pure rules crate with events, thin client on macroquad-gamekit and its harness, capture-and-read review loop, soak, web build.
argument-hint: <game idea, e.g. "a ToeJam-style island wander, two-player split-screen">
---

# Building a game screen

The reference is `fight-macroquad` in augminted-bots (`games/_spikes/apps/fight-macroquad`), on
`macroquad-gamekit` in shared-crates. Read its `main.rs` before writing a shell; copy its shape.

Every command runs through the devshell: `nix develop -c <cmd>` from the repo root, or
`direnv exec . <cmd>` where the nix daemon socket is unreachable.

## 1. Write the screen spec first

A markdown spec beside the code, before any client: the design size (portrait 390×844 for phone
first; landscape 844×390 where the genre is played sideways, as `turbo-turns`' racer is), every
element's position and size as numbers, the timings of each animation, and what the
screen shows in each state. The card game's spec is `games/_spikes/web/README.md` — its card anatomy
table is the model: each element, its size as a fraction of the card width, and why.

Decide readability up front: what must be legible when the thing is partly covered (a card in a
fan shows only its top-left strip, so cost, kind and numbers live there).

## 2. Rules in a pure crate that reports events

See the rules `defrag-runtime-pairs` and `defrag-rules-emit-events`. In short:

- No macroquad, no wasm-bindgen, no I/O. `cargo tree --target wasm32-unknown-unknown -p <crate>`
  shows no wasm-bindgen.
- A step returns typed events plus the new view. Numbers and icons arrive as data, not prose.
- A golden test (a seeded game played to the end) and a replay test, before any client exists.
- A catalogue function (like `deck_fight::pack_cards`) lists every face the client can show.
- Physics that decides an outcome (who is knocked off the screen) is a rule: it lives here, not in
  the client or the gamekit. Keep it replayable: `+ - * /` and `sqrt` only, no platform
  `sin`/`cos`/`atan2` (`turbo-sim`'s `angle.rs` turns headings into vectors with polynomials).
- **Feel tests** for anything the player feels: set up the moment by hand, run it, print the number
  and pin a range (`turbo-sim`: a full-speed knock moves a car 70 units and turns it 128°; a hoop
  is 34 ticks off the table). Tune against those numbers, and a "feels soft" report becomes a
  value to move.
- CPU drivers that never err make modes that depend on mistakes (an elimination screen, a race
  gap) stall. Give them seeded lapses, and assert in the golden test that the mode resolves.

## 3. A thin client on the gamekit and its harness

Read `shared-crates/ui/macroquad-gamekit/CATALOG.md` first, and the rule `defrag-macroquad-traps`.

- **Never write the frame loop.** Implement the kit's `app::Game` (`update`, `draw`, `backdrop`,
  and as needed `ready`, `capture_due`, `capture_note`, `shake`) and hand it to `app::run(DESIGN,
  game)`. The loop brings the clock, `--shot`/`--at`/`--size` captures, settling while a tiling
  window manager resizes the window, the glyph gate, a preloader cover until the screen draws
  whole, `--frame-log`, `--seconds N` and the native 60 FPS cap. `fight-macroquad`, `backrooms`
  and `turbo-turns` are the references.
- **Pick the stage.** `app::run` draws in design units at the window's device pixels. A
  pixel-art game runs on `app::run_on(Stage::Pixel { width, height }, game)`: the kit owns the
  canvas, scales it by whole numbers and letterboxes it, and a `--shot` is the canvas at 1:1
  (`island-funk`). Draw text at `frame.px`, and read the mouse with `frame.pointer()`.
- A game that reviews itself — bots playing to set moments, a screenshot at each — returns true
  from `Game::scripted` and saves each finished frame in `Game::presented` with
  `harness::write_capture` (`island-funk`'s bot).
- The game reads its own flags (seed, level) with `harness::flag_num`; modes that are not the
  game loop (`--soak`, `--sheet`) stay in `main`, before `app::run`.
- Load fonts with the kit's `load_font`, so the glyph gate can warm them.
- Draw text only through the kit — `RichText`/`TextLayout`, or `draw_run` for one line — so the
  report sees it. Measure with `MeasureAt(frame.px)`.
- A game that binds render targets of its own (a light map, split-screen halves) wraps them in
  `push_camera_state`/`pop_camera_state`, so `draw` ends on the camera the loop set.
- **Never animate a font size.** Text that pops or pulses keeps one size: bake it once and scale
  the texture through a `Pose` (`turbo-turns`' `hud::Stamps`), or fade it while its shape scales
  (`fight-macroquad`'s `ui::grown`).
- Faces that rarely change are baked with `Baker` and drawn through a `Pose`. For a card game,
  read the kit's `CARD-FACES.md` first: what goes in the strip a fan leaves visible, the layers,
  baking and lighting.
- Map the harness's pointer scripts (`--hover N`, `--drag N`, `--rclick N`) onto the game's
  targets with `Script::pointer`.
- Give the game a `--soak N` (play N seeds to the end through the real screen, checking what is
  shown against the engine every step) and a `--sheet` (every face at once).

## 4. Review loop: capture, read, fix

```sh
nix develop -c cargo build -p <game>
./target/debug/<game> --shot .tmp/s.png --at 1.25 --hover 2
./target/debug/<game> --sheet .tmp/sheet.png
```

Each capture writes `s.png` and `s.txt`.

- **Look at `s.png`.** Judge it against the spec and against the standard the user asked for:
  readability, colour, depth, alignment, art cropping, things covering other things.
- **Then check `s.txt`'s Problems section**, which catches what a glance misses: text cut short,
  too small, off its surface or overlapping. Fix each, or say why it stands.
- Capture animations at several moments with `--at` (start, middle, end), not only at rest.
- Change one thing, rebuild, recapture the same moment. Keep the captures in `.tmp/`.

## 5. Check the window, soak, build for the web

```sh
./target/debug/<game>                     # the real window: right way up, input lands where aimed
./target/debug/<game> --frame-log         # play through the start: nothing slow after the first second
./target/debug/<game> --soak 20           # must end "0 with a problem"
nix develop -c cargo clippy -p <game> --all-targets -- -D warnings
```

The window check is not optional after any camera, input or window change (rule
`defrag-look-at-the-game`). On macOS, look at the real window by starting the game, taking a
screenshot, and killing the process you started by its PID (`rules/core/kill-only-what-you-started`):

```sh
./target/debug/<game> >/dev/null 2>&1 & pid=$!; sleep 3; screencapture -x .tmp/window.png; kill $pid
```

The screenshot is the whole screen, including the user's desktop: keep it in `.tmp/` and say so.

Captures and soaks run on the fixed clock, so they never show a stall: a frame that took two
seconds looks like any other. `--frame-log` is how a beachball is found (`turbo-turns`' was the
countdown rasterising a new font size every frame).

Web: build with the repo's bundling script (augminted-bots: rule
`augminted-macroquad-client-bundles`), report the wasm size raw and gzipped, and confirm no
wasm-bindgen import:

```sh
grep -c "__wbindgen" <dist>/<game>.wasm    # must print 0
```

- **Sound or pads** come from the kit's `audio` and `gamepads` features. Their JS goes in the page:
  a `build.rs` with `macroquad_gamekit::web::stamp(web_dir, &[Plugin::Audio, Plugin::Gamepads])`,
  and the bundling script's `--web` pointing at that directory. `island-funk` is the shape.
- **Every import needs a script.** The bundler (`cargo run -p mq-bundle`, shared-crates'
  `macroquad-bundle`; augminted-bots wraps it as `scripts/stage-macroquad-client.sh`) fails a
  bundle whose wasm imports a function no staged script provides. `mq-bundle check <dir> <wasm>`
  checks a bundle staged some other way.
- **Then load it in a browser and read the console.** A web build that passes every native check
  and capture can still die on its first frame: `fight-macroquad` did, on a WebGL-1-only call.
  Expect at most a notice that an unused `sapp_jsutils` is present, and the audio autoplay warning
  until the first key press. Headless works without a window on the user's desktop: start
  Chrome or Brave with `--headless=new --remote-debugging-port=<port>`, then read
  `Runtime.consoleAPICalled` and `Runtime.exceptionThrown` and take `Page.captureScreenshot` over
  the DevTools protocol. `--screenshot` with `--virtual-time-budget` hangs on a game's frame loop.

## 6. Write it down

Keep a short journal beside the client (`NOTES.md`): what was tried, what each capture showed,
which traps were hit. A trap that is not already in `defrag-macroquad-traps` goes there, through
the `capture-rule` skill, so the next game does not hit it.
