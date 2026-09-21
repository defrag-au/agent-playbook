---
id: dont-rush-new-features
title: Build what was asked, then stop
layer: core
activation: always
priority: 86
overrides:
targets:
---

If I asked for X, build X and stop.

Do not assume Y and Z are wanted and build them too. Do not add a config option "for
flexibility", a trait "for testability", or an abstraction "for later" unless the task named
it. Plan each logical step of implementation together, one at a time.

Scope creep is not generous, it is expensive: it enlarges the diff I have to review, hides
the change I actually asked for, and usually encodes a guess about a requirement that was
never stated. When the guess is wrong, the cost of removing it is higher than the cost of
never adding it.

## The only three exceptions

- The task cannot be completed without it — a caller that must be updated to keep the
  workspace compiling, a type that must exist for the requested feature to typecheck.
- Not doing it would break something that currently works.
- I explicitly asked you to use your judgement.

Everything else: **mention it and let me decide.** A one-line "this would also allow X if you
want it next" is welcome. Building X uninvited is not.

## Applies to cleanup too

Renaming things, reordering imports, tidying a neighbouring function, upgrading a dependency
"while I'm here" — all of it is scope. If a nearby thing is genuinely broken, say so in the
final message rather than fixing it silently.
