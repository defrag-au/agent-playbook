# rig

The plan for building against **rig**, and the evidence behind it. Written 2026-11-08 against
`0xPlaygrounds/rig` at `63431d79b` (the `main` of that day, 1728 commits), with `rig-core` 0.46.0
published 2026-10-31. The checkout used for every claim below was `~/code/github/rig`; the working
fork is now `~/code/defrag/rig` at `c8b20d3d7`, one commit past it, and both the plugin's tests and
the generated agent build against the fork.

The short version: **rig is not another coding agent. It is a kernel where a turn, a model call,
a tool call and a gate decision are entities moving through stages, and the coding agent is a set
of plugins on it.** That is the part worth evaluating, because it changes what an integration can
be — not just where the prompt lives.

## What this decides

Decided 2026-11-08:

1. **rig is the harness we build against.** `factotum` is not a competitor; it is the multi-client
   host that will run `rig acp` as one of its agents, once its v1 path exists.
2. **The playbook is enforced, not only stated.** [`integrations/rig`](../../integrations/rig)
   registers the `at-*` verbs as native tools and refuses shell reads at Gate, naming the tool. The
   rule stays in `AGENTS.md` as the human-readable copy.
3. **The effect log is the event stream**, and DuckDB is the first reader. OTel is a projection to
   add when a consumer outside this machine needs the data.

This was written by cloning the repository and reading the kernel, not the README.

## The three layers

| Layer | Crate | What it owns |
| --- | --- | --- |
| Library | `crates/rig-core` | providers, models, tools, conversations, embeddings, vector stores |
| Kernel | `crates/rig-ecs` | an agent is an entity; a running turn is `TurnOf` its agent; every model and tool call is `CallOf` the turn |
| Harness | `crates/rig-harness` | the session, the launcher protocol, and the services every plugin shares |
| Agent | `crates/rig-cli` (`rig`), `plugins/rig-*` | the binary, and everything else — tools, commands, subagents, compaction, sign-in, the terminal view |

The launcher generates a small Cargo project from `RIG_HOME/settings.toml`, builds the agent with
exactly the plugins switched on, and runs it. `/reload` rebuilds that project and restarts in the
same session. A plugin is a Bevy plugin in a crate that depends on `rig-harness`; a third-party
plugin is the same kind, loaded through a `[[plugins.extra]]` table.

## The kernel idea

From `crates/rig-ecs/README.md`:

> every step of an agent's work is an effect entity. Each frame the `AgentPass` schedule runs its
> stages (Arrive, Review, Commit, Admit, Prepare, Gate, Start) until nothing moves, and a plugin
> steps in with an ordinary system in a stage.

The three consequences that matter to us, each verified in `crates/rig-harness/PLUGINS.md`:

- **A tool call can be refused with a reason the model reads.** Gate answers `Outcome::refused(why)`
  (`PLUGINS.md:542`). This is the sentence the whole playbook argument turns on: a rule can be a
  gate that says no, in-band, rather than prose the model may ignore.
- **A call can be run another way.** Prepare can edit the arguments or replace the `CallHandler`
  (`PLUGINS.md:536`), which is how `preview` runs a tool against an overlay of the workspace
  (`PLUGINS.md:559`).
- **A turn is data.** A turn, its calls, its holds, its queued messages and every decision are
  entities, recorded through one path. The effect log (`EffectLogPlugin` → `effects.jsonl`) is a
  projection of that path, not a transcript scraped later.

Tool arguments are typed too: `args_schema` derives a tool's parameters from its arguments type,
and a call whose arguments do not fit is refused with where and why (`PLUGINS.md:389-392`). A
plugin tool therefore has no hand-written JSON Schema to drift from the code. That is why the verbs
are native tools in [`integrations/rig`](../../integrations/rig) rather than a second front:
`tools/at-mcp` was written, then moved to `.tmp/`, because MCP is the seam for harnesses that are
not rig.

## Why that is different from what we have

| Today | With the kernel |
| --- | --- |
| A rule is prose in `AGENTS.md`; compliance is observed after the fact | A gate refuses a `shell` read and the model reads why, in the same turn |
| Permission fan-out is built on top of ACP v2 (`factotum`) | First-wins `Decision` and `HoldOf` are kernel concepts; approval is a default plugin on them (`NeedsApproval`) |
| Mutation testing is a soak around the client | A plugin can observe every effect before and after it runs |
| A tool is a CLI plus a second grammar in `at-mcp` | A tool is a Rust type; its schema is its argument type |
| Iterating on the agent means rebuilding an agent binary | `/reload` rebuilds a generated crate and restarts in the session |
| Harness behaviour is fixed unless you fork it | It is a plugin you can turn off or replace |

