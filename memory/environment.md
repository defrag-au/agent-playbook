# Environment

Facts about this machine and where things live. Not constraints — a constraint belongs in
`rules/`, where it can be layered, overridden and enforced. See [`README.md`](README.md) for
the split, and note that anything here must be true of **every** project, because this file
loads for all of them.

## Where repositories live

Repositories are laid out as `~/code/<org>/<repo>`. Which repository these instructions are being
resolved for is declared by that repository's own `.playbook/project.conf` — this file deliberately
carries no inventory of them, because a list of repositories in the personal file is wrong the
moment one is added, renamed or moved.

## Tooling on this machine

`nix` and `direnv` are installed, and `direnv` is at `~/.nix-profile/bin/direnv`. Repositories
that pin a toolchain do it through a `flake.nix` devshell — what that means in practice is the
`rust-devshell-first` rule, not a fact about the machine.

## The Workers edge stack

Measured 2026-10-10 on the `worker` crate with `worker-build` 0.7.4 over js-sys 0.3.106:
**~90+ completed subrequests in one invocation is unreliable** (breakage observed at 90-137;
everything measured below ~45 has been fine). At that count — on the R2 binding and on CDN
`fetch()` alike, so it is the wasm executor rather than a transport — js-sys panics (`RefCell
already borrowed` in `Task::run`), and the worker-build shim cannot reinitialise the Wasm app,
so the isolate answers `error code: 1101` to every later request until a fresh `wrangler
deploy`; a fully sequential variant of the same workload stalled silently instead. Found on the
asset metadata index's read path, walking a 137-candidate policy; the write-up is
`cnft.dev-workers/docs/design/ASSET_METADATA_INDEX.md`, and its probe caps a batch at 64. So a
request's subrequest fan-out has a practical ceiling near 100 on this stack, and a sudden 1101
after a heavy request is a poisoned isolate, not a code error.

## The agent toolkit

`agent-playbook`'s flake builds two things, and `defrag-nix` wires both into every defrag devshell:

- **The inspectors** — `at-peek` (the working tree), `at-crate` (installed crate source and
  build-script output, resolved through `Cargo.lock`), `at-recall` (history and state),
  `at-reach` (the fleet inventory), `at-cost` (the process table by repository), `at-cardano`
  (read-only Cardano chain enquiry through Koios) and `at-describe` (the catalogue). A further
  tool is a line in that flake's `cargoBuildFlags`.
- **The composer** — `playbook`, which resolves the rules for a project and target and can
  `check`/`install` a repository's managed block. It carries its own rule tree, so it needs no
  checkout: `playbook root` prints which tree it is resolving against and how it found it. A
  change to a rule reaches a shell only after it is committed and `defrag-nix` re-locks it, so
  `--root ~/code/defrag/agent-playbook` resolves against the working tree instead.

They are on `PATH` in an **interactive** shell in those repos, because direnv's hook runs on `cd`,
and `nix-direnv` prepends the devshell, so that copy wins where one is active.

`nix-config` also installs the inspectors and `browser-shot` as home packages
(`home/modules/agent-tools.nix`), so both are on `PATH` in **every** shell — including one an agent
spawns outside any repository. `playbook` is deliberately not among them: it renders the rules
embedded in its own binary, so a stale copy writes a stale managed block without saying so, and it
reaches a repository through the devshell `defrag-nix` pins instead (`direnv exec . playbook check …`,
which reads the realised `.direnv/` and needs no nix daemon).

Which copy answers is worth a `which <tool>` when it matters: the devshell's inside a repository,
the home package's everywhere else. Neither is worth a `nix develop` wrapper — see
`rules/org/defrag/agent-tools`.

`at-peek` and `at-crate` have no write path, spawn no subprocess and make no network calls.
`at-crate` reads the cargo home named by `--cargo-home`, `CARGO_HOME` or `~/.cargo`, the project's
`Cargo.lock`, and its `target/` directory; it never invokes `cargo`, so an uninstalled crate is an
answer rather than a fetch. `at-recall` has no write path either, and runs exactly one program —
`git`, with read verbs only. `at-cardano` is the one member with a socket: HTTPS to Koios only, GET
and query-POST, `/submittx` refused, and a key read from `KOIOS_API_KEY` or
`~/.config/koios/api_key` that is never printed. `playbook` writes one file, the managed block,
and only under `install`. That difference is why they are separate binaries, and why they are
separate approvals when the grants are tiered. Which verb to reach for is
`rules/org/defrag/agent-tools`; the traps are the `inspect-code` skill.

## Where reference material lives

Crate cheat sheets are in `references/crates/` in the playbook. `~/.claude/crate-refs/` was the
previous location and is now the wrong place to look.