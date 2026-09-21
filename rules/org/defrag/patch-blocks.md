---
id: defrag-patch-blocks
title: Toggle local [patch] blocks as a unit
layer: org
activation: org:defrag
priority: 60
overrides:
targets:
---

When engaging a local `[patch]` override block in `Cargo.toml` — to consume a sibling working
tree instead of the pinned git rev — **uncomment the entire block as a unit.** Never
selectively un-comment individual entries.

Selective edits cause two failures:

- A line ends up in both the active and the still-commented copy → duplicate-key error
- A transitively-required crate is left commented → unrelated workers break

Toggling the whole `[patch."…"]` table on and off is the only safe shape.

The same applies in reverse: when re-commenting before commit, re-comment the whole block.
Never leave a partial active block behind.

## Why it is worth a rule

The failure is never at the patch site. It shows up as a build error in a crate you did not
touch, or as a duplicate-key error whose line number points at the copy you did not mean to
edit. Both cost more to diagnose than the block took to toggle.
