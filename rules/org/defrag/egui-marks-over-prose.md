---
id: defrag-egui-marks-over-prose
title: Let the marks carry it — egui is weak at prose
layer: org
activation: org:defrag
priority: 42
overrides:
targets:
---

egui has no real text shaping and poor typographic hierarchy, so **blocks of text are the
wrong tool.** If a widget is explaining itself in paragraphs, the design is wrong, not the
copy. Reach for an encoding instead:

| Instead of | Use |
| --- | --- |
| state described in a sentence | a pip track or progress marks |
| composition spelled out | shaped or coloured marks — see `PartyBadge`'s filled/half/hollow basis language, reused as support pips on `ClaimCard` |
| a number the reader must compare by eye | bar height or width |
| long-form detail | behind an expand, on hover, or in a side panel |

## The test

A list of twenty of these should be **scannable**. If reading twenty means reading twenty
paragraphs, redesign.

Keep at most one line of irreducible text — a title, a statement — and put the rest on demand.

## Applies to story captions too

Storybook captions are held to the same standard: one or two short lines, not an essay. A
caption that needs a paragraph to explain the widget is a caption describing a widget that
needs redesigning.
