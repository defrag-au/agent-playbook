---
name: capture-rule
description: Use when the user corrects you, states a preference, or explains a constraint you did not know — or when you notice a rule that should exist. Encodes the correction into the playbook so it applies to every future session instead of being lost with the conversation.
---

# Capturing a rule

Use this the moment a constraint is stated or a pattern is noticed. A correction that stays in
the conversation is a correction that has to be made again next week.

## 1. Decide what kind of thing it is

| What you heard | Where it goes |
| --- | --- |
| A constraint with no trigger ("always X", "never Y") | `rules/<layer>/` |
| A procedure with a trigger ("when doing X, do it this way") | `skills/<name>/SKILL.md` |
| A fact about the environment ("cargo is not on PATH here") | `memory/environment.md` |
| A crate's API surface | `references/crates/` |
| Something true of one model or harness | `models/<target>/addenda/` |
| Something true of exactly one repo | `projects/<repo>/rules/` |

The most common misfiling is a constraint written as a skill. See `skills/README.md` for why
that is not a small mistake: a rule stored as a skill may never fire at all.

## 2. Choose the layer with the layer test

| Layer | Test |
| --- | --- |
| `core` | Would I want this for a Go project written by someone else? |
| `language:rust` | Is it true of the language, or of the Rust tooling I use? |
| `org:defrag` | True across the defrag ecosystem, but not of my work generally? |
| `project` | True of exactly one repository? |

Move a rule **up** the moment a second consumer needs it, and **down** the moment only one
does. A rule at the wrong layer is how the duplication this repository exists to fix gets
started.

## 3. Write it

Copy `templates/rule.md`. Then:

- **Lead with the rule, not the rationale.** The first line must be actionable by an agent
  that reads nothing else — that line is what `emphasis` repeats.
- **Name the tool, not the vibe.** "Run `nix develop -c cargo clippy`" survives paraphrasing;
  "be careful with the toolchain" does not.
- **Include the failure.** A rule with no incident behind it gets deleted by the next person
  who finds it inconvenient. Record the incident in `docs/inventory.md` and keep the shape of
  it in the body.
- **Do not restate a lower layer.** If `rules/core/` says it, a `rules/rust/` rule must not
  repeat it.
- **Do not edit a lower-layer rule to suit one repo.** Add a higher-layer rule with
  `overrides:` — and only if the layers genuinely conflict.

## 4. Verify it fires

```sh
cargo run -- rules                    # does it activate anywhere, or is it an orphan?
cargo run -- list --project <name>    # is it in the set you expected, at the right layer?
cargo test                            # the suite parses the whole tree
```

An orphan rule is a rule that does not exist in practice. `playbook rules` exists to make that
visible, because the predecessor skills sat unloaded for months without anyone noticing.

## 5. Install it

```sh
cargo run -- install --project <name> --repo <path>
cargo run -- check --project <name> --repo <path>   # what CI would run
```

Installing replaces only the managed block, so hand-written content in the repo's file is
untouched. If the rule should reach a repo that is not yet in `projects/`, add the project
first — see `templates/project/`.
