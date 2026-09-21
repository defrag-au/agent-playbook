---
id: rust-test-layout
title: Where tests live, and what they may touch
layer: language:rust
activation: language:rust
priority: 62
overrides:
targets:
---

## Directive

- Unit tests → a `mod tests` block in the same file. They can reach private items, which is
  usually the point.
- Integration tests → `tests/`, one file per scenario, exercising the public API only.
- Runnable examples → `examples/` when they clarify the API. `cargo run -p <crate> --example
  <name>` is the test that a consumer can actually use the thing.
- WASM tests → `wasm-bindgen-test` and a WASM-capable runner. They do not run under plain
  `cargo test`, so a green suite is not evidence that a WASM test passed.
- Tests are deterministic. No network calls unless they are feature-gated or mocked.
- A fixture a second crate needs → capture it in the shared `data/` crate rather than copying
  it. Shared serialization fixtures live in `test_datum_serialization.rs`.

## Rationale

The WASM and determinism rules are the two ways a suite reports success without having checked
anything. A `wasm-bindgen` test that silently does not run, and a network test that passes
because the fixture is cached, both look identical to a passing test — and both mean the same
thing when a real bug ships. A test that depends on a live API is a test that fails for reasons
unrelated to the change.
