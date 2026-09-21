---
id: archivist-devshell-commands
title: This repo's devshell commands
layer: project
activation: project:archivist
priority: 55
overrides:
targets:
---

```sh
nix develop -c cargo test
nix develop -c cargo clippy --all-targets --all-features -- -D warnings
nix develop -c cargo build
nix develop -c cargo fmt
```

Native binaries (libp2p + axum). Not a Cloudflare Worker project — the wasm target and the
worker tooling are not part of this repo's workflow.

## The advice this replaced

The project's earlier notes said to run `source ~/.bash_profile && cargo --version` to find the
toolchain. That is now wrong twice over: it picks up an unpinned toolchain, and it makes the
failure mode depend on whether the shell was warm. It is recorded here rather than deleted
because it is the kind of advice that gets rediscovered — the nix daemon refusal in a sandbox
*looks* like a missing environment, and the profile is the first place to look.

The correct answer to that symptom is `direnv exec . <cmd>`, not a profile.
