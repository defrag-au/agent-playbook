---
id: rust-tooling-handles-grunt-work
title: Let fmt and clippy do the mechanical work
layer: language:rust
activation: language:rust
priority: 64
overrides:
targets:
---

## Directive

Formatting and mechanical lint fixes are tool work. Do not hand-align code or type out a fix
the tool will apply itself.

```sh
nix develop -c cargo fmt
nix develop -c cargo clippy --fix --allow-dirty --all-targets --all-features -- -D warnings
```

- Run them, then read what changed and check the remaining diagnostics manually. `--fix` first,
  then review — the pass changes the lines you were about to read.
- A warning is a failure, not a note. This codebase does not accumulate "known warnings".
- `--fix` applies only mechanically safe suggestions — it will not choose between two valid
  shapes · know a `disallowed_methods` match is a false positive on a blessed call · fix
  anything needing a semantic decision.
- A rule clippy cannot enforce → add a source scan in `cargo test`, in the same spirit as
  `cargo fmt --check`. Add the test with the rule.

## Rationale

`-D warnings` is deliberate rather than incidental. A suite that accumulates "known warnings"
cannot show you the one that matters, so a warning is treated as a failure from the first one
rather than after the hundredth.

The review pass is not optional because `--fix` only applies suggestions that are mechanically
safe: it will not make a choice that needs a semantic decision, and it cannot tell a genuine
lint from a false positive on a blessed call.

Some rules cannot be enforced by clippy at all. Those get a source scan in `cargo test`, in the
same spirit as `cargo fmt --check` — the scan is what makes the rule a failure rather than a
note.
