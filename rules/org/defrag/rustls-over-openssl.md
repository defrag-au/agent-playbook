---
id: defrag-rustls-over-openssl
title: Prefer rustls over OpenSSL
layer: org
activation: org:defrag
priority: 38
overrides:
targets:
---

## Directive

Use `rustls` rather than OpenSSL for TLS in any new dependency or HTTP client, and when choosing
between two crates that differ only in which they pull in.

- Check a crate's default features for `native-tls` or `openssl-sys`; prefer a `rustls` feature
  where one exists:

```toml
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json"] }
```

- Check the transitive tree, not the crate you typed — OpenSSL usually arrives because a crate's
  *default* features pulled it in:

```sh
nix develop -c cargo tree --target wasm32-unknown-unknown -p <crate> | grep -e openssl -e native-tls
```

## Rationale

Two reasons, and the second is the one that bites. The HTTP clients in this ecosystem already use
rustls:

- **Cross-compilation.** OpenSSL needs a system library and a matching toolchain; rustls is pure
  Rust. A `wasm32-unknown-unknown` build cannot link OpenSSL at all — see
  [`defrag-runtime-pairs`](runtime-pairs.md) for the same constraint in its other form.
- **A dependency that arrives transitively.** You rarely choose OpenSSL directly; it appears
  because a crate's *default* features pulled it in, which is why the check is on the transitive
  tree rather than on the crate you typed.