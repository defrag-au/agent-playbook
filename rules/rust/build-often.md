---
id: rust-build-often
title: Build often — the compiler is the best information you have
layer: language:rust
activation: language:rust
priority: 68
overrides:
targets:
---

## Directive

- A change that could plausibly break a type → compile it. Do not read your way to confidence
  through three files when one `cargo check` answers the question in seconds.
- `nix develop -c cargo check` while iterating · `cargo build` when you need artefacts · `-p
  <crate>` to scope a large workspace.
- `cargo clippy` for the lints the compiler does not carry — see
  [`rust-tooling-handles-grunt-work`](tooling-handles-grunt-work.md).
- Build fails → read the whole error before editing. Rust errors cascade; fixing a consequence
  first produces a second round of errors that look new.
- Build already established as expensive and the change cannot reach it (editing a markdown
  file does not need a compile) → do not build.

## Rationale

The compiler knows more about the code than you can work out by reading it. It resolves every
path, checks every type, and reports every mismatch with a location and a suggestion — strictly
better information than reasoning about whether a reference is still valid, or searching the
tree for a call site that may not exist.

Rust errors cascade: the first one is usually the real one and the rest are consequences, which
is why a failing build is read in full before anything is edited.

The exception is about reach, not convenience: use judgement about what a change can reach.
