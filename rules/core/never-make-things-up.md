---
id: never-make-things-up
title: Never invent data to satisfy an interface
layer: core
activation: always
priority: 88
overrides:
targets:
---

## Directive

- Never fabricate a value to make a function, fixture or screen look functional. Before
  writing a placeholder, find where the data actually comes from — a config file, a table, an
  API response, a sibling implementation, an env var. A real search turning up nothing → ask,
  do not invent.
- Same rule for: a plausible address, hash or timestamp invented for a fixture (it hides real
  parsing bugs, because invented data is already in the format the code expects) ·
  `unwrap_or(0)`, `Default::default()` or a hard-coded fallback on a value that should have
  been sourced · a struct written from memory of what an API "should" return (cite the actual
  response or the docs) · a test asserting what the code currently does · a confident
  explanation of a failure you have not verified.
- Choosing a sensible default and **stating it** is fine. Inventing a value and presenting it
  as data is not. Test: if someone asks "where does this number come from?", is there an
  answer? "Nowhere yet" → say so and ask.

## Rationale

Invented data is worse than missing data, because it is already in the format the code
expects. A plausible fixture hides real parsing bugs — the interesting inputs are the ones an
external source actually sends, and a made-up one cannot be wrong in the way that matters.

A silent default is fabricated data wearing a type. `unwrap_or(0)` on a value that should
have been read from somewhere looks like a decision, and it is read as one by whoever finds
it later.

"A plausible API shape" is the version of this that survives longest: a struct written from
memory, then a parser derived from it, then a bug that only appears against the real service.

In prose the same failure is a confident mechanism you have not checked — "it's probably a
caching issue" is a made-up value in sentence form.
