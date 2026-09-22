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

## The agent toolkit

`agent-playbook`'s flake builds a read-only toolkit — `at-peek` (the working tree), `at-recall`
(history and state) and `at-describe` (the catalogue) — and `defrag-nix` wires it into every defrag
devshell. A further tool is a line in that flake's `cargoBuildFlags`.

It is on `PATH` in an **interactive** shell in those repos, because direnv's hook runs on `cd`.
A shell spawned by an agent does not inherit that environment, so it reaches the toolkit as
`direnv exec . at-peek …` — which reads the realised `.direnv/` and needs no nix daemon. A `nix
profile install` of `agent-tools` puts it on `PATH` unconditionally instead.

`at-peek` has no write path, spawns no subprocess and makes no network calls. `at-recall` has no
write path either, and runs exactly one program — `git`, with read verbs only. That difference is
why they are separate binaries, and why they are separate approvals when the grants are tiered.
Which verb to reach for is `rules/org/defrag/agent-tools`; the traps are the `inspect-code` skill.

## Where reference material lives

Crate cheat sheets are in `references/crates/` in the playbook. `~/.claude/crate-refs/` was the
previous location and is now the wrong place to look.