---
id: rust-test-layout
title: Where tests live, and what they may touch
layer: language:rust
activation: language:rust
priority: 62
overrides:
targets:
---

- **Unit tests beside the code**, in a `mod tests` block in the same file. They can reach
  private items, which is usually the point.
- **Integration tests in `tests/`**, one file per scenario. They exercise the public API only.
- **Runnable examples in `examples/`** when they clarify the API — `cargo run -p <crate>
  --example <name>` is the test that a consumer can actually use the thing.
- **WASM tests use `wasm-bindgen-test`** and need a WASM-capable runner. They do not run under
  plain `cargo test`, so a green suite is not evidence that a WASM test passed.
- **Tests are deterministic.** No network calls unless they are feature-gated or mocked. A test
  that depends on a live API is a test that fails for reasons unrelated to the change.

## Why the last two matter most

They are the two ways a suite reports success without having checked anything. A `wasm-bindgen`
test that silently does not run, and a network test that passes because the fixture is cached,
both look identical to a passing test — and both mean the same thing when a real bug ships.

## Reusable fixtures

Capture a fixture in the shared `data/` crate when a second crate needs it, rather than copying
it. Shared serialization fixtures live in `test_datum_serialization.rs`.
