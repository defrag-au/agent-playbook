---
id: defrag-egui-layout-traps
title: egui layout traps that cost an afternoon
layer: org
activation: renders:egui
priority: 45
overrides:
targets:
---

## Directive

Read before laying out a panel, sizing a `Ui`, or wiring an async result into a widget.

- **A detail pane beside content → `detail_split`, not a right-hand `Panel`.** A top-down `Ui`
  never reads the reservation, so the following `CentralPanel` takes full width and the pane
  floats over the content.
- **`Color32` stores PREMULTIPLIED channels.** Each channel must be `<= alpha`.
- **Images load when the widget is BUILT, not drawn.** Reserve the space, then gate on
  `ui.is_rect_visible` — `activity_feed` is the shape.
- **`Ui::set_max_width` WIDENS a `Ui` with less room.** Use `viewport::fit(ui, 520.0)`.
- **An overflowing `ui.horizontal` widens the parent.** Use `horizontal_wrapped`.
- **egui repaints ON DEMAND — an async result can sit unread.** Wake the `Context` on send, or
  tick `request_repaint_after` off an explicit pending flag, never off `data.is_none()`.
- **Compact-breakpoint touch sizing inflates non-interactive rows.** Opt out with
  `ui.spacing_mut().interact_size = Vec2::ZERO` on the region.

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
