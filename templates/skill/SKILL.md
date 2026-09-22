---
name: <skill-name>
description: <One specific, actionable line. This is the entire signal the model has for deciding whether to load this skill. "Use when X, to do Y" — not "helps with code". 1-1024 characters.>
---

<!--
Copy to skills/<skill-name>/SKILL.md. The `name` must match the directory name exactly:
lowercase alphanumerics with single hyphens.

Do NOT add `disable-model-invocation: true` unless the skill should only ever be invoked by
hand. Every one of this playbook's eight predecessor skills had it set, together with a
missing description, which meant none of them could ever fire.

Checklist:
  - Does it have a trigger? No trigger means it is a rule — put it in rules/.
  - Steps, not principles. A skill that says "be careful" is a rule in the wrong place.
  - Concrete commands and paths, complete enough to run.
  - Supporting files go in this directory and are referenced with relative paths.
-->

# <Title>

<What this does, and the situation that should have brought the reader here.>

## 1. <First step>

```sh
<the command>
```

## 2. <Second step>

<What to look for, and what it means if it is not there.>

## <Traps>

<The things that look right and are wrong. A skill is the right place for a trap: it is
retrieved exactly when the trap is about to be walked into.>
