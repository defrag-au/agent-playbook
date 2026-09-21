---
id: defrag-runtime-pairs
title: Keep logic pure — runtime bridges come in pairs
layer: org
activation: org:defrag
priority: 56
overrides:
targets:
---

**wasm-bindgen code can never run under macroquad/miniquad.** The miniquad `gl.js` runtime has
no wasm-bindgen glue; browser interop goes through the miniquad plugin protocol
(`sapp_jsutils`) instead. This is permanent, not a version issue.

Runtime-specific bridges therefore come in pairs: `wallet-core` for wasm-bindgen frontends,
`wallet-miniquad` for macroquad games. egui-widgets and macroquad-widgets do not interchange
for the same reason.

## The consequence for structure

**Keep logic in small pure crates** — no wasm-bindgen, no I/O, no runtime deps — so the same
crate can be consumed from a macroquad game, a wasm-bindgen frontend (Leptos) **and** a
Cloudflare worker. Push runtime bindings to thin edge crates.

Macroquad targets demand more discipline here than other runtimes: check a dependency's
transitive tree for wasm-bindgen before adding it to a macroquad game.

```sh
# a native cargo tree is blind to this — the target flag is required
nix develop -c cargo tree --target wasm32-unknown-unknown -p <crate>
```

`cardano-tx/tests/miniquad_linkable.rs` asserts the default-feature-free build reaches no
wasm-bindgen, so a macroquad host can still link it.
