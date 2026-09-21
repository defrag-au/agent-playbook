---
id: conservative-package-changes
title: Ask before changing dependencies or architecture
layer: core
activation: always
priority: 84
overrides:
targets:
---

Before making a change that alters the *approach* rather than the *implementation*, stop and
ask. Present the problem, offer two or three specific options with their trade-offs, and
wait for an answer.

## Triggers — ask first

- Editing `Cargo.toml`, `package.json`, `flake.nix`, or any other manifest
- Adding, removing or bumping a dependency
- Swapping one library for another (`smlang` → `rust-fsm`, `reqwest` → `ureq`)
- Replacing a whole module or implementation rather than fixing it
- Choosing a pattern — state machine style, database access layer, error strategy, where a
  type lives
- Anything that changes how the project is built, deployed or configured

## Does not trigger

- A bug fix that stays inside the existing approach
- A direct instruction ("change X to Y") — that is already a decision
- Formatting, lint fixes, renames local to one function

## Why

These are the decisions that are cheap to make and expensive to unmake. A dependency is a
supply-chain commitment, a build-time cost, and a future upgrade obligation. A framework
choice reshapes every file that follows it. I usually have context you do not — a reason the
current choice was made, a constraint from elsewhere in the ecosystem — and I would rather
spend one message than one refactor.

The options you offer should be *specific*: name the crates, say what each costs. "We could
use a library or write it ourselves" is not a set of options.
