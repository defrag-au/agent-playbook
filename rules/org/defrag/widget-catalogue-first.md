---
id: defrag-widget-catalogue-first
title: Read the widget catalogue before building any UI
layer: org
activation: org:defrag
priority: 62
overrides:
targets:
---

## Directive

**Before writing any widget, read the catalogue.**

- `~/code/defrag/shared-crates/ui/egui-widgets/CATALOG.md` — ~100 widgets, one line each
- `~/code/defrag/shared-crates/ui/macroquad-widgets/CATALOG.md` — the macroquad set

Read the whole file. **Do not grep instead.**

- About to hand-build something widget-shaped — a chip, an id display, a label-value grid, a card,
  a chart, a stat strip → scan first. It probably exists.
- New widget genuinely warranted → build it in `egui-widgets` with a `//!` header, add a storybook
  story, regenerate the catalogue. The three-step checklist is at the bottom of `WIDGETS.md`.
- Both catalogues are generated from each module's own `//!` header by `tests/catalog.rs`, and a
  test asserts the committed copy matches. A module without a
  `//! \`Name\` — one-line purpose.` header **fails** the test. Keep the first sentence a summary:
  it is cut at the first full stop, so detail belongs in the paragraphs below.

```sh
UPDATE_CATALOG=1 nix develop -c cargo test -p egui-widgets --test catalog
```

## Rationale

**Grep is how the duplicate got built.** `IdPill` — middle-elided identifier plus a copy button —
was reimplemented inline, and worse, in a project that already depended on the crate containing it.
Grep only finds the name you already guessed, which is why the instruction is to read the whole
file rather than search it.

**The catalogue is a checked-in artefact.** The committed copy matching the generated one is the
same contract as `cargo fmt --check`, so a stale catalogue is a failing test rather than a stale
document.

**The cost of not knowing.** A wrapped 60-character stake address shipped for weeks because nobody
knew the widget already existed.

**Why the catalogue test fails on a missing header.** An undiscoverable widget is a widget that
gets built twice, so the failure is deliberate: the header is the index entry, and a module without
one is invisible to the next person scanning the catalogue. That is also why the first sentence is
cut at the full stop — detail below it does not bloat the index.