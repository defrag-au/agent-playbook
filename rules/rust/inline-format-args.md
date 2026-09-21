---
id: rust-inline-format-args
title: Inline format arguments everywhere
layer: language:rust
activation: language:rust
priority: 66
overrides:
targets:
---

Always use inline format arguments. Clippy warns on the alternative, and the lint is baked
into the lint command as an error.

```rust
// no
format!("Hello {}", name)
println!("wrote {} rows to {}", count, path)

// yes
format!("Hello {name}")
println!("wrote {count} rows to {path}")
```

Applies to every formatting macro: `format!`, `println!`, `eprintln!`, `write!`,
`writeln!`, `panic!`, `assert!`, `assert_eq!`, `debug_assert!`, `tracing` macros.

## When writing new code

Use inline args from the first draft. Do not write `{}` and expect a later pass to catch it —
that pass is the one that costs a round trip.

## When editing existing code

Convert to inline args in the lines you are already touching. Do not sweep the file: an
unrelated formatting change buries the diff I asked for. If a file is broadly non-compliant,
let `clippy --fix` handle it as its own change — see
[`rust-tooling-handles-grunt-work`](tooling-handles-grunt-work.md).
