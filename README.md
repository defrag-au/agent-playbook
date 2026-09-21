# agent-playbook

A single, transferable source of truth for how I want coding agents to work — across
models, across harnesses, across repositories.

The repository is **data plus one small tool**. The rules are markdown files with a flat
frontmatter header; `src/` resolves them for a given project and target and renders the
result into whatever file a given agent actually reads.

## Why this exists

Agent instructions currently live in five unconnected places, and they drift:

| Where | What it is | Problem |
| --- | --- | --- |
| `~/.claude/CLAUDE.md` | Claude memory | Invisible to any non-Claude agent; mixes universal rules with Claude-harness quirks |
| `~/.agents/skills/*` | Eight "skills" | They are standing constraints, not skills — every one has `description: (no description)` and `disable-model-invocation: true`, so none is ever invocable. They are rules wearing a skill's file layout |
| `<repo>/CLAUDE.md` | Per-repo notes | Duplicated by hand into `AGENTS.md`, then drifts |
| `<repo>/AGENTS.md` | Per-repo notes | Same content as `CLAUDE.md`, now *contradicting* it — the devshell rule exists only here |
| `~/.claude/crate-refs/*` | Crate cheat sheets | Real research artefacts, filed as memory |

Two concrete failures are already in the tree. `cnft.dev-workers/CLAUDE.md` says lint with
`cargo clippy --fix …`; the `AGENTS.md` beside it says `nix develop -c cargo clippy …`. Both
are read by agents, and only one is correct. And nothing anywhere records *which rule wins*
when a repo file and a global file disagree — so the answer has been "whichever the agent
read last".

This repository fixes the shape of the problem: one file per rule, an explicit precedence
model, and generated output so a repo's agent file cannot drift from the rule it cites.

## The five layers

Content is separated by **kind**, because each kind has a different lifetime and a different
delivery mechanism.

| Layer | Kind | Lifetime | Delivered as |
| --- | --- | --- | --- |
| `rules/` | Standing constraints — always on | Changes rarely, applies to every turn | Concatenated into the agent file |
| `skills/` | On-demand procedures | Invoked when a task matches | Copied to `~/.agents/skills/` |
| `memory/` | Durable facts about me and my environment | Changes when the environment does | Concatenated into the agent file |
| `references/` | Research artefacts — crate cheat sheets, API notes | Per-version, disposable | Read on demand, path cited from a rule |
| `models/` | Per-model and per-harness tweaks | Changes per model release | Overlay applied at compose time |

The distinction that matters most: **a rule constrains every response, a skill runs a
procedure, a reference is looked up.** The eight mis-filed skills move into `rules/`
(most of them) or `skills/` (the two that are genuinely procedural — crate research and
the widget screenshot loop).

## Precedence

Rules resolve low → high. A higher layer may override a lower one, and must say so
explicitly with an `overrides:` field naming the rule it replaces.

```
1. rules/core/                 universal — every model, every language, every repo
2. rules/<language>/           rust/, and whatever else appears
3. rules/org/<org>/            defrag/ — ecosystem-wide conventions
4. projects/<repo>/            this repository only
5. <repo>/AGENTS.md            hand-written, outside the managed block — the user's own file, always wins
```

`models/<target>/` is not a precedence layer. It is a *filter and amplifier* over the
result: it can exclude rules, emphasise them, and append harness-specific addenda. A model
overlay never silently outranks a project rule — if a target needs to beat one, that is a
signal the rule is wrong, not that the overlay should win.

Full resolution rules, including how `activation` and `overrides` are evaluated, are in
[`docs/precedence.md`](docs/precedence.md).

## Repository layout

```
Cargo.toml        the tool: no dependencies, std only
src/              resolution engine + CLI
  frontmatter.rs    flat `key: value` readers
  model.rs          Rule, Project, Target, Layer, Activation
  load.rs           finding the root, reading the data tree
  resolve.rs        the resolution engine
  render.rs         the managed block
  install.rs        splice and check
  main.rs           the CLI
tests/            spec tests for the resolution model
rules/            one file per standing constraint, flat frontmatter
  core/           model-, language- and org-agnostic
  rust/           activated for Rust repositories
  org/defrag/     activated for defrag-org repositories
skills/           on-demand procedures, real SKILL.md files with descriptions
memory/           environment facts and durable preferences
references/       crate cheat sheets and API notes
  crates/         one file per crate@version
models/           compose-time overlays: generic, claude-code, deepseek-flash, zed
projects/         one directory per repository: project.conf + rules/
templates/        scaffolds for new rules, skills, projects and overlays
docs/             precedence model, rule format, migration inventory
dist/             generated output (gitignored)
```

