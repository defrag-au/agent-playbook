---
id: rust-inline-format-args
title: Inline format arguments everywhere
layer: language:rust
activation: language:rust
priority: 66
overrides:
targets:
---

## Directive

Inline format arguments everywhere — clippy warns on the alternative and the lint command
treats warnings as errors.

```rust
// no
format!("Hello {}", name)
println!("wrote {} rows to {}", count, path)

// yes
format!("Hello {name}")
println!("wrote {count} rows to {path}")
```

- Every formatting macro: `format!`, `println!`, `eprintln!`, `write!`, `writeln!`, `panic!`,
  `assert!`, `assert_eq!`, `debug_assert!`, `tracing` macros.
- New code → inline args from the first draft.
- Editing existing code → convert the lines you are already touching · do not sweep the file.
  Broadly non-compliant file → `clippy --fix` as its own change — see
  [`rust-tooling-handles-grunt-work`](tooling-handles-grunt-work.md).

## Rationale

The lint is baked into the lint command as an error, so a `{}` is a build failure rather than a
style note — which is why this is stated as a rule rather than left to review.

Writing `{}` and expecting a later pass to catch it costs a round trip through the compiler to
fix something that was free to get right the first time. Sweeping a file to fix it costs the
reviewer the diff they asked for, which is why the conversion is scoped to the lines already
being touched.
