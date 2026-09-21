# Environment

Facts about the machines and tooling I work on. Not constraints — see
[`rules/rust/devshell-first`](../rules/rust/devshell-first.md) for the rule that follows from
the first entry here.

## Rust tooling is behind a Nix devshell

Every Rust repo in this workspace defines its toolchain in `flake.nix`, and the pinned channel
comes from `defrag-nix`. `cargo`, `rustc`, `clippy`, `rustfmt`, `trunk`, `wrangler` and `node`
are not on the default `PATH`.

- Enter with `nix develop -c <cmd>`, or `direnv exec . <cmd>` where `.direnv/` is realised.
- `direnv exec` is the one that works **in a sandbox**, because `nix develop` needs the daemon
  socket at `/nix/var/nix/daemon-socket/socket`, which a sandbox refuses.
- A bare `cargo` failure reads like a permissions problem rather than a missing toolchain.
- `rustup` is not available, and `rustup target add` is never the answer — targets come from
  the devshell.

## Repositories

| Path | What |
| --- | --- |
| `~/code/defrag/shared-crates` | Common Rust crates; home of the egui and macroquad widget libraries |
| `~/code/defrag/cnft.dev-workers` | Cloudflare Workers ecosystem; D1, WASM |
| `~/code/defrag/mitos` | — |
| `~/code/hodlcroft/archivist` | Native binaries, libp2p + axum. Not a worker project |
| `~/code/defrag/ecosystem-docs` | Canonical cross-service contracts |
| `~/code/defrag/agent-playbook` | This repository |

Crate cheat sheets live in `references/crates/` here, not in `~/.claude/crate-refs/` — that
directory was the previous location and is now the wrong place to look.

## Screenshots

Chromium headless **floors the window near 620px**, lays out at the floor, and then crops the
capture to the width requested. A narrow screenshot is therefore a wide layout with its right
edge sliced off, which reads as an overflow bug that is not there. Use CDP
`Emulation.setDeviceMetricsOverride` instead — see the `widget-screenshot` skill.

Brave is installed at `/Applications/Brave Browser.app/Contents/MacOS/Brave Browser` and is
Chromium, so nothing extra needs installing for headless captures.

## Zed's agent shell

Sandboxed by default:

- No shell substitution in commands that need approval — `$VAR`, `$(…)`, backticks, `<(…)` are
  rejected. Resolve values first, or pass files as literal arguments.
- `.git` is not writable while sandboxed.
- Network is per-host through an HTTP/HTTPS proxy, so `https://` works where `git@`/`ssh://`
  does not.
- Writable by default: the project roots and a per-thread temp directory.
