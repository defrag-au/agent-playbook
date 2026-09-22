---
id: cnft-wasm-safe-serde
title: Use wasm-safe-serde for u64/i64 deserialization
layer: project
activation: project:cnft-dev-workers
priority: 48
overrides:
targets:
---

## Directive

**Always use `wasm-safe-serde` for `u64`/`i64` fields** — it handles both the string and the
integer representation, which external APIs send inconsistently.

```rust
#[serde(with = "wasm_safe_serde::u64_required")]
pub amount: u64,

#[serde(with = "wasm_safe_serde::u64_option")]
pub fee: Option<u64>,
```

Never write a custom deserializer for number/string handling — a hand-rolled one handles fewer
cases than the crate does.

## Rationale

JavaScript numbers cannot represent all `u64` values, so JSON APIs written in JS send them as
strings; APIs written in Rust send them as numbers. The same field can arrive either way
depending on which service produced it. `wasm-safe-serde` is the crate that already knows
this, which means the failure mode of not using it is an intermittent deserialization error
on large values only — the kind that passes every test with small fixtures and breaks in
production.

**Why this is a project rule and not an org rule.** It is here because this is the repo that
consumes those external APIs. Move it up to `rules/org/defrag/` if a second repo starts
deserializing third-party JSON.
