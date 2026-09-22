# Precedence

## Resolution order

Rules are collected low → high, then rendered. Within the collection, sort is by
`layer` order, then `priority` descending, then `id` ascending — the last of those exists so
the output is byte-stable and `--check` can diff it.

```
core  →  rust  →  org  →  project  →  target addenda
```

`<repo>/AGENTS.md` is not in the chain at all. The composer only ever writes *inside* a
managed block, so hand-written content in that file is untouched by construction — there is
no rule to resolve, because the two never occupy the same bytes.

## Overrides

A rule with `overrides: rust-devshell-first` drops that rule from the collection. Overrides
are resolved in one pass, highest layer first, so a `project` rule beats an `org` rule beats
a `core` rule. Two rules at the same layer overriding each other is a bug: `--check` reports
it and the composer exits non-zero.

## Target overlays

A target is a model+harness pair that consumes rules. `models/<target>/overlay.conf` is flat
`key: value`, same parser as rule frontmatter.

| Key | Effect |
| --- | --- |
| `model`, `harness` | Documentation only. Not used in resolution |
| `include` | Path prefixes to add. Only needed for `activation: manual` rules, which are otherwise never auto-included |
| `exclude` | Rule ids or path prefixes to drop |
| `emphasis` | Rule ids to repeat in a short preamble at the top of the output |
| `addenda` | Filenames in `models/<target>/addenda/` appended after all rules |
| `default_file` | Filename `playbook install` writes into when `--file` is not given |
| `instruction_files` | The harness's instruction-file priority order, most significant first. Empty means the harness merges everything it finds, so nothing can shadow anything. `playbook install` and `check` warn when an existing file outranks the one being written — see `models/zed/overlay.conf` for the worked case |

### Why `emphasis` exists

Long context degrades instruction-following, and the rules most worth following are the ones
most likely to be buried. `emphasis` repeats those rules — verbatim, ids intact — in a
`## Non-negotiable` block at the top of the generated file, so they are read first and read
twice.

This is the honest mechanism for "this model needs to be told more firmly". It does not
change the rule, so a target cannot quietly hold a weaker version of a constraint than the
source of truth. Which ids a target emphasises is a claim about that model, and belongs in
the overlay with a comment saying when it was last checked.

### What an overlay must not do

- **Must not add a rule that contradicts a rule.** An overlay that needs to beat a project
  rule is a signal the rule is wrong. Fix the rule.
- **Must not carry org or project specifics.** `models/zed/` applies to every repo in the
  playbook; a Zed quirk that only matters for one repo belongs in that repo's project file.
- **Must not be a place to park prose.** An addendum is for harness mechanics — tool names,
  how a nudge is phrased, what a sandbox refuses. Behavioural rules go in `rules/`.

## Worked example

`shared-crates` + `claude-code`. First the activation filter, which is mechanical:

```
core/*           activation: always          → 10 rules
rules/rust/*     activation: language:rust    →  4 rules   (the project declares rust)
rules/org/defrag activation: org:defrag       →  7 rules   (the project declares defrag)
projects/shared-crates/rules/*                →  2 rules   (repo-specific)
                                              ────────────
                                                23 rules
```

Nothing is excluded and nothing is overridden, so `playbook list --project shared-crates`
shows 23 rows in layer order and no `superseded` section. That is the normal case: the
precedence machinery is only visible when it has work to do.

Then the `claude-code` overlay:

```conf
emphasis: working-first, never-make-things-up, test-preservation, verify-before-claiming
addenda:  no-privately-preamble.md, tool-names.md
```

Output: a `## Non-negotiable` block with four rule leads repeated, the 23 rules in layer
order each under its own heading with an HTML comment naming its source, then the two
Claude-harness addenda demoted one heading level so they sit under the rules rather than
competing with them.

`archivist` resolves to 15 — the same 10 core and 4 rust rules, its own single project rule,
and **no org rules**, because it declares `org: hodlcroft`. `cnft-dev-workers` resolves to 27:
the same core, rust and org sets, plus six project rules of its own.
