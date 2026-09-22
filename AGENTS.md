# agent-playbook — working on this repository

This repo is a **data tree plus one small tool**. The data is markdown; the tool resolves it.
Read [`README.md`](README.md) for the design, [`docs/precedence.md`](docs/precedence.md) for
the resolution model, and [`docs/rule-format.md`](docs/rule-format.md) for the frontmatter
contract. This file is only about working *on* the repository.

## The one thing to understand first

The tool is not a library of prompts. It resolves **which rules apply to a given project and
target**, and renders them into a managed block that a repository commits. Everything in
`src/` exists to make that resolution correct, deterministic and loud about failures.

So the bias throughout is: **a rule that does not apply must not appear, and a rule that fails
to parse must not disappear quietly.** The predecessor implementation got both wrong, which is
why it was replaced. See the README's "Why the tool is Rust and not a shell script".

## Building and testing

`flake.nix` provides the toolchain — `cargo`, `rustc`, `clippy` and `rustfmt`, from the same fenix
channel the rest of the org builds with. None of them is on the default `PATH`, so the shell has to
come from one of these:

```sh
nix develop -c cargo test                 # this repository's own devshell

# the same devshell, for a shell that is not interactive — an editor's, an agent's, a script's:
direnv exec . cargo test
direnv exec . cargo clippy --workspace --all-targets --all-features -- -D warnings
```

`direnv allow .` once, from an interactive shell, is what makes `.envrc` (`use flake`) load on `cd` —
which is why an interactive shell in this directory has `cargo` without being asked. A shell an
ditor or an agent spawned sources no rc file and so runs no direnv hook: there, `command not found:
cargo` means an unloaded shell, not a missing toolchain, and `direnv exec .` is the answer. Neither
of them needs `defrag-nix`; see *The flake* below for why that is a constraint and not a limitation.

Add `--offline` if cargo tries to reach the network. It should never need to: the composer
crate has **no dependencies**, deliberately, so it builds with no network and no registry
cache. That is a property to preserve, not an accident — the tool is the thing you reach for
when a repo's toolchain is what is broken. The `tools/` crates hold the same line.

Before pushing:

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Clippy is clean at `-D warnings` and should stay that way.

### The flake

`flake.nix` pins the same fenix channel `defrag-nix` does, and exports `playbook`, `agent-tools`
(`at-peek`, `at-recall`, `at-describe`), a devshell, and a `checks` entry that runs the suite.

**It cannot consume `defrag-nix`, and does not need to.** That input already points the other way:
`defrag-nix` takes *this* repository as an input, for `packages.agent-tools`, and wires the binaries
into the org's shells. An input back would be a cycle, and flake inputs are a DAG — Nix refuses to
resolve one rather than doing something surprising with it. What is shared instead is the pin: both
flakes build from the same fenix channel, so a bump is one `nix flake update` in each repository and
they agree again afterwards. The lock is per repository; the toolchain is not inherited, it is
matched.

That is also the property worth keeping: a repository meant to be read by someone outside the org
builds with nothing but its own flake. `defrag-nix`'s input uses `nixpkgs.follows` and
`fenix.follows`, so neither is evaluated twice.

The repo is still **not self-hosted**: there is no `projects/agent-playbook/` and no managed
block in this file.

## Adding a rule

Use [`skills/capture-rule/SKILL.md`](skills/capture-rule/SKILL.md). The short version:

1. Pick the layer with the layer test — `core` / `language:rust` / `org:defrag` / `project`.
2. Copy `templates/rule.md`, fill the frontmatter, delete every placeholder.
3. `cargo run -- rules` — confirm it activates somewhere. **An orphan rule is not a rule.**
4. `cargo test` — the suite parses the whole tree.
5. `cargo run -- list --project <name>` for each affected project, then install.

## Conventions that the tests enforce

These are not style preferences; `cargo test` fails if they are violated, and the failure
message names the file.

- **Frontmatter is flat `key: value`.** No nesting, no block sequences, no multi-line strings.
  The parser is `awk`-shaped on purpose. If a rule needs structure, it is a reference file.
- **`layer` is namespaced**: `core`, `language:rust`, `org`, `project`. A bare `rust` does not
  parse, because a bare name is ambiguous between a language and a typo.
- **`id`, `title`, `layer`, `activation` are required and non-empty.** A missing one is an
  error, not a default.
- **No duplicate ids**, anywhere in the tree.
- **Every rule has a body.** A heading with no text cannot constrain anything.
- **Every rule activates for some project.** Orphans fail `no_rule_is_unreachable`.
- **Every rule parses.** `every_rule_in_the_tree_parses_and_has_a_body`.
- **Rendering is byte-stable.** `check` diffs the output, so nondeterminism becomes a false
  "stale" report. `rendering_is_byte_stable` pins it.
- **Nothing is overridden today.** `overrides:` is enforced to point strictly downward; see
  `docs/rule-format.md`. Do not add an override to demonstrate the mechanism.

## When you find a rule at the wrong layer

Move it. The layer test is in `rules/README.md`. Moving a rule up the moment a second consumer
needs it — and down the moment only one does — is the maintenance this repository exists to
make possible. `docs/inventory.md` records the four layer corrections made during the initial
consolidation; add to that section when you make another.

## When a rule stops being true

Delete it, or replace it. Do not append a correction below it and do not comment it out.

The one exception is a fact whose replacement is unintuitive, where the stale version is worth
naming so it is not rediscovered —
`projects/archivist/rules/devshell-commands.md` is the worked example.
