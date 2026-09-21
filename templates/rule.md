---
id: <layer>-<kebab-case-id>
title: <One line. Becomes the `##` heading in the generated file.>
layer: <core | language:rust | org | project>
activation: <always | language:rust | org:defrag | project:<name> | manual>
priority: <50>
overrides: <comma-separated rule ids this supersedes, or leave empty>
targets: <comma-separated target names, or leave empty for all>
---

<!--
Copy to rules/<layer>/<id>.md, or projects/<repo>/rules/<id>.md.

Delete these comments and every placeholder. A rule with a `<...>` left in it is a rule that
resolves and reads as if it means something.

Checklist:
  - The first paragraph is actionable on its own — `emphasis` repeats it verbatim.
  - It does not restate a lower layer. If rules/core/ says it, delete it from here.
  - It names the tool, not the vibe: a concrete command survives paraphrasing.
  - The incident behind it is recorded in docs/inventory.md.
  - `cargo run -- rules` shows it activating somewhere. An orphan rule is not a rule.
-->

<The rule, in one or two sentences. What to do or not do.>

<Then the specifics: exact commands, exact paths, the shape to copy.>

## Why

<The failure this prevents. Include the concrete incident — a rule with no incident behind it
gets deleted by the next person who finds it inconvenient.>

## Applies to

<Optional. Narrow the scope, or name the exceptions. If this section is longer than the rule
itself, the rule is probably at the wrong layer.>
