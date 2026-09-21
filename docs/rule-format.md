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
layer: rust
activation: language:rust
priority: 50
overrides:
targets:
---

Body. Markdown. Whatever length the rule needs.
```

## Fields

| Field | Required | Values | Meaning |
| --- | --- | --- | --- |
| `id` | yes | kebab-case, unique across the whole repo | The contract. Referenced by `overrides:` and by an overlay's `exclude:` / `emphasis:` |
| `title` | yes | plain text, one line | Becomes the `##` heading in the generated file |
| `layer` | yes | `core`, `rust`, `org`, `project` | Sort order in the output. Unknown layers sort last |
| `activation` | yes | see below | When the rule is included |
| `priority` | no | integer, default `50` | Higher sorts earlier within its layer |
| `overrides` | no | comma-separated ids | Rule ids this rule supersedes. Those rules are dropped at compose time and the drop is reported |
| `targets` | no | comma-separated target names | Restricts the rule to those targets. Empty means all |

### `activation`

| Value | Included when |
| --- | --- |
| `always` | Always. Use for rules that hold for every model, language and org |
| `language:rust` | The project declares `languages: rust` |
| `org:defrag` | The project declares `org: defrag` |
| `project:shared-crates` | The project is `shared-crates` |
| `manual` | Never automatically. A target must name the rule's id in its `include:` |

`manual` exists for rules that are real but narrow — a migration procedure that applies once,
a rule that is only correct while a specific constraint is in force. Manual rules live in
`rules/` so they are versioned and reviewable, without being loaded into every session.

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
