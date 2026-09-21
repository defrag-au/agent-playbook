---
id: defrag-widget-catalogue-first
title: Read the widget catalogue before building any UI
layer: org
activation: org:defrag
priority: 62
overrides:
targets:
---

**Before writing any widget, read the catalogue.**

- `~/code/defrag/shared-crates/ui/egui-widgets/CATALOG.md` — ~100 widgets, one line each
- `~/code/defrag/shared-crates/ui/macroquad-widgets/CATALOG.md` — the macroquad set

Read the whole file. **Do not grep instead.** Grep only finds the name you already guessed,
which is exactly how the duplicate got built: `IdPill` — middle-elided identifier plus a copy
button — was reimplemented inline, and worse, in a project that already depended on the crate
containing it. A wrapped 60-character stake address shipped for weeks because nobody knew it
already existed.

If you are about to hand-build something widget-shaped — a chip, an id display, a
label-value grid, a card, a chart, a stat strip — scan first. It probably exists.

## If a new widget is genuinely warranted

Build it in `egui-widgets` with a `//!` header, add a storybook story, and regenerate the
catalogue. The three-step checklist is at the bottom of `WIDGETS.md`.

## Both catalogues are generated

From each module's own `//!` header, by `tests/catalog.rs`, and a test asserts the committed
copy matches — the same contract as `cargo fmt --check`. Adding a widget means giving it a
`//! \`Name\` — one-line purpose.` header; the test **fails** on a module without one, and
that is deliberate. An undiscoverable widget is a widget that gets built twice.

```sh
UPDATE_CATALOG=1 nix develop -c cargo test -p egui-widgets --test catalog
```

Keep the first sentence a summary — it is cut at the first full stop, so detail belongs in
the paragraphs below, where it does not bloat the index.
