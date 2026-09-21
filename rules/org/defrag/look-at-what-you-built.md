---
id: defrag-look-at-what-you-built
title: Render it and look at it before reporting it done
layer: org
activation: org:defrag
priority: 46
overrides:
targets:
---

## Directive

Every widget in `ui/egui-widgets` has a story in `ui/_storybook-egui`. A new or changed widget must
be rendered and looked at before it is reported as done.

- Applies to narrow widths too, where most of the real failures live — see the
  [`widget-screenshot`](../../../skills/widget-screenshot/SKILL.md) skill for the exact invocation,
  including why `--window-size` cannot produce a mobile shot.
- Not evidence: it compiles · its tests pass · the storybook entry renders without a panic · it
  looks right in the code.
- A *deployed* app is checked the same way — the helper points at a real URL.

## Rationale

Unit tests do not catch layout. They do not catch a default that every test case happens to share.
They do not catch a panel floating over the content it was supposed to sit beside. A widget that
compiles and passes its tests can still be unusable on the screen it was built for.

Real data is wider than fixture data — that is how the wrapped stake address shipped. Pointing the
helper at a real URL is the only way to check a widget against real data at real width.