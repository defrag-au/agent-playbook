---
id: defrag-egui-layout-traps
title: egui layout traps that cost an afternoon
layer: org
activation: org:defrag
priority: 45
overrides:
targets:
---

## Directive

Read before laying out a panel, sizing a `Ui`, or wiring an async result into a widget.

### A detail pane beside content → `detail_split`, not a right-hand `Panel`

A right `Panel` reserves its strip by shrinking the parent's `cursor.max.x`, and a **top-down**
`Ui` never reads `cursor.max.x`. The reservation is dropped, the following `CentralPanel` takes
full width, and the pane floats over the content's right edge — hiding exactly the column a reader
came for.

Panels want a `Ui` that is arbitrating a whole region, not one you are laying out yourself
mid-column.

### `Color32` stores PREMULTIPLIED channels

Each channel must be `<= alpha`. `from_rgba_premultiplied` with larger channels blends additively
and renders far lighter than intended.

### Images load when the widget is BUILT, not when it is drawn

`ui.add(Image::new(url))` in a long list starts a fetch for every row, including those below the
fold. Reserve the space, then gate on `ui.is_rect_visible` — `activity_feed` is the shape to copy.

### `Ui::set_max_width` WIDENS a `Ui` that has less room

It assigns `max_rect.max.x` outright rather than taking a minimum, so `set_max_width(520.0)` inside
a 342pt phone lays out at 520 and overflows off both edges. Use `viewport::fit(ui, 520.0)`.

### An overflowing `ui.horizontal` widens the parent

It does not just clip: everything drawn *after* it inherits the inflated width. When a row might
not fit, it is `horizontal_wrapped`.

### egui repaints ON DEMAND — an async result can sit unread indefinitely

A fetch that completes on a JS callback and pushes to an `mpsc` wakes nothing; the channel is only
drained on the next frame. Either wake the `Context` at the point of send, or tick
`request_repaint_after` while work is outstanding — and key that tick off an explicit *pending*
flag, **never** off `data.is_none()`, which is also what failure looks like and will spin forever
on battery.

### Compact-breakpoint touch sizing inflates non-interactive content too

`spacing.interact_size.y = 44` is a floor on allocated space *and* sets row height in
`horizontal`/`horizontal_wrapped`. Opt a dense region out with
`ui.spacing_mut().interact_size = Vec2::ZERO` — on the region, not inside the chip, because by then
the row height is already decided.

## Rationale

These are catalogued modules, so [`defrag-widget-catalogue-first`](widget-catalogue-first.md) already
covers *finding* them — these are the ones that look correct, compile, and are wrong at runtime.

The mechanism behind the first trap: `Layout::available_from_cursor_max_rect` takes only `min.y` in
its `TopDown` arm, so the reservation a right `Panel` makes is never read.

`AccessGate` shipped with the `set_max_width` bug — the one screen whose job is explaining how to
get in was clipped on every phone.

`egui-widgets/tests/contrast.rs` asserts the premultiplied-channel rule for the theme, and the
`theme_states` story shows the interaction states a resting-state story cannot.

The `ui.horizontal` overflow was measured: a legend row running 80pt long took a 180pt-tall
scatter plot off-screen with it, and made the wrapping paragraphs below stop wrapping.

The repaint trap is invisible on a desktop, where the first mouse move hides it completely. On a
phone nothing moves — collection-ownership's index sat on "Loading collections…" forever with the
response already in memory.

The touch-sizing trap showed up as read-only status chips coming out as 34pt squares around 10pt
text.
