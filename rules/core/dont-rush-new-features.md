---
id: dont-rush-new-features
title: Build what was asked, then stop
layer: core
activation: always
priority: 86
overrides:
targets:
---

## Directive

- Asked for X → build X and stop. Do not add Y and Z because they seem wanted: no config
  option "for flexibility", no trait "for testability", no abstraction "for later".
- Three exceptions only: the task cannot be completed without it · not doing it would break
  something that currently works · I asked you to use your judgement.
- Everything else → mention it and let me decide. A one-line "this would also allow X if you
  want it next" is welcome. Building X uninvited is not.
- Applies to cleanup too: renames, import reordering, tidying a neighbouring function, bumping
  a dependency "while I'm here". If a nearby thing is genuinely broken, say so in the final
  message rather than fixing it silently.

## Rationale

Scope creep is not generous, it is expensive. It enlarges the diff I have to review, hides the
change I actually asked for, and usually encodes a guess about a requirement that was never
stated — and when the guess is wrong, the cost of removing it is higher than the cost of never
adding it.

The cleanup clause exists because "while I'm here" changes are the ones that make a
behavioural diff unreviewable. A formatting sweep bundled with a fix means the fix cannot be
seen.
