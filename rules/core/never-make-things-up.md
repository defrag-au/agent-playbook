---
id: never-make-things-up
title: Never invent data to satisfy an interface
layer: core
activation: always
priority: 88
overrides:
targets:
---

Never fabricate values to make a function, a fixture or a screen look functional.

If an interface needs data, **find out where that data actually comes from before writing a
placeholder.** The source almost always exists — a config file, a table, an API response, a
sibling implementation, an environment variable. Look for it. If a genuine search turns up
nothing, ask where it should come from rather than inventing it.

This applies to more than literals:

- **Sample data in a fixture** — a realistic-looking address, hash or timestamp that was
  invented rather than captured. It hides real parsing bugs, because invented data is
  already in the format the code expects. The interesting inputs are the ones an external
  source actually sends.
- **A default that looks like a decision** — `unwrap_or(0)`, `Default::default()`, a
  hard-coded fallback. A silent default on a value that should have been sourced is
  fabricated data wearing a type.
- **A plausible API shape** — writing a struct from memory of what an API "should" return,
  then deriving the parser from it. Cite the actual response or the actual docs.
- **A test that asserts what the code currently does** — that is inventing a specification
  to match an implementation.
- **An explanation of why something is broken** — a confident mechanism you have not
  verified. "It's probably a caching issue" is a made-up value in prose form.

## The distinction that matters

Making things up is not the same as *choosing*. Choosing a sensible default and stating it
is fine. Inventing a value and presenting it as data is not. The test: if someone later asks
"where does this number come from?", is there an answer?

If the answer is "nowhere yet", say so plainly and ask.
