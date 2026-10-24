---
id: defrag-macroquad-traps
title: macroquad's silent traps — each fails with no error and a wrong picture
layer: org
activation: renders:macroquad
priority: 52
overrides:
targets:
---

## Directive

Before drawing with macroquad directly, check the pieces of `macroquad-gamekit` that already handle
these (`shared-crates/gaming/macroquad-gamekit/CATALOG.md`). Each trap compiles, runs, and shows a wrong
picture with no error.

- **The window is upside down under `Camera2D::from_display_rect`.** It runs y down into a render
  target and up on the window. Use `harness::window_camera` for the window and
  `harness::target_camera` for a target. A capture draws into a target, so **a capture that looks
  right says nothing about the window.**
- **Dropping a `RenderTarget` frees its texture**, and the next texture reuses the id, so a kept
  `Texture2D` silently becomes something else. Hold the `RenderTarget` (`Baked` does).
- **A render target's texture is upside down** when drawn: draw it with `flip_y: true`.
- **Translucent layers wear down a target's alpha**, because the default pipeline blends alpha like
  colour. A layered face comes out see-through. Bake layered art with `Baker`, which seals it. A
  whole-frame capture wears down the same way and looks washed out against a viewer's white while
  the window is fine; `Capture::save` writes the PNG opaque, so capture through it.
- **A capture is only the size you asked for if the window is.** A tiling window manager
  (AeroSpace) retiles the window, so `Viewport::current` and every `--shot` follow the tile, not
  `--size`. Take the viewport from `harness::Args::viewport`, which uses `--size` when capturing.
- **Every new glyph re-uploads the whole font atlas.** Caching a glyph marks the atlas dirty and
  the next `draw_text_ex` uploads all of it; draws batch to the end of the frame but uploads do
  not. A text-heavy first frame uploaded a 2048² atlas 166 times, and macOS's OpenGL kept each
  16 MB staging copy: gigabytes of graphics memory, 43 GB and a Metal out-of-memory in release.
  Run the screen through `macroquad_gamekit::app::run`, which turns on the kit's glyph gate (text
  waits a frame for its glyphs, cached in one batch after the last draw), and load fonts with the
  kit's `load_font` so the gate can warm them.
- **Text queued before the glyph atlas grows vanishes.** The atlas doubles and deletes its old
  texture mid-batch, typically on the first large text at a new size. Under the glyph gate it grows
  only between frames. A bake runs ungated (a face missing its text would be missing it for good,
  not for a frame), so `Baker` still draws twice for this.
- **Bake text that must never wait a frame.** Under the gate, text at a size or character not seen
  before appears one frame late; large text (over 64 device pixels) warms its digits as a set and
  every other character one at a time, so a title's letters each pop in once. A countdown or a
  banner that must be whole on its first frame belongs in a `Baker` face, scaled through a `Pose`.
- **A capture can outrun the glyph gate and photograph an empty line.** `capture_due` is asked
  BEFORE the frame is drawn, so a sentence that first appears on that frame — a live quote's
  failure reason, a wallet's error — is held back and never recorded: the PNG has an empty panel
  and the report's Problems list is empty too. Wait until the state has been unchanged for two
  drawn frames, not just until `--at`. The same gate makes a truncated string at a size above 64
  device pixels change length while it is measured (letters cache one at a time), so the `fit`
  that cut it re-cuts differently each frame and never settles; draw a sentence at a body size,
  where the whole ASCII set warms at once.
- **macroquad 0.4.16 doubles a glyph atlas at random.** A glyph landing within 2 px of a row's end
  fails the bounds check and doubles the whole atlas instead of wrapping, so one run in twelve
  ended at 8192² (256 MB). Upstream fixed it after 0.4.16 (`8d602d3`), so every workspace
  building the kit carries `[patch.crates-io] macroquad = { git =
  "https://github.com/not-fl3/macroquad", rev = "8d602d3f8ebb8a3a582ee15d92983bbb81ee3ef6" }`
  until a release has it: a `[patch]` only applies in the workspace that builds.
