---
id: defrag-runtime-pairs
title: Keep logic pure — runtime bridges come in pairs
layer: org
activation: org:defrag
priority: 56
overrides:
targets:
---

## Directive

**wasm-bindgen code can never run under macroquad/miniquad** — the miniquad `gl.js` runtime has no
wasm-bindgen glue; browser interop goes through the miniquad plugin protocol (`sapp_jsutils`)
instead. Permanent, not a version issue.

- Runtime-specific bridges come in pairs: `wallet-core` for wasm-bindgen frontends,
  `wallet-miniquad` for macroquad games. egui-widgets and macroquad-widgets do not interchange.
- Keep logic in small pure crates — no wasm-bindgen, no I/O, no runtime deps — so the same crate
  can be consumed from a macroquad game, a wasm-bindgen frontend (Leptos) **and** a Cloudflare
  worker. Push runtime bindings to thin edge crates.
- Macroquad target → check a dependency's transitive tree for wasm-bindgen before adding it. A
  native `cargo tree` is blind to this; the target flag is required:

```sh
nix develop -c cargo tree --target wasm32-unknown-unknown -p <crate>
```

- WASM build → minimise the features enabled on a wasm32 target. Default features are chosen for a
  native build, so a wasm consumer inherits capabilities it cannot use; a feature that pulls in a
  system library fails at link time, not at the crate that introduced it. Check the transitive
  tree, not the direct dependency.

## Rationale

The two runtimes cannot be bridged by a version bump: the miniquad `gl.js` runtime has no
wasm-bindgen glue at all, so a crate that reaches wasm-bindgen is unlinkable from a macroquad host.
That is why the bridges are duplicated rather than shared.

`cardano-tx/tests/miniquad_linkable.rs` asserts the default-feature-free build reaches no
wasm-bindgen, so a macroquad host can still link it — the test is the enforcement of the pure-crate
rule.

Macroquad targets demand more discipline here than other runtimes, which is why the transitive tree
check is called out for them specifically.