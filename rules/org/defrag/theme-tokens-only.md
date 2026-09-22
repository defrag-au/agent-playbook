---
id: defrag-theme-tokens-only
title: Sizes, spacing and colour come from the theme
layer: org
activation: org:defrag
priority: 58
overrides:
targets:
---

## Directive

No widget writes a point size down. No renderer crate copies a palette or a ramp.

### Sizes resolve through the ramp

```rust
// no
ui.label(RichText::new("total").size(11.0));

// yes
ui.label(RichText::new("total").text_size(ui.text_size(TextSize::Base)));
```

- A config struct holds a `TextSize`, not an `f32`, and resolves it in `show()` where a `Ui`
  finally exists — `route_quote` and `pool_inspector` are the shapes to copy.
- A genuinely non-textual size (a pixel dimension, e.g. `ImageStack::size(96.0)`) opts out with a
  trailing `// theme-exempt: <reason>`.

### The vocabulary lives in one place

`ui/ui-theme` owns colour tokens, `Ink`, the colour science and the type ramp. Each renderer
aliases it and implements `Palette`, so an `Ink` written on either side resolves on both. Nothing
in that crate names a renderer.

**Never copy a palette or a ramp into a renderer crate.** The same applies to `Space`, `Radius` and
`Breakpoint`.

### The `f32` ratchet

`f32` config fields are a ratchet — the count may only go down. Convert one, lower the baseline.

### Changing a ramp is a restyle, not a refactor

Move a ramp as its own change, with the reason written down. Never during a migration.

## Rationale

**The duplication was measured, not assumed.** When the colour model came to be maintained twice,
six colours were byte-identical, three had diverged, `success` was a different hue per side, and
relative luminance was implemented four times in two precisions.

**Why named steps rather than numbers.** The point of a named step — `Space`, `Radius`,
`Breakpoint` — is that one place decides what it means, which is what a copied literal gives up.

**Why the tests are source scans.** `egui-widgets/tests/theme_tokens.rs` and
`macroquad-widgets/tests/text_sizes.rs` scan sources because clippy cannot do this:
`disallowed_methods` matches a method *path*, not its arguments, so it cannot tell `.size(11.0)`
from `.size(ui.text_size(…))` — and banning `RichText::size` outright would ban the blessed form
too. A source scan is exact and runs in `cargo test`.

**Why the `f32` count is a ratchet.** The remaining `f32` config fields are the unconverted
remainder of the migration; a baseline that may only go down is what stops the count drifting back
up one convenience field at a time.

**Why a ramp change is a restyle.** `TextScale::canvas` was **derived from the sources**, not
chosen — the 73 sizes that crate used land on exactly eight values, so those are the eight. Picking
a nicer-looking ramp during a migration would have restyled every macroquad surface under cover of
a rename.