The last two are what make this an evaluation of *ways of working* rather than of a library.

## What we verified, and what we did not

Verified 2026-11-08, from the clone unless a source is named:

| Claim | Where |
| --- | --- |
| The agent is all plugins, on or off in `settings.toml`, and `/reload` rebuilds it | `crates/rig-harness/README.md:11-34`, `docs/agent.md` |
| A gate can refuse a call with a reason the model reads | `crates/rig-harness/PLUGINS.md:542` |
| Prepare can replace a call's handler; `preview` runs one on an overlay | `PLUGINS.md:536`, `PLUGINS.md:559` |
| Tool schemas derive from the arguments type and are validated by the kernel | `PLUGINS.md:389-392` |
| `AgentsMdPlugin` reads `AGENTS.md` from the working directory up to the project root, nearest last, after `RIG_HOME/AGENTS.md`; `CLAUDE.md` is not read | `docs/agent.md` |
| `SkillsPlugin` reads `.rig/skills`, `.agents/skills`, `RIG_HOME/skills`, `~/.agents/skills` | `docs/agent.md` |
| **The playbook's `generic` target already lands**: it writes `AGENTS.md` and `.agents/skills` | `models/generic/overlay.conf` |
| MCP works in both fronts: `$RIG_HOME/mcp.json` and the project's `.rig/mcp.json`, and editor servers forwarded over ACP | `crates/rig-harness/README.md:78`, `docs/agent.md` |
| `rig acp` serves **ACP v1**; a v2 client gets the v1 fallback; each session runs in its own process | `plugins/rig-acp/README.md` |
| The library emits GenAI semconv spans (`rig::telemetry`) and has typed observations (`rig::observe`) | docs.rs, rig 0.46.0 |
| Every settled effect is an `EffectRecord` with `id`, `parent`, `scope`, `turn`, `purpose`, `kind`, `outcome`, `decided_by` and three timestamps | `crates/rig-core/src/effect/mod.rs:824-903`, `PLUGINS.md:1451-1460` |
| The harness exposes only a `trace` feature (Bevy system spans); no OTel exporter plugin is documented | `crates/rig-harness/Cargo.toml` |
| Session data: `sessions/<id>/logs/<agent-id>.jsonl`, `files/effects.jsonl` (opt-in), `traffic.jsonl` (opt-in), `spill/` | `docs/agent.md` |
| Headless drive: `rig -p --json`, `rig --json`, `rig help-agents` | `docs/agent.md` |
| `rig-inspect` reads the agent's own Bevy world, read-only, off by default | `plugins/rig-inspect/README.md` |
| The built-in coding tools are `read`, `edit`, `write`, `shell`; `search`, `list`, `delete` are left to `shell` | `crates/rig-harness/README.md:54` |
| An extra plugin crate builds into a generated agent, with no stage conflicts | `rig plugin check --build`, 2026-11-09, 26 plugins |
| A tool's typed result can be published and read back from the effect log | `tool_output."agent_playbook.search"` in a real session, 2026-11-09 |
| A gate refusal reaches the model in-band, naming the replacement | a forced `shell rg` session on `deepseek/deepseek-flash`, 2026-11-09 |
| The redirect is deterministic and adds no model call | `integrations/rig/tests/redirect.rs`, 2026-11-09 |
| The plugin and the agent build from one local rig checkout | `~/code/defrag/rig` at `c8b20d3d7`, `rig plugin check --build` |

Not verified, and stated as such:

- **The install path.** The docs say `cargo install rig-cli`; the crates.io API answered 404 for
  that name on 2026-11-08 while `rig-core` is 0.46.0. The repository's own fallback is
  `cargo install --path crates/rig-cli` or `RIG_SOURCE`.
- **Whether a gate that refuses produces fewer turns than prose.** Measured with the tools present
  and only the gate switched off (below): the gate cost one model call and one extra refusal, and
  produced the bounded answer instead of raw `rg` lines. A prose-only baseline is still unmeasured.
- **What `/reload` costs** in wall time on this machine. The guide says only the generated crate
  recompiles; the first build of a plugin crate is the slow part.
