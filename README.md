# agent-playbook

A single, transferable source of truth for how I want coding agents to work — across
models, across harnesses, across repositories.

The repository is **data plus a small composer and an inspection toolkit**. The rules are
markdown files with a flat frontmatter header; `src/` resolves them for a given project and
target and renders the result into whatever file a given agent actually reads. `tools/` builds
the read-only binaries those agents use to look at code, history and a private fleet without a
shell pipeline — see [The inspection toolkit](#the-inspection-toolkit).

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
4. <repo>/.playbook/rules/     this repository only
```

A repository states its own identity in `<repo>/.playbook/project.conf` — its name, org,
ecosystem, renderers and languages — and owns its rules in `<repo>/.playbook/rules/`. This
repository never names a consuming repository: it supplies the layers above and the composer
that reads the consumer's declaration.

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
flake.nix         the devshell, and packages: `playbook`, `agent-tools`, `browser-shot`, `tui-shot`, `image-shot`, `fleet-sync` and `travel-secrets`
src/              resolution engine + CLI
  frontmatter.rs    flat `key: value` readers
  model.rs          Rule, Project, Target, Layer, Activation
  load.rs           finding the root, reading the data tree and a consumer's manifest
  resolve.rs        the resolution engine
  render.rs         the managed block
  install.rs        splice and check
  init.rs           scaffolding a consumer's `.playbook/`
  skills.rs         delivering OCFL-stored skills into a repository
  main.rs           the CLI
tools/            the agent toolkit, plus the skill store
  at-core/          the output contract, path containment and text reading, shared
  at-peek/          the working tree — stat, slice, search
  at-crate/         installed crate source and build output — where, installed, files, slice, search, out
  at-recall/        history and state — state, log, diff, pr
  at-reach/         a private fleet tree — root, hosts, probe
  at-cost/          what the machine is costing, by repository — repos, tree, pid
  at-cardano/       read-only Cardano chain enquiry through Koios
  at-describe/      the catalogue, which opens nothing
  browser-shot/     browser captures, scenarios and frame bursts
  tui-shot/         terminal captures and scenario runs in a PTY it owns
  image-shot/       image inspection, derived views and diffs on the IIIF grammar
  fleet-sync/       payloads for a satellite — bundle, plan, apply (writes)
  travel-secrets/   a whitelisted, re-encrypted pass-store subset for travel (writes, decrypts)
  skill-store/      OCFL publish and export for skills (writes, and fetches)
  zed-thread-tools/ thread auditing, recovery and a TUI
tests/            spec tests for the resolution model
rules/            one file per standing constraint, flat frontmatter
  core/           model-, language- and org-agnostic
  rust/           activated for Rust repositories
  org/defrag/     activated for defrag-org repositories
skills/           authoring tree: one directory per skill, published into `.skill-store/`
.skill-store/     the OCFL store — one immutable version per published skill
memory/           environment facts and durable preferences
references/       crate cheat sheets and API notes
  crates/         one file per crate@version
models/           compose-time overlays: generic, claude-code, deepseek-flash, zed
                  (models/zed-personal writes the personal instructions file)
templates/        scaffolds for new rules, skills, projects and overlays
docs/             precedence model, rule format, skills delivery, migration inventory, inspection and browser tools
dist/             generated output (gitignored)
```

`models/` holds **targets**, not just models. A target is whatever consumes rules — usually
a model+harness pair. `claude-code/` declares `model: claude`, `harness: claude-code`;
`zed/` is harness-only and applies regardless of model. See [`models/README.md`](models/README.md).

## Using it

Every repository that consumes the playbook carries its own declaration at
`.playbook/project.conf`. Run the composer inside that repository; it walks up to the nearest
manifest, so a subdirectory works too. There is no `--project`: a project is a place, not a
name in a registry here.

```sh
# What does this repository resolve to?
playbook list

# Print the block, or write it to a file.
playbook compose --target claude-code
playbook compose --target claude-code --out dist/block.md

# Write it into the repo, replacing only the managed block.
playbook install --target claude-code

# Fail if the repo's managed block is stale — the cargo fmt --check of agent rules.
playbook check --target claude-code

# Every shared rule, and the activation that reaches it.
playbook rules

# Deliver the skills this repository declares, into its harness's project directory.
playbook skills sync  --target zed
playbook skills check --target zed
playbook skills sync  --target all            # both harnesses, or every one that has a directory
```

`skills sync` exports each declared source through `skill-store` and records the version, tree
digest and target in `.playbook/skills.lock`; `skills check` re-exports and compares, byte for
byte. A manifest with `skills_scope: global` delivers into `~/.agents/skills` and
`~/.claude/skills` instead, with the lock beside the manifest — that is how the personal
catalogue in `fleet` keeps every harness current in one command. An edit you have not published
is delivered from a gitignored `.playbook/skills.patch`, and every line it affects says
`(patched)`. See [`docs/skills.md`](docs/skills.md).

A repository that has no manifest yet gets one with `playbook init`, which writes
`.playbook/project.conf` and an empty `.playbook/rules/`:

```sh
cd ~/code/defrag/example
playbook init --name example --target claude-code
```

Three commands take paths for consumers whose manifest and output are not in the same place:
`--manifest <repo>/.playbook/project.conf` names the declaration, `--repo <dir>` names the
repository whose block is written or checked, and `--root <dir>` names the rule tree. The
personal instructions file uses the first two — its manifest lives in the fleet repository and
its output is `~/.config/zed/AGENTS.md`:

```sh
playbook check \
  --manifest ~/code/DamonOehlman/fleet/personal/.playbook/project.conf \
  --repo ~/.config/zed
```

Run `playbook` from anywhere inside this repository (the root is found by walking up), or pass
`--root`. `playbook` is also on `PATH` in every defrag devshell — `defrag-nix` wires it in
alongside the `at-*` toolkit — where it resolves against the rule tree it was packaged with, which
is what CI wants and what `playbook root` reports:

```sh
playbook check --target claude-code          # inside ~/code/defrag/cnft.dev-workers
playbook root     # /nix/store/…-playbook-0.1.0/share/playbook · the packaged copy
```

A rule change in this repository reaches a consumer shell after it is committed, pushed, and
re-locked in `defrag-nix`; until then, `--root ~/code/defrag/agent-playbook` resolves against the
working tree. To build:

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
cheaply. The Rust version has 106 tests, an exhaustive `match` on activation, and a rule that
will not compile if a variant is unhandled. The dependency-free property survives — the
crate has no dependencies at all, so it still builds with no network and no registry cache.

## The inspection toolkit

The other half of this repository is not a rule. It is the thing that makes it possible to stop
writing rules about shell pipelines: seven binaries, built from `tools/`, that give an agent the
reads it needs without an approval per command and without a bound hidden in a pipe.

| Binary | Reads | Verbs |
| --- | --- | --- |
| `at-peek` | working-tree files, and spawns nothing at all | `stat`, `slice`, `search` |
| `at-crate` | extracted crate source, `Cargo.lock`, and build-script output; spawns nothing | `where`, `installed`, `files`, `slice`, `search`, `out` |
| `at-recall` | `.git` objects and refs, through one subprocess — `git`, read verbs only | `state`, `log`, `diff`, `pr` |
| `at-reach` | a private fleet tree, and one read-only probe over `ssh` (or `sh` locally) | `root`, `hosts`, `probe` |
| `at-cost` | the process table, attributed to repositories; spawns nothing | `repos`, `tree`, `pid` |
| `at-cardano` | Koios over HTTPS, GET and query-POST only; the key is never printed | nouns and aspects: `tip`, `network`, `epoch`, `block`, `blocks`, `tx`, `utxo`, `address`, `account`, `asset`, `policy`, `pool`, `drep`, `proposal`, `script`, `datum`, `endpoints`, `query`, `run` |
| `at-describe` | nothing; it prints the catalogue | the six verb tables |

`rg` becomes `at-peek search`, `sed -n '40,60p' f.rs` becomes `at-peek slice f.rs:40-60`,
`grep -rn … ~/.cargo/registry/src` becomes `at-crate search <pattern> <crate>`, and `git status`
plus `git branch` plus `git log -1` becomes `at-recall state`. The mapping an agent is held to is
[`rules/org/defrag/agent-tools.md`](rules/org/defrag/agent-tools.md); the traps are
[`skills/inspect-code/SKILL.md`](skills/inspect-code/SKILL.md).

```sh
at-peek search 'render_claim' --count   # matches per file, with the true total
at-crate where headless_chrome          # version, source path, build output
at-crate search 'device_metrics' headless_chrome   # every mention in the crate
at-recall state --summary               # branch, HEAD, changed paths — the frame, no rows
at-recall pr --base main                # the facts a PR description is written from
at-reach hosts                          # the private fleet inventory
at-reach probe example-host             # OS, Nix, Tailscale, repos — read-only
at-cardano tip                            # the chain head, no key required
at-cardano tx <hash>                      # block, fee, size, input/output totals
at-cardano address utxos <address>        # the UTxOs at an address, with a page bound
at-cardano pool delegators <pool>         # who delegates, and their stake
at-cardano endpoints                      # every Koios path, and what answers it
at-cardano query tx_info --post --body '{"_tx_hashes":["<hash>"]}'   # the long tail
at-cardano run --scenario .tmp/block.dsl  # several questions, a value carried between them
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

The tier each binary sits in is the reason there are seven, because a prefix allowlist can only
see a difference that is in the binary. `at-describe` opens no file at all. `at-peek` reads files
and runs nothing. `at-crate` reads the cargo home, `Cargo.lock` and `target/`, and runs nothing —
in particular it never invokes `cargo`, so an uninstalled crate is an answer rather than a fetch.
`at-recall` runs `git` and nothing else, with read verbs only, a deny-list test asserting no
mutating verb is reachable, a repository's pagers and diff drivers neutralised, and `@{…}` refused
by syntax because the reflog is not read. `at-reach` reads a private fleet tree and runs one fixed
probe over `ssh` or `sh`; it has no write, sudo or secret verb. Its contract is
[`docs/fleet.md`](docs/fleet.md). `at-cost` reads the macOS process table and nothing else.
`at-cardano` is the one member with a socket: HTTPS to a Koios host chosen only by `--network`, GET
and query-POST only, redirects disabled, `/submittx` refused by name, and a key read from
`~/.config/koios/api_key` that no output ever prints.

`defrag-nix` installs them into every defrag devshell, so they are on `PATH` in an interactive
shell in any of those repositories; a shell an agent spawned reaches for them as
`direnv exec . at-peek …`.

The verbs above are built. `tree`, `find`, `outline` and `scope` in `at-peek`; `show`, `blame`,
`why` and `churn` in `at-recall`; and the `review` and `release` recipes are designed and not
written — [`docs/inspection-tools.md`](docs/inspection-tools.md) is the design record, and
`at-describe` answers with what exists rather than what was planned.

## The image tool

`tools/image-shot/` is outside the `at-` namespace for the same reason as `browser-shot` and
`tui-shot`: it writes derived views. It inspects and derives images on the IIIF worker's five-axis
operation grammar, so a crop is `--box X,Y,W,H`, a render is `full/400,/0/gray.png`, and the report
prints the canonical operation it applied.

```sh
image-shot info .tmp/capture.png          # format, dimensions, colour type, size
image-shot stats .tmp/capture.png         # mean/sd, alpha, colours, ink bbox, sharpness
image-shot crop .tmp/capture.png --box 0,0,780,200 --zoom 2
image-shot fit .tmp/capture.png --edge 24
image-shot diff .tmp/before.png .tmp/after.png --tolerance 2
image-shot run  --scenario .tmp/review.dsl
```

A scenario composes the same operations: `source`, `crop`, `fit`, `sheet`, `overlay`, `trim`,
`stats`, `diff` and `assert`, with `run.txt` / `run.json` beside the images. It is line-oriented and
has no shell action. The grammar is [`tools/image-shot/SCENARIO.md`](tools/image-shot/SCENARIO.md).

It reads PNG, JPEG, GIF and WebP, and refuses BMP, TIFF, SVG, AVIF, HEIC and PDF by name. Writes go
only into its artifacts directory, never over an input; a derived name is a hash of the source
bytes, the canonical operation and the tool version. The build is `anyhow` plus `image` with four
codecs and no native libraries. [`tools/image-shot/README.md`](tools/image-shot/README.md) is the
full contract. The procedure an agent follows is
[`skills/image-shot/SKILL.md`](skills/image-shot/SKILL.md), published into `.skill-store/`.

## The satellite sync tool

`tools/fleet-sync/` is outside the `at-` namespace because it writes. It turns a machine that
has the work into a payload for a machine that reads it: `bundle` writes one git bundle per
repository in the profile plus a flat manifest, and `plan`/`apply` run on the target, remove the
fixed artifact directories and reset the checkouts to the commits in that manifest.

```sh
fleet-sync bundle defrag-air --out /Volumes/travel/air
fleet-sync plan   defrag-air --from /Volumes/travel/air
fleet-sync apply  defrag-air --from /Volumes/travel/air --clean artifacts
```

The target is not contacted. `apply` refuses unless the local hostname matches the profile and
the profile does not write a repository; a checkout with tracked changes is blocked until
`--force`, and the payload is refused when it does not fit with 256 MiB of headroom. `--clean
artifacts` removes `target`, `.direnv`, `node_modules`, `.tmp` and `.venv`; `--clean reset` also
runs `git clean -xfd` and protects `.env`. The profile keys are `sync: bundle|none` and
`sync_clean: artifacts|reset|none`; see [`docs/fleet.md`](docs/fleet.md) and the `fleet-sync`
skill in the private fleet repository.

## The travel credentials tool

`tools/travel-secrets/` is the credential half of the same trip, and it is separate from
`fleet-sync` on purpose: moving commits and decrypting a password store are different approvals,
and their payloads belong in different directories.

```sh
travel-secrets --store ~/.password-store --whitelist fleet/travel/defrag-air.md \
               --recipient <TRAVEL_FPR> --out /Volumes/travel/air-secrets
```

The binary is this repository's devshell package; run the command there, or
`nix run .#travel-secrets -- …` from the checkout.

Every whitelisted `pass` entry is decrypted with the caller's keyring and re-encrypted to the
travel recipient through a pipe, so plaintext never reaches a file and the source store is read
only. The output is a self-contained store — entries plus a root `.gpg-id` naming the travel key
— and a flat `manifest` with the recipient, entry count, size and SHA-256 per entry. Symlinks are
refused, the source's nested `.gpg-id` files are not copied, and the output directory must be
empty.

It reads the `pass:` line of `fleet/travel/<host>.md`, which is a per-trip selection and nothing
else; the inventory and the procedure stay in the private fleet repository. The tool refuses a
non-terminal stdin unless `--allow-non-tty` is explicit, because its job is to decrypt secrets.
**An agent must not run it, `pass`, or `gpg --decrypt` against a real keyring** — prepare the
selection, then hand the command over. The crate's gpg round-trip test uses throwaway keys in a
temporary `GNUPGHOME`; it is still the human's command, not the agent's.

## The Zed thread tools

`tools/zed-thread-tools/` is deliberately outside the `at-` namespace. `zed-thread-audit`
reports size, message and tool-call counts, tool-output bytes and compaction state for Zed's
agent thread store, and `--explain` attributes a thread's largest outputs to the tool, message
and request argument without printing the result body; `zed-thread-recover` appends a
`Compaction(Summary)` message so a thread
that has hit its context limit can continue; `zed-thread-tui` is the unified library over
`sidebar_threads` and `threads.db`, driving archive, single-thread delete, bundle export/import
and recovery from a selected row. The audit binary opens the store read-only and never prints
message content; the writers refuse to run while Zed is open, and make a `VACUUM INTO` backup
before changing either store. None of them is linked into `agent-tools`, so the `at-`
allowlist keeps its promise.

## Adding a rule

1. Copy [`templates/rule.md`](templates/rule.md) into the right layer directory.
2. Fill the frontmatter and write the body. The `id` is the contract — other files
   reference it in `overrides:` and `emphasis:`.
3. Run `playbook rules` to confirm it activates for the repositories you expect, and that it is
   not an orphan.
4. Run `playbook list` in each affected repository, then re-install for any repo whose managed
   block changes. A rule that is true of one repository belongs in that repository's
   `.playbook/rules/`, not here.
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
