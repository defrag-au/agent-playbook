---
id: conservative-package-changes
title: Ask before changing dependencies or architecture
layer: core
activation: always
priority: 84
overrides:
targets:
---

## Directive

Stop and ask before a change that alters the *approach* rather than the *implementation*.
Present the problem, two or three specific options with their trade-offs, and wait.

Ask first for:

- editing `Cargo.toml`, `package.json`, `flake.nix` or any other manifest
- adding, removing or bumping a dependency
- swapping one library for another (`smlang` → `rust-fsm`, `reqwest` → `ureq`)
- replacing a whole module or implementation rather than fixing it
- choosing a pattern — state machine style, database access layer, error strategy, where a
  type lives
- anything that changes how the project is built, deployed or configured

No need to ask for: a bug fix inside the existing approach · a direct instruction ("change X
to Y") · formatting, lint fixes, or renames local to one function.

Options must be **specific** — name the crates, say what each costs. "We could use a library
or write it ourselves" is not a set of options.

## Rationale

These are the decisions that are cheap to make and expensive to unmake. A dependency is a
supply-chain commitment, a build-time cost, and a future upgrade obligation. A framework
choice reshapes every file that follows it.

I usually have context you do not — a reason the current choice was made, a constraint from
elsewhere in the ecosystem — and I would rather spend one message than one refactor.