- **Whether `rig acp` behind `factotum` is a small change.** The daemon is v2-only
  (`crates/factotum/src/agent.rs` sends `ProtocolVersion::V2` and refuses an agent without the v2
  session capability), so it needs a v1 client path before it can spawn `rig acp`. That is recorded
  in `factotum/docs/design/agent.md`, not here.

## The enforcement measurement

2026-11-09, `deepseek/deepseek-flash`, the same prompt against the same repository and the same
tool set. The only difference: `rig_playbook::ToolsOnlyPlugin` registers the tools and no Gate;
`rig_playbook::GatedPlugin` adds the Gate. The prompt was adversarial on purpose — *"Use the shell
tool to run exactly this command and report what it prints: `rg -n conservative-package-changes .`"*
— because the question is what happens when the model would otherwise bypass the toolkit. (The gate
is now `GatedPlugin`; `PlaybookPlugin` is the daily driver, with the shell policy below.)

| | Baseline, no gate | Gated, refused | Gated, redirected |
| --- | --- | --- | --- |
| Turns | 1 | 1 | 1 |
| Model calls | 2 | 3 | **2** |
| Tool calls | 1 `shell`, ran | 2: `shell` refused, `at_peek_search` ran | 1 `shell`, served by `at_peek_search` |
| Decider | — | `{"plugin":"playbook"}`, `Err{denied, refusal}` | — (a `Prepare` substitution) |
| Answer | four raw `rg` lines | bounded: 4 matches, 443 walked, the skips named | the same bounded answer |

Four readings worth keeping:

- **A refusal costs one model call; a redirect costs none.** The refusal needs the model to read it
  and ask again. `Prepare` replaces the handler instead, so the model's one call returns the
  bounded answer and the run matches the ungated baseline exactly.