- **Never animate a font size.** Each distinct pixel size rasterises a new glyph set into the
  atlas, so text that pops or grows by drawing at `size × scale` rasterises every frame: a 96-point
  countdown pop stalled the game 2.2 s (a macOS beachball), in release as in debug. Draw it with
  `TextLayout::draw_scaled`, which rasterises once and scales the quads, or bake it once with
  `Baker` and scale the texture through a `Pose`. Captures run on a fixed clock and never show the
  stall; play with `--frame-log` to see it.
- **Fractional text sizes must measure and draw at the same size.** Use the kit's `RichText` with
  `MeasureAt(pixel_scale)`, which measures the glyphs the draw will use, rather than `draw_text_ex`
  with a rounded `font_size`, or words overlap and spaces disappear.
- **Text centred by its size or its line box sits high.** UI labels are mostly capitals and digits,
  and the eye centres their ink: the band from the baseline to the top of a capital. The point size
  is much taller than that band (VT323's capitals are about half of it), and `text_top`'s line box
  hangs a descender below it, so `top = box.y + (box.h - size) / 2` or a hand-tuned `+4` leaves a
  stamp's word high, a legend's square low beside its word, and a different error in every face.
  Place text by a midline instead: `Painter::text_in(s, rect, size, color, Align)` for one line
  against a box's start, centre or end (a chip, a button, a figure right against a tile's edge),
  `Painter::stack_in(&[Line], rect, align, gap)` for a block such as a list row's title over its
  subtitle, and `text_mid` / `mono_mid` on a row's midline with the icon beside it centred on the
  same `y`. The widgets catalogue's "Text placement" section lists every one. Lay a card out as
  named midlines (`title_mid`, `legend_mid`) and derive its height from the last one, rather than
  fixed offsets from its top.
- **Images decode from PNG and TGA only.** Convert art to PNG at build time, not JPEG.
- **With `high_dpi`, touches arrive in device pixels and the mouse in points.** For one pointer,
  read the mouse (macroquad turns touches into mouse events) and convert once, through
  `Viewport::to_design`. For several fingers (a stick and a button at once), read
  `macroquad_widgets::touch_points()`, never `touches()`: it returns them in points. On a 3x phone a
  raw touch lands three times too far out, so a stick is three times too sensitive and a button
  cannot be hit where it is drawn, while a desktop browser's device emulation, driven by the mouse,
  looks fine. Island Funk's first touch controls shipped like that.
- **A touch has one phase a frame, its last event's.** macroquad stores touches in a map, so a
  finger that lands and moves before the next frame arrives as `Moved`, never `Started`, and one
  that lands and lifts arrives only as `Ended`. On a phone a landing thumb nearly always moves a
  little, so code that waits for `Started` drops touches at random: touch-and-hold works one time in
  a few. Treat a finger seen for the first time as landing, whatever its phase. The kit's `Thumbs`
  and the widgets' `Gestures` and `FloatingStick` do.
- **`get_screen_data` returns black on macOS** when the window is unfocused or not yet composited,
  and works when it is: so it passes at the desk and fails in a background run. Capture through a
  render target: `harness::Capture`, or `harness::save_texture_png` for a game that already
  composes its frame in a target of its own.
- **`draw_arc` thickness grows outward from the radius**, not centred on it.
- **miniquad's log macros print a lone literal verbatim.** `macroquad::logging::info!("refused:
  {why}")` logs the braces, not the reason: with nothing after the string the macro passes it
  through unformatted, and it still compiles. Pass the value positionally (`"refused: {}", why`)
  whenever the string is the only argument. The `log` and `tracing` macros do not have this trap.

These pass every native run and capture, and fail only in the browser:

