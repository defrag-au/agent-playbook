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
these (`shared-crates/ui/macroquad-gamekit/CATALOG.md`). Each trap compiles, runs, and shows a wrong
picture with no error.

- **The window is upside down under `Camera2D::from_display_rect`.** It runs y down into a render
  target and up on the window. Use `harness::window_camera` for the window and
  `harness::target_camera` for a target. A capture draws into a target, so **a capture that looks
  right says nothing about the window.**
- **Dropping a `RenderTarget` frees its texture**, and the next texture reuses the id, so a kept
  `Texture2D` silently becomes something else. Hold the `RenderTarget` (`Baked` does).
- **A render target's texture is upside down** when drawn: draw it with `flip_y: true`.
- **Translucent layers wear down a target's alpha**, because the default pipeline blends alpha like
  colour. A layered face comes out see-through. Bake layered art with `Baker`, which seals it.
- **Text queued before the glyph atlas grows vanishes.** The atlas doubles and deletes its old
  texture mid-batch, typically on the first large text at a new size. `Baker` draws twice for this.
- **Fractional text sizes must measure and draw at the same size.** Use the kit's `RichText` and
  `MacroquadMeasure` rather than `draw_text_ex` with a rounded `font_size`, or words overlap and
  spaces disappear.
- **Images decode from PNG and TGA only.** Convert art to PNG at build time, not JPEG.
- **With `high_dpi`, touches arrive in device pixels and the mouse in points.** Read the mouse
  (macroquad turns touches into mouse events) and convert once, through `Viewport::to_design`.
- **`get_screen_data` returns black on macOS.** Capture through a render target (`harness::Capture`).
- **`draw_arc` thickness grows outward from the radius**, not centred on it.

## Rationale

Every item above happened in the 2026-10 `fight-macroquad` spike in augminted-bots, and none of
them raised an error. The first five each cost a capture-and-look cycle to diagnose. The upside-down
window shipped past twenty clean captures and a twenty-seed soak, because both drew into render
targets; the user found it on the first manual play.

The fixes live in the kit so a new game inherits them. The camera orientation is pinned by
`the_window_camera_puts_the_origin_top_left` in `harness.rs`, which checks the camera's matrix and
needs no window.
