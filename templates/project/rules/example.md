---
id: <project>-<kebab-case-id>
title: <One line. Becomes the `##` heading.>
layer: project
activation: project:<name>
priority: <50>
overrides:
targets:
---

<!--
Copy to projects/<name>/rules/<id>.md.

Use `activation: project:<name>` even though this file is only ever read for that project —
it keeps the rule correct if it is later moved up to a shared layer.

Before writing: could this be true of another repo? If yes, it belongs in rules/ instead, and
writing it here is how the duplication this playbook exists to remove gets started.
-->

<The rule.>

## Why

<The incident.>
