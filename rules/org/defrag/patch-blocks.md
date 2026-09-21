---
id: defrag-patch-blocks
title: Toggle local [patch] blocks as a unit
layer: org
activation: org:defrag
priority: 60
overrides:
targets:
---

## Directive

Engaging a local `[patch]` override block in `Cargo.toml` — to consume a sibling working tree
instead of the pinned git rev → uncomment the **entire `[patch."…"]` table as a unit**.

- Never selectively un-comment individual entries.
- Re-commenting before commit → re-comment the whole block. Never leave a partial active block
  behind.

Selective edits cause two failures:

- A line ends up in both the active and the still-commented copy → duplicate-key error
- A transitively-required crate is left commented → unrelated workers break

## Rationale

The failure is never at the patch site. It shows up as a build error in a crate you did not touch,
or as a duplicate-key error whose line number points at the copy you did not mean to edit. Both
cost more to diagnose than the block took to toggle, which is why the whole-table toggle is worth a
rule rather than a preference — toggling the whole `[patch."…"]` table on and off is the only safe
shape.