- **Prose lost and the gate did not.** The baseline model *knew* the skill prefers `at_peek_search`
  and said so — then ran `rg` anyway because the user asked for it ("I ran the literal command as
  you asked"). The gated model could not.
- **The substitution is recorded.** A redirected call is `tool:shell` in the log, with
  `agent_playbook.redirection` naming the command and the tool that answered it, beside
  `agent_playbook.search`. It has no `decided_by`, because nothing was refused.
- **What the replacement bought is the answer.** `rg` prints four lines and cannot say how many
  files it walked or what it skipped. `at_peek_search` states the true total, the walk, the skipped
  directories, one secret-shaped file and one symlink. That is the difference between a result and
  a bounded one.

`turn` is a poor discriminator here: rig counts one turn however many calls it contains. Model
calls and tool calls are the quantities, and both are in the effect log.

The final pair used `kill-only-what-you-started` rather than the original pattern, because this
document quoting the original made the search return the document and the model read it — a
benchmark corpus must not contain its own prompts.

The next measurement — scenarios drawn from 143 real Zed threads, and the three ways to drive rig
through them — is [`rig-eval.md`](rig-eval.md).

## What rig does not do

- **No multi-client session.** One session per process, one editor connection, and a rig session
  cannot be open in two places at once. `factotum` exists for the opposite premise; the two compose
  at a process boundary, and neither replaces the other.
- **No LSP, diagnostics, references or rename** in its built-in tools. Against Zed's native agent
  that is a real downgrade in coding capability, however much better the architecture is.
- **No OTEL exporter for the agent.** `rig::telemetry` covers the library; the harness has `trace`.
  For local analysis the effect log makes one unnecessary, and an exporter is a projection to add
  only if a consumer outside this machine needs the data.
- **Not stable.** Pre-1.0, 67 releases, breaking changes each time; Bevy is pinned exactly (0.20);
  Rust 1.97.1 or newer; Linux and macOS only.
- **Not a store.** Nothing here replaces HawDB or answers the trace-store question.

## The seams

| Seam | What it gives | What it costs |
| --- | --- | --- |
| `AGENTS.md` + `.agents/skills` | the playbook works today, no new target | read once; an edit needs `/reload` or `/new` |
| MCP (`mcp.json`, ACP forwarding) | one adapter for rig, Zed and Claude | a process per session, and no kernel-side typing |
| A plugin | typed tools, gates, effects, `/reload` iteration | Bevy + `rig-harness` pinned; a crate outside this workspace |
| ACP | Zed's panel, threads, MCP forwarding | v1 only; no editor filesystem or terminal |
| Effect log / cassette | replay and analysis without instrumentation | opt-in, and a log format to learn |
| `-p --json` | headless runs and tests | the JSON protocol, which is documented by `rig help-agents` |

## Observation: the effect log is canonical, DuckDB first

Decided 2026-11-08. The effect log is the event stream; DuckDB is the first way we read it. OTel is
deferred rather than rejected: it is a projection to add when a consumer needs it, not the schema
to design around.

The reason is that `EffectRecord` already contains what a span would carry, plus one field a span
does not. From `crates/rig-core/src/effect/mod.rs:824-903` — the field names are the JSON keys,
because the struct has no `rename_all` (`published` is the one rename, to `tool_output`):

| JSON key | What it holds |
| --- | --- |
| `id` | the dispatch's id |
| `parent` | the effect it was made under — the parent link a trace needs; absent for a root |
| `scope` | the agent's id, so one trace per agent |
| `turn`, `purpose` | the turn number, and what it was for (`turn`, `compaction`, …) |
| `kind` | internally tagged on `effect`: `completion`, `tool_call`, `embed`, `rerank`, `retrieve`, `custom`. A tool call carries `name` and the model's raw `args` |
| `outcome`, `reviewed` | serde `Result`: `{"Ok": …}` or `{"Err": …}`, and a reviewer's replacement |
| `disposition` | `ran`, `answered_before_start`, `answered_while_running`, `superseded`, `cancelled` |
| `decided_by` | absent, `"person"`, or `{"plugin":"name"}` — **which gate decided it**. OTel has no attribute for this |
| `started_ms`, `first_item_ms`, `finished_ms` | dispatch, first streamed item (time to first token), outcome |
| `tool_output` | the facts a call published |

`EffectLogPlugin` (off by default) appends these to
`RIG_HOME/sessions/<id>/files/effects.jsonl`. The session log at `logs/<agent-id>.jsonl` carries
usage and compactions; a cost question is a join of the two, not a third source.

**The queries, run.** DuckDB is in the devshell and the queries were checked against real sessions on
2026-11-09. The maintained versions are in
[`skills/rig-session-query`](../../skills/rig-session-query/SKILL.md). The one thing to know here is
that a session log is **not one effect per line**: `header` lines and delta records interleave, and
a query keyed on `kind.effect` alone undercounts completions. The skill's coalescing view handles
both.

A deeper question — the call tree under one turn — is a recursive CTE over `id`/`parent`, which is
worth writing only once there is a session where the tree is not obviously flat.

**What would put OTel back.** A single view across rig, Zed's agent, `factotum` and the workers.
That is the one thing the log cannot do locally, because it is one process's file. Even then the
cheapest first step is a sidecar: an OpenTelemetry Collector's `filelog` receiver over
`effects.jsonl` needs no SDK and no plugin, and the agent stays as it is.

## What we are building

1. **`rig-playbook`** — built in [`integrations/rig`](../../integrations/rig). Seven `at-*` verbs
   are native tools: typed arguments, schema derived from the type, a `Reads` footprint, and the
   agent's own directory as the root. Every tool calls its verb's `collect` in process and publishes
the typed answer, so there is no `PATH` or subprocess dependency; [`toolkit-core.md`](toolkit-core.md)
   records that seam. `PlaybookPlugin` is the daily driver: a plain `rg`/`grep` shell call is
   **redirected** in `Prepare` to the in-process search, and every other shell command runs through
   the classifier, which permits reads and builds and refuses writes. `GatedPlugin` keeps the
   earlier read-refusal policy for the measurement above. The rule engine follows on the same seam.
2. **An observation sink.** The procedure is written and verified
   ([`skills/rig-session-query`](../../skills/rig-session-query/SKILL.md)); it read the 2026-11-09
   sessions, including the delta records the first draft missed. DuckDB first; HawDB when the
   question needs joins; OTel only when a consumer outside this machine needs the data.
3. **`factotum` ↔ rig.** Keep the daemon; add a v1 agent path; run `rig acp` as one of its agents,
   beside Zed's. The daemon is the only piece that does multi-client replay and fan-out, and rig is
   the only piece whose behaviour is plugins. Neither substitutes for the other.

`tools/at-mcp` stays useful for the harnesses that are not rig (Zed's agent, Claude), but the
plugin is the surface that gets the kernel's typing and gating. That is the direction to evaluate.

## The experiments, in order

| # | Experiment | Answers | Size |
| --- | --- | --- | --- |
| 1 | `rig plugin enable rig_inspect::InspectPlugin`, then ask the agent what plugins it loaded and what each added | whether the API is learnable from the running world | minutes, no code |
| 2 | **Built** in `integrations/rig`. Load it and run a session | does the model retry with the tool, and in how many turns | one run |
| 3 | Export `effects.jsonl` into DuckDB and answer one real question from it (tool cost, gates, or TTFT) | whether the log is enough without an instrumentation layer | half a day, once one session exists |
| 4 | `rig -p --json` driven from a script, then `rig-cassette` replay | whether a rig flow is testable | half a day |

Experiment 2 is settled: a redirect expresses "serve this read with the tool" at the baseline's
cost, and a refusal costs one model call. Experiment 3 is done. Experiment 4 has run: the runner
and its first result are in [`rig-eval.md`](rig-eval.md). The result is negative on cost — the gate
is bypassed by a `cd &&` prefix and by the ungated `read` tool — and names what to change next.

## Reading map

In order, and only what is needed:

| Read | For |
| --- | --- |
| `crates/rig-harness/README.md:36-82` | the plugin inventory: what exists, and whether it is on by default |
| `crates/rig-harness/PLUGINS.md:46-121` | the glossary — effects, stages, holds, decisions, layers, footprints, facts |
| `crates/rig-harness/PLUGINS.md:21-44` | the task → section table; jump only to what a task needs |
| `crates/rig-ecs/README.md` | the kernel in 34 lines |
| `corpus/shared`, then `corpus/base` | idiomatic plugins written against the public API, built and tested by CI |
| `corpus/base/src/shell_rewrite.rs`, `effect_query.rs`, `output_truncate.rs` | policy at `Prepare`/`Gate`/`Review`, live effect queries, and bounding a tool result |
| `corpus/base/src/session_replay.rs`, `http_api.rs`, `window_gui.rs` | replay, and the two ways to put a front on the agent |
| `test-support/rig-harness-test-support` | `app_in`, `connect`, `run_until`, collectors, scripted replies: the deterministic harness |
| `test-support/rig-defaults-test-support` | the default plugin set, and `assert_stages_ordered` |
| `docs/agent.md` | the agent as a user meets it: sessions, tools, subagents, `--json`, `RIG_HOME` |
| `AGENTS.md` | rig's own engineering conventions, and a good model for an agent-facing repository |
| `plugins/rig-basics/src/telemetry.rs`, `telemetry/` | `UsagePlugin`, `EffectLogPlugin`, `DiagnosticsPlugin` — the observability seam |

`corpus/` is worth knowing about beyond examples: it is a set of plugin crates (`base`, `shared`,
`rf-01`…`rf-08`, `ho`, `oc`, `sn`, `sp`, `ct`, `fh`, `library`) written specifically to prove the
kernel through its public API, and CI builds every one. It is the reason the plugin API can be
trusted, and the best source of idiom.

## Open questions

1. Does the redirect reach a real call? It is implemented for a plain `rg`/`grep`, and on a forced
   shell read it matches the baseline exactly. On a natural prompt it is rarely the first program
   of the command: the model prefixes `cd <repo> &&`, which the Gate does not see. Measured in
   [`rig-eval.md`](rig-eval.md), and the largest thing to fix before the next run.
2. Does `/reload` after a rule change fit inside a working session, or is it a coffee break?
3. Does `rig` beat Zed's native agent on coding tasks despite having no LSP — or is it a harness
   we drive in Zed over ACP and never use as a coder?
4. Does the JSONL as written match the record's serde shape — `outcome` as `Ok`/`Err`, `kind` tagged
   on `effect` — and does `EffectLogPlugin` keep timings on by default? Both are read from one real
   log before any query is trusted.
5. Does the playbook need a `models/rig/` target at all, given `generic` already lands, or only a
   small addendum naming `/reload` and the gate behaviour?

## What would make us change course

- A gate that cannot refuse **and** name the replacement, so compliance still costs a retry.
- `/reload` measured in minutes, which would break the edit-rule-and-continue loop.
- The pre-1.0 churn landing on the plugin API faster than we can hold a pin.
- The tool surface making it a materially worse coder than the agent we already use — in which case
  the plugin still stands, but rig is driven through Zed over ACP rather than used as a coder.
