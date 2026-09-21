---
id: defrag-egui-marks-over-prose
title: Let the marks carry it — egui is weak at prose
layer: org
activation: org:defrag
priority: 42
overrides:
targets:
---

## Directive

Blocks of text are the wrong tool in egui — reach for an encoding instead:

| Instead of | Use |
| --- | --- |
| state described in a sentence | a pip track or progress marks |
| composition spelled out | shaped or coloured marks — see `PartyBadge`'s filled/half/hollow basis language, reused as support pips on `ClaimCard` |
| a number the reader must compare by eye | bar height or width |
| long-form detail | behind an expand, on hover, or in a side panel |

- A list of twenty should be **scannable** — if reading twenty means reading twenty paragraphs,
  redesign.
- Keep at most one line of irreducible text (a title, a statement); put the rest on demand.
- Storybook captions to the same standard: one or two short lines, not an essay.

## Rationale

egui has no real text shaping and poor typographic hierarchy, so a paragraph is not a design
element — it is a wall. A widget explaining itself in paragraphs is a design problem, not a copy
problem.

The failure is not that the text is unreadable; it is that a list of twenty becomes twenty
paragraphs to read rather than a shape to scan.

A caption that needs a paragraph to explain the widget is a caption describing a widget that needs
redesigning.