`models/` holds **targets**, not just models. A target is whatever consumes rules — usually
a model+harness pair. `claude-code/` declares `model: claude`, `harness: claude-code`;
`zed/` is harness-only and applies regardless of model. See [`models/README.md`](models/README.md).

## Using it

```sh
# What would this project's rule set look like?
playbook list --project shared-crates

# Print the block, or write it to a file.
playbook compose --project shared-crates --target claude-code
playbook compose --project shared-crates --out dist/block.md

# Write it into the repo, replacing only the managed block.
playbook install --project shared-crates --repo ~/code/defrag/shared-crates

# Fail if the repo's managed block is stale — the cargo fmt --check of agent rules.
playbook check --project shared-crates --repo ~/code/defrag/shared-crates

# Every rule, and which projects it reaches. Flags any rule that activates nowhere.
playbook rules
```

Run it from anywhere inside the playbook (the root is found by walking up), or pass
`--root`. To build:

```sh
nix develop -c cargo build --release   # or: cargo build --release, if cargo is on PATH
```

### Why the tool is Rust and not a shell script

The first version was POSIX `sh` with `awk`, on the theory that a dependency-free script
could run on a machine before its devshell existed. It was replaced after it produced two
**silent wrong-answer** bugs in an afternoon:

1. `awk`'s `NR == FNR` idiom for splitting two files fails when the first file is empty —
   `NR` and `FNR` stay in lock-step, every line of the second file is treated as part of the
   first, and a project with no overrides resolved to **zero rules** while reporting success.
2. `return` inside a pipeline runs in a subshell, so `prefix_match` always returned "no
   match". The exclusion feature never worked.

Neither failed loudly, and neither was the kind of thing a script could be tested for
cheaply. The Rust version has 61 tests, an exhaustive `match` on activation, and a rule that
will not compile if a variant is unhandled. The dependency-free property survives — the
crate has no dependencies at all, so it still builds with no network and no registry cache.

## Adding a rule

1. Copy [`templates/rule.md`](templates/rule.md) into the right layer directory.
2. Fill the frontmatter and write the body. The `id` is the contract — other files
   reference it in `overrides:` and `emphasis:`.
3. Run `playbook rules` to confirm it activates for the projects you expect, and that it is
   not an orphan.
4. Run `playbook list --project <name>` for each affected project, then re-install for any
   repo whose managed block changes.
5. `nix develop -c cargo test` — the suite parses the whole tree, so a malformed rule fails
   the build rather than disappearing from a generated file.

The [`capture-rule`](skills/capture-rule/SKILL.md) skill is the same procedure written for
an agent to follow mid-session, which is when the pattern is actually noticed.

## Open questions

- **A flake.** The repo has a `Cargo.toml` and no `flake.nix`, so its own `devshell-first`
  rule cannot be followed from inside it. Two shapes are possible and they trade off
  differently: reuse `defrag-nix`'s `rust-worker-stack` like the sibling repos do (in
  lock-step, but ties a *transferable* repository to one org's flake), or a self-contained
  `nixpkgs` shell (transferable, but a second toolchain definition to maintain). Undecided.
- **CI.** `playbook check` is the natural CI gate for the consuming repos, but it needs the
  playbook present. Until there is a remote, the block is installed by hand.

## Non-goals

- **Not a prompt library.** Nothing here is a prompt template. Rules are constraints on
  behaviour, not reusable strings.
- **Not model benchmarking.** `models/` records what a target needs to be *told*, not how
  well it performs.
- **No rule without a failure behind it.** Every rule in here exists because something went
  wrong without it. `docs/inventory.md` records that failure next to the rule it produced —
  the prose in a rule is the *conclusion*, and the incident is the argument for keeping it.
