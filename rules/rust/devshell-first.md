---
id: rust-devshell-first
title: Rust tooling lives behind the Nix devshell
layer: language:rust
activation: language:rust
priority: 70
overrides:
targets:
---

`cargo`, `rustc`, `clippy`, `rustfmt`, `trunk`, `wrangler`, `node` and the wasm targets are
**not on the default `PATH`**. They are provided only inside the devshell defined in
`flake.nix`. Wrap every command:

```sh
nix develop -c cargo build --workspace
nix develop -c cargo test -p <crate>
nix develop -c cargo clippy --workspace --all-targets --all-features -- -D warnings
nix develop -c cargo fmt
```

`nix develop --command <cmd>` is equivalent.

## In a sandboxed agent shell, use direnv instead

`nix develop` needs the nix daemon socket (`/nix/var/nix/daemon-socket/socket`), which a
sandboxed agent shell refuses:

```
error: cannot connect to socket … Operation not permitted
```

`direnv exec . <cmd>` reads the already-realised devshell out of `.direnv/` and needs no
daemon, so **in a sandbox it is the invocation that works**:

```sh
direnv exec . cargo build -p <crate>
```

If `.direnv/` is cold, run `direnv allow .` once from an unsandboxed shell.

## The failure reads as a permissions problem

A bare `cargo …` fails with something that looks like a filesystem or permissions error
rather than a missing toolchain. Recognise it before debugging it — the answer is never to
install anything.

## Do not

- **Install a separate Rust toolchain.** The devshell pins the channel so every repo stays in
  lock-step. `rustup` is not available, so `rustup target add …` is never the answer; targets
  come from the devshell.
- **Source `~/.bash_profile` to find cargo.** That was the right move before the devshell
  existed; it is now a way to pick up a stale, unpinned toolchain.
- **Reach for the network to fix a build.** Add `--offline` when a build tries to fetch a
  crate the devshell cache already holds. Note that a sandboxed shell also cannot write the
  shared registry cache (`~/.cargo/registry`), so a genuine *first* fetch of a new crate can
  fail with `Operation not permitted` for that reason rather than a missing toolchain.
