---
name: game-spike
description: Use when starting a new game, a new game screen, or a visual pass on one, in a defrag repo that draws with macroquad. The procedure that produced fight-macroquad — spec first, pure rules crate with events, thin client on macroquad-gamekit and its harness, capture-and-read review loop, soak, web build.
---

# Building a game screen

The reference is `fight-macroquad` in augminted-bots (`games/_spikes/apps/fight-macroquad`), on
`macroquad-gamekit` in shared-crates. Read its `main.rs` before writing a shell; copy its shape.

Every command runs through the devshell: `nix develop -c <cmd>` from the repo root, or
`direnv exec . <cmd>` where the nix daemon socket is unreachable.

## 1. Write the screen spec first

A markdown spec beside the code, before any client: the design size (portrait 390×844 for phone
first), every element's position and size as numbers, the timings of each animation, and what the
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

## 3. A thin client on the gamekit and its harness

Read `shared-crates/ui/macroquad-gamekit/CATALOG.md` first, and the rule `defrag-macroquad-traps`.

- `harness::Args::read()` for the review flags; the game reads its own (seed, level) with
  `harness::flag_num`.
- `harness::Viewport::current(DESIGN)`; `window_camera` on the window; `Capture::window` when
  `--shot` is due; `frame_dt(args.capturing())` for the clock.
- If `args.capturing()`, call `harness::report::enable()` before loading anything.
- Draw text only through the kit's `RichText`/`TextLayout`, so the report sees it.
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

Web: build with the repo's bundling script (augminted-bots: rule
`augminted-macroquad-client-bundles`), report the wasm size raw and gzipped, and confirm no
wasm-bindgen import:

```sh
grep -c "__wbindgen" <dist>/<game>.wasm    # must print 0
```

## 6. Write it down

Keep a short journal beside the client (`NOTES.md`): what was tried, what each capture showed,
which traps were hit. A trap that is not already in `defrag-macroquad-traps` goes there, through
the `capture-rule` skill, so the next game does not hit it.
