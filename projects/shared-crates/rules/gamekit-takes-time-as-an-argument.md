---
id: shared-crates-gamekit-takes-time-as-an-argument
title: macroquad-gamekit never reads the clock, and every module has a catalogue line
layer: project
activation: project:shared-crates
priority: 50
overrides:
targets:
---

## Directive

**Nothing in `ui/macroquad-gamekit` calls `get_time`, `get_frame_time` or `Instant`.** Time is a
parameter (`update(dt)`, `value(t)`, `phase`), owned by the game. The one exception is
`harness::frame_dt`, which exists to choose between the fixed capture clock and the real one.

- A new module gets a `//!` header whose first line is its catalogue line, then
  `UPDATE_CATALOG=1 nix develop -c cargo test -p macroquad-gamekit --test catalog`.
- A visual module gets a story in `macroquad-storybook` under "gamekit".
- Text goes through `TextLayout::draw`, so the harness report sees it. A module that draws text
  another way is invisible to every reviewer that reads the report.

## Rationale

Capturing an exact moment (`--at 1.25`) is what made the card pass reviewable, and it only works
because no animation reads the clock: the fixed 1/60 s step makes 1.25 s the same frame every run.
One `get_time()` in a tween would make captures irreproducible and its tests timing-dependent.
