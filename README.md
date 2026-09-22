# agent-playbook

A single, transferable source of truth for how I want coding agents to work — across
models, across harnesses, across repositories.

The repository is **data plus two small tools**. The rules are markdown files with a flat
frontmatter header; `src/` resolves them for a given project and target and renders the result
into whatever file a given agent actually reads. `tools/` builds the read-only binaries those
agents use to look at code and history without a shell pipeline — see
[The inspection toolkit](#the-inspection-toolkit).

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
procedure, a reference is looked up.** The eight mis-filed skills went six into `rules/`, one
into `skills/crate-research`, and one — `rust-environment-verification` — to the bin, because
its advice had been made wrong by the devshell. See [`docs/inventory.md`](docs/inventory.md).

## Precedence

Rules resolve low → high. A higher layer may override a lower one, and must say so
explicitly with an `overrides:` field naming the rule it replaces.

```
1. rules/core/                 universal — every model, every language, every repo
2. rules/<language>/           rust/, and whatever else appears
3. rules/org/<org>/            defrag/ — ecosystem-wide conventions
4. projects/<repo>/rules/      this repository only
```

A repository's own `<repo>/AGENTS.md` is **not in the chain**, because the composer only ever
writes between its own markers. Hand-written content in that file cannot conflict with a
rule, because the two never occupy the same bytes — which is a stronger guarantee than
winning an argument about precedence.

`models/<target>/` is not a precedence layer. It is a *filter and amplifier* over the
result: it can exclude rules, emphasise them, and append harness-specific addenda. A model
overlay never silently outranks a project rule — if a target needs to beat one, that is a
signal the rule is wrong, not that the overlay should win.

Full resolution rules, including how `activation` and `overrides` are evaluated, are in
[`docs/precedence.md`](docs/precedence.md).

## Repository layout

```
Cargo.toml        the workspace manifest; the composer is std-only, deliberately
flake.nix         the devshell, and packages: `playbook` and `agent-tools`
src/              resolution engine + CLI
  frontmatter.rs    flat `key: value` readers
  model.rs          Rule, Project, Target, Layer, Activation
  load.rs           finding the root, reading the data tree
  resolve.rs        the resolution engine
  render.rs         the managed block
  install.rs        splice and check
  main.rs           the CLI
tools/            the read-only agent toolkit: three binaries, four crates
  at-core/          the output contract and path containment, shared
  at-peek/          the working tree — stat, slice, search
  at-recall/        history and state — state, log, diff, pr
  at-describe/      the catalogue, which opens nothing
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
docs/             precedence model, rule format, migration inventory, inspection tools
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
cheaply. The Rust version has 63 tests, an exhaustive `match` on activation, and a rule that
will not compile if a variant is unhandled. The dependency-free property survives — the
crate has no dependencies at all, so it still builds with no network and no registry cache.

## The inspection toolkit

The other half of this repository is not a rule. It is the thing that makes it possible to stop
writing rules about shell pipelines: three read-only binaries, built from `tools/`, that give an
agent the reads it needs without an approval per command and without a bound hidden in a pipe.

| Binary | Reads | Verbs |
| --- | --- | --- |
| `at-peek` | working-tree files, and spawns nothing at all | `stat`, `slice`, `search` |
| `at-recall` | `.git` objects and refs, through one subprocess — `git`, read verbs only | `state`, `log`, `diff`, `pr` |
| `at-describe` | nothing; it prints the catalogue | the two verb tables |

`rg` becomes `at-peek search`, `sed -n '40,60p' f.rs` becomes `at-peek slice f.rs:40-60`, and
`git status` plus `git branch` plus `git log -1` becomes `at-recall state`. The mapping an agent is
held to is [`rules/org/defrag/agent-tools.md`](rules/org/defrag/agent-tools.md); the traps are
[`skills/inspect-code/SKILL.md`](skills/inspect-code/SKILL.md).

```sh
at-peek search 'render_claim' --count   # matches per file, with the true total
at-recall state --summary               # branch, HEAD, changed paths — the frame, no rows
at-recall pr --base main                # the facts a PR description is written from
at-describe                             # the catalogue, one screen
```

Working on the toolkit itself, the same reads are `cargo run -p at-recall -- state`, or the built
binaries in `result/bin` from `nix build .#agent-tools`.

Three properties make the tools cheap to allowlist once, and each is a test rather than an
intention:

- **Bounded, and the bound is stated.** `# 50 of 143 matches in 27 files` is a fact about the whole
  search, not about the part that was printed. A truncated answer cannot be mistaken for a complete
  one, which is the failure `| head -20` produces by construction.
- **A closed grammar.** The flags are read from the catalogue the help renders, so the two cannot
  drift; there is no `--` pass-through and no environment configuration, and an unknown flag is exit
  2 rather than an argument forwarded to something else.
- **The exits are commands.** `# next: at-recall diff HEAD --patch · the hunks` is built from the
  invocation that produced it and runs exactly as printed — an *address*, never a handle, so no
  state crosses invocations.

The tier each binary sits in is the reason there are three, because a prefix allowlist can only
see a difference that is in the binary. `at-describe` opens no file at all. `at-peek` reads files
and runs nothing. `at-recall` runs `git` and nothing else, with read verbs only, a deny-list test
asserting no mutating verb is reachable, a repository's pagers and diff drivers neutralised, and
`@{…}` refused by syntax because the reflog is not read.

`defrag-nix` installs them into every defrag devshell, so they are on `PATH` in an interactive
shell in any of those repositories; a shell an agent spawned reaches for them as
`direnv exec . at-peek …`.

The verbs above are built. `tree`, `find`, `outline` and `scope` in `at-peek`; `show`, `blame`,
`why` and `churn` in `at-recall`; and the `review` and `release` recipes are designed and not
written — [`docs/inspection-tools.md`](docs/inspection-tools.md) is the design record, and
`at-describe` answers with what exists rather than what was planned.

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
