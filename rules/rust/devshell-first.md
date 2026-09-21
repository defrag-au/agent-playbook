---
id: rust-devshell-first
title: Rust tooling lives behind the Nix devshell
layer: language:rust
activation: language:rust
priority: 70
overrides:
targets:
---

## Directive

`cargo`, `rustc`, `clippy`, `rustfmt`, `trunk`, `wrangler`, `node` and the wasm targets are
**not on `PATH`** — they come only from the devshell defined in `flake.nix`. Wrap every
command:

```sh
nix develop -c cargo build --workspace
nix develop -c cargo test -p <crate>
nix develop -c cargo clippy --workspace --all-targets --all-features -- -D warnings
nix develop -c cargo fmt
```

`nix develop --command <cmd>` is equivalent.

**In a sandboxed shell, `nix develop` cannot reach the daemon socket**
(`/nix/var/nix/daemon-socket/socket`) — `cannot connect to socket … Operation not
permitted`. Use direnv instead, which reads the already-realised devshell out of `.direnv/`
and needs no daemon:

```sh
direnv exec . cargo build -p <crate>
```

If `.direnv/` is cold, run `direnv allow .` once from an unsandboxed shell.

Never: install a separate toolchain, source `~/.bash_profile` to find cargo, or reach for the
network. Add `--offline` if cargo tries to fetch a crate the devshell cache already holds.

## Rationale

**The failure reads as a permissions problem.** A bare `cargo …` fails with something that
looks like a filesystem or filesystem-permission error rather than a missing toolchain, which
is why this is worth stating before it is worth debugging. The answer is never to install
anything.

**Why not `rustup`.** The devshell pins the channel so every repo in the ecosystem stays in
lock-step. `rustup` is not available, so `rustup target add …` is never the answer; targets
come from the devshell.

**Why not a shell profile.** An earlier version of this advice said to run
`source ~/.bash_profile && cargo --version`. That picks up an unpinned toolchain and makes the
failure mode depend on whether the shell was warm. It is recorded in
`projects/archivist/rules/devshell-commands.md` because the symptom it was written for still
occurs and a profile is the first place anyone looks.

**A sandbox has a second failure the devshell cannot fix.** A sandboxed shell also cannot
write the shared registry cache (`~/.cargo/registry`), so a genuine *first* fetch of a new
crate can fail with `Operation not permitted` for that reason rather than a missing toolchain.
`--offline` is the correct response when the crate is already cached; a network request is not.
