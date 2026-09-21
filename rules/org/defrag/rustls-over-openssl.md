---
id: defrag-rustls-over-openssl
title: Prefer rustls over OpenSSL
layer: org
activation: org:defrag
priority: 38
overrides:
targets:
---

Use `rustls` rather than OpenSSL for TLS in any new dependency or HTTP client, and when
choosing between two crates that differ only in which they pull in.

The HTTP clients in this ecosystem already do. When adding a crate, check its default features
for `native-tls` or `openssl-sys` and prefer a `rustls` feature where one exists:

```toml
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json"] }
```

## Why

Two reasons, and the second is the one that bites:

- **Cross-compilation.** OpenSSL needs a system library and a matching toolchain; rustls is
  pure Rust. A `wasm32-unknown-unknown` build cannot link OpenSSL at all — see
  [`defrag-runtime-pairs`](runtime-pairs.md) for the same constraint in its other form.
- **A dependency that arrives transitively.** You rarely choose OpenSSL directly; it appears
  because a crate's *default* features pulled it in. That is why the check is on the transitive
  tree, not on the crate you typed:

```sh
nix develop -c cargo tree --target wasm32-unknown-unknown -p <crate> | grep -e openssl -e native-tls
```
