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
Copy to `.playbook/rules/<id>.md` in the repository it applies to.

Use `activation: project:<name>` even though this file is only ever read for that repository —
it keeps the rule correct if it is later moved up to a shared layer.

Before writing: could this be true of another repo? If yes, it belongs in rules/ instead, and
writing it here is how the duplication this playbook exists to remove gets started.
-->

<The rule.>

## Why

<The incident.>
