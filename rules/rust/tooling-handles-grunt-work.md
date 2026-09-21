---
id: rust-tooling-handles-grunt-work
title: Let fmt and clippy do the mechanical work
layer: language:rust
activation: language:rust
priority: 64
overrides:
targets:
---

Formatting and mechanical lint fixes are tool work. Do not spend a turn hand-aligning code or
typing out the fix for a lint the tool will apply itself.

```sh
nix develop -c cargo fmt
nix develop -c cargo clippy --fix --allow-dirty --all-targets --all-features -- -D warnings
```

Run them, then **read what changed** and check the remaining diagnostics manually. The
ordering matters: `--fix` first, then review, because the automated pass changes the lines
you were about to read.

## `-D warnings` is not optional

The lint command treats warnings as errors. A warning is a failure, not a note. That is
deliberate — this codebase does not accumulate "known warnings", because a suite with a
hundred warnings cannot show you the one that matters.

## Where the tooling stops

`--fix` only applies suggestions that are mechanically safe. It will not:

- Choose between two valid shapes
- Know that a `disallowed_methods` match is a false positive on a blessed call
- Fix anything requiring a semantic decision

Some rules in this playbook cannot be enforced by clippy at all — a source scan in
`cargo test` is the tool for those, in the same spirit as `cargo fmt --check`. When you add a
rule of that kind, add the test with it.
