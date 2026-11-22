---
name: capture-rule
description: Use when you notice something that should be different — a tool that misdirected, a rule that was ignored, a repeated workaround, a correction from the user — or when the user states a constraint. Records it as a finding first, and turns it into a rule only after a decision.
---

# Capturing a finding, and turning it into a rule

Two stages, and they are not the same task. The first is cheap and happens now. The second is
expensive and happens after a decision.

Most of the time you only do the first one. That is the intended outcome, not a shortcut.

## 1. Capture the finding

Copy `templates/finding.md` to `.playbook/findings/<YYYY-MM-DD>-<slug>.md` in the repository you
are working in, and fill in two sentences.

- **Do not choose a layer.** Do not write rule text. Do not think about activation.
- **Point at the evidence.** `thread` plus `message` reopens it with
  `zed-thread-audit --behaviour <thread>`. A finding without a pointer is an anecdote.
- **`class: unknown` is fine**, and is better than a guess. The report stage may reclassify.
- **One `write_file`.** If capture costs more than the thing it describes, it will not happen next
  time, and the pattern is lost.

Stop here unless a decision has already been made. The full loop — the field definitions, the
classes, the report recipe and the four outcomes — is in
`~/code/defrag/agent-playbook/docs/findings.md`.

## 2. Check for a decision

A decision is a `## Decision` section in the finding naming one of `fix-now`, `fix-later`,
`accept-as-is` or `reject`.

- **No decision → stop.** Leave the finding open. It is not a rule yet.
- **`fix-later`, `accept-as-is`, `reject` → stop.** Nothing is produced.
- **`fix-now` → continue.** The decision should already name the measure. If it does not, ask for
  one before writing anything.

## 3. Decide what kind of rule it is

| What the decision says | Where it goes |
| --- | --- |
| A constraint with no trigger ("always X", "never Y") | `rules/<layer>/` |
| A procedure with a trigger ("when doing X, do it this way") | `skills/<name>/SKILL.md` |
| A fact about the environment ("cargo is not on PATH here") | `memory/environment.md` |
| A crate's API surface | `references/crates/` |
| Something true of one model or harness | `models/<target>/addenda/` |
| Something true of exactly one repo | that repository's `.playbook/rules/` |

The most common misfiling is a constraint written as a skill. See `skills/README.md` for why that
is not a small mistake: a rule stored as a skill may never fire at all.

## 4. Choose the layer with the layer test

| Layer | Test |
| --- | --- |
| `core` | Would I want this for a Go project written by someone else? |
| `language:rust` | Is it true of the language, or of the Rust tooling I use? |
| `org:defrag` | True across the defrag ecosystem, but not of my work generally? |
| `project` | True of exactly one repository? |

Move a rule **up** the moment a second consumer needs it, and **down** the moment only one does.
A rule at the wrong layer is how the duplication this repository exists to fix gets started.

## 5. Write it

Copy `templates/rule.md`. Then:

- **Lead with the rule, not the rationale.** The first line must be actionable by an agent that
  reads nothing else — that line is what `emphasis` repeats.
- **Name the tool, not the vibe.** "Run `nix develop -c cargo clippy`" survives paraphrasing;
  "be careful with the toolchain" does not.
- **Cite the finding.** Put its path in the rule's *Why* section. A rule with no incident behind
  it gets deleted by the next person who finds it inconvenient.
- **Do not restate a lower layer.** If `rules/core/` says it, a `rules/rust/` rule must not repeat
  it.
- **Do not edit a lower-layer rule to suit one repo.** Add a higher-layer rule with `overrides:` —
  and only if the layers genuinely conflict.

## 6. Verify it fires

```sh
cargo run -- rules                    # does it activate anywhere, or is it an orphan?
cargo run -- list --repo <path>       # is it in the set you expected, at the right layer?
cargo test                            # the suite parses the whole tree
```

An orphan rule is a rule that does not exist in practice. `playbook rules` exists to make that
visible, because the predecessor skills sat unloaded for months without anyone noticing.

## 7. Install it, and record it

```sh
cargo run -- install --target <target> --repo <path>
cargo run -- check   --target <target> --repo <path>   # what CI would run
```

Installing replaces only the managed block, so hand-written content in the repo's file is
untouched. If the repository has no `.playbook/` yet, scaffold it there first with `playbook init`
— see `templates/project/`.

Then add the rule and the finding's path to the table in `docs/inventory.md`. That table is the
provenance: the rule's prose is the conclusion, the finding is the argument for keeping it.

## 8. Close the finding

Append to the finding's `## Decision` section the rule it produced:

```markdown
Rule: rules/org/defrag/<id>
```

A `fix-now` that produced no rule is a finding whose decision should be revisited.
