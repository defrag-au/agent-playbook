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

Before drawing with macroquad directly, check `shared-crates/gaming/macroquad-gamekit/CATALOG.md`.
Each trap below compiles, runs, and shows a wrong picture with no error.

- **The window is upside down under `Camera2D::from_display_rect`.** Use `harness::window_camera`
  for the window and `harness::target_camera` for a target. A capture that looks right says
  nothing about the window.
- **Dropping a `RenderTarget` frees its texture**, and the next texture reuses the id. Hold it.
- **A render target's texture is upside down** when drawn: `flip_y: true`.
- **Translucent layers wear down a target's alpha.** Bake layered art with `Baker`; capture
  through `Capture::save`, which writes opaque.
- **A capture is only the size you asked for if the window is.** Take the viewport from
  `harness::Args::viewport`.
- **Every new glyph re-uploads the whole font atlas.** Run the screen through
  `macroquad_gamekit::app::run` (the glyph gate) and load fonts with `load_font`.
- **Text queued before the atlas grows vanishes.** The gate grows it between frames; `Baker`
  draws twice for the same reason.
- **Bake text that must never wait a frame.** Large text warms one character at a time; use a
  `Baker` face scaled through a `Pose`.
- **A capture can outrun the glyph gate and photograph an empty line.** Wait for two drawn frames
  of unchanged state; draw long sentences at a body size.
- **macroquad 0.4.16 doubles a glyph atlas at random.** Carry the workspace `[patch.crates-io]`
  pinned to `8d602d3f8ebb8a3a582ee15d92983bbb81ee3ef6`.
- **Never animate a font size.** Use `TextLayout::draw_scaled`, or bake once and scale through a
  `Pose`; `--frame-log` shows the stall.
- **Fractional text sizes must measure and draw at the same size.** Use `RichText` with
  `MeasureAt(pixel_scale)`.
- **Text centred by its size or line box sits high.** Place by midlines: `Painter::text_in`,
  `Painter::stack_in`, `text_mid`/`mono_mid`. The catalogue's "Text placement" section lists them.
- **Images decode from PNG and TGA only.** Convert art to PNG at build time.
- **With `high_dpi`, touches are device pixels and the mouse is points.** Convert once through
  `Viewport::to_design`; for several fingers use `macroquad_widgets::touch_points()`.
- **A touch has one phase a frame, its last event's.** Treat a first-seen finger as landing,
  whatever its phase; `Thumbs`, `Gestures` and `FloatingStick` do.
- **`get_screen_data` returns black on macOS** when unfocused or uncomposited. Capture through a
  render target: `harness::Capture` or `harness::save_texture_png`.
- **`draw_arc` thickness grows outward from the radius**, not centred.
- **miniquad's log macros print a lone literal verbatim.** Pass the value positionally
  (`"refused: {}", why`) when the string is the only argument.

These pass every native run and capture, and fail only in the browser:

- **`render_target()` kills the web build.** Use `harness::plain_target` (sample count 0).
- **A missing miniquad plugin never stops the page loading.** Take audio and gamepads from the
  kit's features, and stamp their JS from `build.rs` with `macroquad_gamekit::web::stamp`.
- **A copied `gl.js` drifts from the miniquad beside it.** Bundle with `mq-bundle`; never copy
  the JS or write `index.html` by hand.
- **`set_cursor_grab(true)` at load breaks the page.** Grab the pointer on the first click.

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
