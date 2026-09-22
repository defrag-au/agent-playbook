# Rule format

Every rule is one markdown file with a **flat** frontmatter header. Flat is a hard
requirement, not a style preference: the composer is POSIX `sh` with no YAML parser, so a
rule must be readable by `awk` looking for `key: value` at the start of a line. Nesting,
multi-line strings and block sequences are not available. If a rule needs structure beyond
this, it needs to be a reference file instead.

```markdown
---
id: rust-devshell-first
title: Rust tooling lives behind the Nix devshell
layer: language:rust
activation: language:rust
priority: 50
overrides:
targets:
---

## Directive

`cargo` is not on `PATH`. Wrap every command:

```sh
nix develop -c cargo build --workspace
```

In a sandboxed shell, use `direnv exec . <cmd>` — see below.

## Rationale

The failure reads as a permissions problem rather than a missing toolchain, which is why it is
worth stating before it is worth debugging.
```

### The two sections are the format

**`## Directive` is required and is the only part that is compiled.** It is the terse,
concrete direction an agent needs. `## Rationale` is optional and is **never emitted** — it is
the argument for keeping the rule, read by a person deciding whether to.

The split exists because the two have opposite optima. The compiled block competes for an
agent's attention on every turn; the rationale is a maintenance document that only has to be
convincing once. Compiling the source verbatim spends the agent's budget on the second job.

A rule with no `## Directive` is returned whole, with a compiler warning — the test suite
(`every_rule_has_a_directive_section`) is what makes it a hard failure. Nothing is dropped
either way, because a rule that loses its text silently is worse than a long one.

### Writing the directive

- **Lead with the rule.** The first **bullet or paragraph** is repeated verbatim by a
  target's `emphasis:`, so it must be actionable on its own — and for a bullet list that means
  the *first bullet*, so make it the rule's core rather than a narrow case. `Rule::lead()`
  takes the first bullet plus its indented continuation lines, or the first paragraph if the
  directive opens with prose.
- **Name the tool, not the vibe.** A concrete command survives paraphrasing; an adjective does
  not.
- **Keep it under about twenty lines.** That is a bar, not a law — the traps list is longer
  because seven traps do not compress into one. The measured result across the tree is −40%
  against the old whole-body output; rules that were already pure lists compress least.
- **Use the notation.** `situation → action` on one line · `·` between prohibitions in a list ·
  a table where the content is a mapping · a code block where it is a command. Imperative
  fragments, not sentences.
- **Emit only the delta from model defaults.** A frontier model already knows serde derives,
  `snake_case`, and what `cargo fmt` does. What it does not know is this project's specifics —
  spend the budget there.
- **Do not restate a lower layer.** If `rules/core/` says it, a `rules/rust/` rule must not
  repeat it.
- **Do not edit a lower-layer rule to suit one repo.** Add a higher-layer rule with
  `overrides:` — and only if the layers genuinely conflict.

### Writing the rationale

Write it for the person who is about to delete the rule. Include the incident, the measurement,
the alternatives considered, and anything that would otherwise be rediscovered. The incidents
that predate this repository are in `docs/inventory.md`; new ones belong here, next to the rule
they justify.

### Why `layer` is spelled out

The layer name is `language:rust`, not `rust`. A bare name is ambiguous between a language
and a typo — `layer: org` and `layer: organizaton` look the same to a parser that accepts any
unknown name as a language layer, and the typo would silently sort into the wrong place.
Spelling the namespace out means an unknown layer fails to parse, and the loader reports it.

## Fields

| Field | Required | Values | Meaning |
| --- | --- | --- | --- |
| `id` | yes | kebab-case, unique across the whole repo | The contract. Referenced by `overrides:` and by an overlay's `exclude:` / `emphasis:` |
| `title` | yes | plain text, one line | Becomes the `##` heading in the generated file |
| `layer` | yes | `core`, `language:<name>`, `org`, `project` | Sort order in the output |
| `activation` | yes | see below | When the rule is included |
| `priority` | no | integer, default `50` | Higher sorts earlier within its layer |
| `overrides` | no | comma-separated ids | Rule ids this rule supersedes. Those rules are dropped at compose time and the drop is reported |
| `targets` | no | comma-separated target names | Restricts the rule to those targets. Empty means all |

All four required keys must be **present and non-empty**. A missing one is a load error, not
a default — a rule with no `activation` that quietly became `always` is a rule that applies
somewhere nobody chose. `manual` exists for rules that are real but narrow: a migration that
applies once, a constraint that is only correct while something else is in force. Manual
rules live in `rules/` so they are versioned and reviewable, without being loaded into every
session.

### `activation`

| Value | Included when |
| --- | --- |
| `always` | Always. Use for rules that hold for every model, language and org |
| `language:rust` | The project declares `languages: rust` |
| `org:defrag` | The project declares `org: defrag` |
| `ecosystem:defrag` | The project declares `ecosystems: defrag` |
| `project:shared-crates` | The project is `shared-crates` |
| `manual` | Never automatically. A target must name the rule's id in its `include:` |

`org` and `ecosystem` answer different questions, and a rule belongs to whichever one it is
actually true of:

- **`org` is who operates the repository** — our services, a client's codebase.
- **`ecosystem` is what the repository is built with** — its devshell, its toolchain, and the
  conventions that follow from them.

They coincide for most repositories, and where they do, `org` is the shorter thing to write. They
diverge where a repository lives under one directory and is built with another's conventions:
`~/code/hodlcroft/compositor` is operated by hodlcroft and takes its shell — and so its `at-*`
toolkit — from `defrag-nix`. A rule about the toolkit is true of it and activates on
`ecosystem:defrag`; a rule about the cnft services is not true of it and activates on `org:defrag`.

The field is independent of `layer`, which decides **where a rule sorts**, not where it applies.
The two usually agree; where they do not, the activation is the precise statement and the layer is
just a rank.

## Writing the body

- **Lead with the rule, not the rationale.** The first line should be actionable by an agent
  that reads nothing else. Rationale goes below.
- **Include the failure.** A rule without a concrete "this went wrong" is a rule that gets
  deleted by the next person who finds it inconvenient. `docs/inventory.md` keeps the
  incident; the body should keep at least the shape of it, because the incident is what makes
  the rule memorable to a model.
- **Name the tool, not the vibe.** "Run `nix develop -c cargo clippy`" beats "be careful with
  the toolchain". Concrete commands survive paraphrasing; adjectives do not.
- **Do not restate a lower layer.** If `rules/core/` says it, a `rules/rust/` rule must not
  repeat it. Restatement is how the current CLAUDE.md/AGENTS.md pair came to disagree.
- **Keep it under a screenful.** If it is longer, it is a reference file with a rule wrapped
  around it.

## Superseding a rule

Do not edit a rule in a lower layer to suit one project. Add a rule in the higher layer with
`overrides:` naming the original. The composer reports the replacement, so the override is
visible in the output rather than being an invisible divergence:

```
Superseded by a higher layer in this project:

- `rust-devshell-first`
```

**No rule currently overrides another.** The mechanism exists for the first genuine conflict,
and is deliberately unused until there is one — an override invented to demonstrate the
feature is a second version of a rule that was already correct, which is the exact failure
this repository was built to stop.

When one does appear, treat it as a signal. If a rule is overridden by three projects, it is
wrong at its current layer: move it up or delete it.

The loader enforces the direction: a rule may only override one in a **strictly lower**
layer. Same-layer and upward overrides are both errors, because neither has a defined answer
and the shell implementation resolved them by accident of sort order.
