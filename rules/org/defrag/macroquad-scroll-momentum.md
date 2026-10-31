---
id: defrag-macroquad-scroll-momentum
title: A macroquad scrolling container scrolls with momentum, through the widgets' Scroll
layer: org
activation: renders:macroquad
priority: 54
overrides:
targets:
---

## Directive

Every macroquad container that scrolls holds a `macroquad_widgets::Scroll` and reads its offset
from `Scroll::offset`. Never hand-roll an `f32` offset with its own wheel and drag code. A list
that steps a whole row at a time uses `macroquad_widgets::Momentum::rows` instead.

Each frame, before laying out:

```rust
let input = ScrollInput::read(frame.input.gesture.drag, frame.pointer());
self.scroll.update(area, &input, Some(content_len - area.h), frame.dt);
let tap = self.scroll.filter_tap(frame.input.gesture.tap);
```

- Pass `None` for the maximum until the content's length is known, so a position asked for
  early (a `--scroll` flag) is not clamped to nothing.
- Hit-test with the tap from `filter_tap`. A finger that lands on a coasting list only stops it.
- Behind a modal, pass `ScrollInput::none()`. The container then takes no new input, and a coast
  already under way runs out.
- Jump with `Scroll::set` for a reveal, a key or a restored position. It stops any coast.

Between them, `Scroll` and `Momentum` decide which drag is the container's, the flick and its
coast, the wheel in notches or points, the clamped ends, and the tap that only caught a coast.
A hand-rolled offset gets each of these wrong in its own way.

## Rationale

TOKEN ARENA's feed and holders board got `Momentum` and felt like a phone's lists. The staking
site's wall of Perps, the storybook's wall story and sidebar, and the gamekit wall example each
had their own offset instead. Each followed a drag and stopped dead on release, and each scaled
the wheel differently: by half, by 36 points a notch, or not at all. On 2026-10-29 the user asked
for the arena's momentum on the wall, and on macroquad scroll containers in general.
`macroquad-widgets` gained `Scroll` on top of `Momentum`, and all four moved onto it the same day.
