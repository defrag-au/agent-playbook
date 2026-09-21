---
id: rust-build-often
title: Build often — the compiler is the best information you have
layer: language:rust
activation: language:rust
priority: 68
overrides:
targets:
---

In a Rust project, building is the fastest way to find out what is wrong.

The compiler knows more about the code than you can work out by reading it. It resolves every
path, checks every type, and reports every mismatch with a location and a suggestion. That is
strictly better information than reasoning about whether a reference is still valid, or
searching the tree for a call site that may not exist.

So: after a change that could plausibly break a type, **compile it**. Do not read your way to
confidence through three files when one `cargo check` answers the question in seconds.

## Practically

- `nix develop -c cargo check` while iterating; `cargo build` when you need artefacts
- Scope it when the workspace is large: `-p <crate>`
- `cargo clippy` for the lints the compiler does not carry — see
  [`rust-tooling-handles-grunt-work`](tooling-handles-grunt-work.md)
- When a build fails, **read the whole error before editing.** Rust errors cascade: the first
  one is usually the real one and the rest are consequences. Fixing a consequence first
  produces a second round of errors that look new.

## The exception

Do not build when you have already established that the build is expensive and the change
cannot affect it — editing a markdown file does not need a compile. Use judgement about what
a change can reach.