- **`render_target()` kills the web build.** In macroquad 0.4 it defaults to `sample_count: 1`,
  which `render_target_ex` treats as multisampled, so every pass ends in a `glReadBuffer` blit.
  WebGL 1, miniquad's default in the browser, lacks it: "gl.readBuffer is not a function" on the
  first frame, then a panic at miniquad's `wasm.rs:34` on every later event. Use
  `harness::plain_target` (sample count 0). The kit's `tests/plain_targets.rs` scan fails on a
  bare call.
- **A missing miniquad plugin never stops the page loading.** `gl.js` stubs each import nothing
  provides and logs "No <name> function in gl.js" when it is called: no quad-snd `audio.js` is a
  silent game, no gamepads plugin is deaf pads, no `quad-net.js` a dead socket. Take audio and
  gamepads from the kit's `audio` and `gamepads` features, and stamp their JS from `build.rs` with
  `macroquad_gamekit::web::stamp` — never a CDN copy.
- **A copied `gl.js` drifts from the miniquad it runs beside**, and a mismatch is a blank canvas
  with no error (cnft.dev-workers committed 0.4.10's beside a 0.4.11 wasm). Never commit or
  hand-copy miniquad's JS, or write a bundle's `index.html` by hand. Bundle with shared-crates'
  `macroquad-bundle`, run in each repo as `cargo run -p mq-bundle` (`tools/mq-bundle`): it takes
  the JS from the versions the package links, renders the shared shell, and fails the build if the
  wasm imports a function no staged script provides. `--base /path/` for a page served at a bare
  path.
- **`set_cursor_grab(true)` at load breaks the page.** A browser grants pointer lock only after a
  user gesture. Refused, miniquad keeps asking until Chrome rate-limits it, and the shared shell
  reports the rejection as a boot error over a black canvas. Grab the pointer on the first click,
  natively too or behind `cfg!(target_arch = "wasm32")`.

## Rationale

Every item in the first list happened in the 2026-10 `fight-macroquad` spike in augminted-bots, and
none of them raised an error. The two browser-only traps were found by the `island-funk` spike on
2026-10-17, and the render-target one was then confirmed to break `fight-macroquad`'s web build,
which had passed every native check. The first five each cost a capture-and-look cycle to diagnose. The upside-down
window shipped past twenty clean captures and a twenty-seed soak, because both drew into render
targets; the user found it on the first manual play. The washed-out capture and the retiled
window were found by the `turbo-turns` spike on 2026-10-18: alpha read back at 0.8 over a grained
table, and every capture came out 844×970 whatever `--size` said. The same spike's countdown
froze the window at "GO!": captures run on a fixed clock and never show a stall, so it took a log
of wall-clock frame times to find. Porting those fixes to the other spikes on 2026-10-18 found the
backrooms spike still on `sample_count: 1`, missing its audio plugin, and grabbing the pointer
at load; once the first two were fixed, the third was what kept its page black.

The atlas upload storm and the random atlas doubling were found by solada-ledger's replay client
on 2026-10-18, by counting uploads in an instrumented macroquad: 129 retained 16 MB regions
matched the 166 full-atlas uploads of one frame. With the gate and the upstream packing fix,
both of its views held about 19 MB of graphics memory on every run.

The high-text trap was found by the BLOCKWATCH rework in cnft.dev-workers on 2026-10-24: every
capture report was clean, and the user still saw a stamp's word riding high, a legend's squares
below their words, and `block_train`'s "full" above its status light. The report checks that text
fits its surface, not that it is centred in its box, so only the picture shows it. It came back
the same day in TOKEN ARENA, built from scratch: chips, buttons and feed rows placed with
`text_top` plus offsets, all off by a point or two. The midline helpers existed, but the painter
had no row in the widgets catalogue, so nothing pointed at them; it now has a generated "Text
placement" section, and `text_in` / `stack_in` cover the one-line and two-line box cases.

The fixes live in the kit so a new game inherits them. The camera orientation is pinned by
`the_window_camera_puts_the_origin_top_left` in `harness.rs`, which checks the camera's matrix and
needs no window.
