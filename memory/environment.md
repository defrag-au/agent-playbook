# Environment

Facts about this machine and where things live. Not constraints — a constraint belongs in
`rules/`, where it can be layered, overridden and enforced. See [`README.md`](README.md) for
the split, and note that anything here must be true of **every** project, because this file
loads for all of them.

## Where repositories live

| Path | What |
| --- | --- |
| `~/code/defrag/shared-crates` | Common Rust crates; home of the egui and macroquad widget libraries |
| `~/code/defrag/cnft.dev-workers` | Cloudflare Workers ecosystem; D1, WASM |
| `~/code/defrag/mitos` | — |
| `~/code/hodlcroft/archivist` | Native binaries, libp2p + axum. Not a worker project |
| `~/code/defrag/ecosystem-docs` | Canonical cross-service contracts |
| `~/code/defrag/agent-playbook` | The rule set these instructions are generated from |

Repositories are laid out as `~/code/<org>/<repo>`.

## Tooling on this machine

`nix` and `direnv` are installed, and `direnv` is at `~/.nix-profile/bin/direnv`. Repositories
that pin a toolchain do it through a `flake.nix` devshell — what that means in practice is the
`rust-devshell-first` rule, not a fact about the machine.

## Where reference material lives

Crate cheat sheets are in `references/crates/` in the playbook. `~/.claude/crate-refs/` was the
previous location and is now the wrong place to look.