---
id: defrag-egui-icons-only
title: Never use raw Unicode symbols in egui
layer: org
activation: org:defrag
priority: 44
overrides:
targets:
---

**Never use raw Unicode symbols** — `●` `○` `✓` `✕` `→` `★` and friends. They render as
broken boxes in the browser, because neither the default egui font nor the Phosphor font
includes the geometric and symbol Unicode blocks.

Use `PhosphorIcon` from `icons.rs`:

| Instead of | Use |
| --- | --- |
| `✓`, `●` | `PhosphorIcon::CheckCircle` |
| `✕`, `×` | `PhosphorIcon::X` |
| `○`, `◌` | `PhosphorIcon::Clock` |
| `⚠` | `PhosphorIcon::Warning` |
| `+`, `−` | `PhosphorIcon::Plus` / `PhosphorIcon::Minus` |
| `→` | `PhosphorIcon::ArrowRight` |

Basic ASCII (`!`, `?`, `#`, `+`, `-`) is fine.

## Adding an icon

Look the codepoint up in the Phosphor CSS
(`https://unpkg.com/@phosphor-icons/web@2.1.1/src/regular/style.css`), then add the variant
to `PhosphorIcon` in `icons.rs` — enum, `codepoint()`, `ALL`, `name()`.

## Why it is a rule and not a nitpick

The failure is invisible in a native build on a machine whose system fonts happen to cover the
glyph, and visible to every user in a browser. "It looked fine for me" is the default outcome
of testing it the wrong way.
