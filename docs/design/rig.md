# rig

The plan for building against **rig**, and the evidence behind it. Written 2026-11-08 against
`0xPlaygrounds/rig` at `63431d79b` (the `main` of that day, 1728 commits), with `rig-core` 0.46.0
published 2026-10-31. The checkout used for every claim below is `~/code/github/rig`; it sits
outside every workspace root, so it is read through `at-peek --root` and `at-recall --root`.

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

Not verified, and stated as such:

- **The install path.** The docs say `cargo install rig-cli`; the crates.io API answered 404 for
  that name on 2026-11-08 while `rig-core` is 0.46.0. The repository's own fallback is
  `cargo install --path crates/rig-cli` or `RIG_SOURCE`.
- **Whether a gate that refuses produces fewer turns than prose.** It should, because the refusal
  is in-band; that is hypothesis 2 below, not a measurement.
- **What `/reload` costs** in wall time on this machine. The guide says only the generated crate
  recompiles; the first build of a plugin crate is the slow part.
- **Whether `rig acp` behind `factotum` is a small change.** The daemon is v2-only
  (`crates/factotum/src/agent.rs` sends `ProtocolVersion::V2` and refuses an agent without the v2
  session capability), so it needs a v1 client path before it can spawn `rig acp`. That is recorded
  in `factotum/docs/design/agent.md`, not here.

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

**The queries, unrun.** DuckDB is not installed in the devshell and no rig session has been recorded
yet, so these are written against verified field names and nothing more. They are the first thing to
run against a real log, and the first one that fails is information about the wire shape, not about
the decision.

```sql
-- What each tool costs, in calls and wall time.
SELECT json_extract_string(kind, '$.name')                  AS tool,
       count(*)                                             AS calls,
       round(quantile_cont(finished_ms - started_ms, 0.5))  AS p50_ms,
       round(quantile_cont(finished_ms - started_ms, 0.95)) AS p95_ms,
       count(*) FILTER (WHERE json_extract(outcome, '$.Err') IS NOT NULL) AS errors
FROM read_json_auto('effects.jsonl')
WHERE json_extract_string(kind, '$.effect') = 'tool_call'
GROUP BY 1 ORDER BY calls DESC;

-- A turn in one row: model calls, tool calls, gates, wall time.
SELECT scope, turn,
       count(*) FILTER (WHERE json_extract_string(kind, '$.effect') = 'completion') AS model_calls,
       count(*) FILTER (WHERE json_extract_string(kind, '$.effect') = 'tool_call')   AS tool_calls,
       count(*) FILTER (WHERE decided_by IS NOT NULL)                               AS gated,
       max(finished_ms) - min(started_ms)                                           AS wall_ms
FROM read_json_auto('effects.jsonl')
GROUP BY 1, 2 ORDER BY 1, 2;

-- Who said no, and to what. The playbook's enforcement, measured.
SELECT coalesce(json_extract_string(decided_by, '$.plugin'),
                json_extract_string(decided_by))         AS decider,
       json_extract_string(kind, '$.name')               AS tool,
       count(*)                                          AS decided
FROM read_json_auto('effects.jsonl')
WHERE decided_by IS NOT NULL
GROUP BY 1, 2 ORDER BY decided DESC;

-- Time to first token, beside total time.
SELECT quantile_cont(first_item_ms - started_ms, 0.5)   AS ttft_p50_ms,
       quantile_cont(finished_ms - started_ms, 0.5)     AS total_p50_ms
FROM read_json_auto('effects.jsonl')
WHERE first_item_ms IS NOT NULL;
```

A deeper question — the call tree under one turn — is a recursive CTE over `id`/`parent`, which is
worth writing only once there is a session where the tree is not obviously flat.

**What would put OTel back.** A single view across rig, Zed's agent, `factotum` and the workers.
That is the one thing the log cannot do locally, because it is one process's file. Even then the
cheapest first step is a sidecar: an OpenTelemetry Collector's `filelog` receiver over
`effects.jsonl` needs no SDK and no plugin, and the agent stays as it is.

## What we are building

1. **`rig-playbook`** — built in [`integrations/rig`](../../integrations/rig). Seven `at-*` verbs
   are native tools: typed arguments, schema derived from the type, a `Reads` footprint, and the
   agent's own directory as the root. A Gate refuses a `shell` read and names the tool. The tools
   and the plugin stop crossing a process boundary first — that seam is
   [`toolkit-core.md`](toolkit-core.md) — and the rule engine follows on it.
2. **An observation sink.** The procedure is written
   ([`skills/rig-session-query`](../../skills/rig-session-query/SKILL.md)); the first run over a
   real `effects.jsonl` is next. DuckDB first; HawDB when the question needs joins; OTel only when
   a consumer outside this machine needs the data.
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

Experiment 2 is the one that matters. If a gate cannot express "refuse and redirect" without the
model retrying — and that is unverified — the playbook stays instructions, and the case weakens
considerably.

## Reading map

In order, and only what is needed:

| Read | For |
| --- | --- |
| `crates/rig-harness/README.md:36-82` | the plugin inventory: what exists, and whether it is on by default |
| `crates/rig-harness/PLUGINS.md:46-121` | the glossary — effects, stages, holds, decisions, layers, footprints, facts |
| `crates/rig-harness/PLUGINS.md:21-44` | the task → section table; jump only to what a task needs |
| `crates/rig-ecs/README.md` | the kernel in 34 lines |
| `corpus/shared`, then `corpus/base` | idiomatic plugins written against the public API, built and tested by CI |
| `docs/agent.md` | the agent as a user meets it: sessions, tools, subagents, `--json`, `RIG_HOME` |
| `AGENTS.md` | rig's own engineering conventions, and a good model for an agent-facing repository |
| `plugins/rig-basics/src/telemetry.rs`, `telemetry/` | `UsagePlugin`, `EffectLogPlugin`, `DiagnosticsPlugin` — the observability seam |

`corpus/` is worth knowing about beyond examples: it is a set of plugin crates (`base`, `shared`,
`rf-01`…`rf-08`, `ho`, `oc`, `sn`, `sp`, `ct`, `fh`, `library`) written specifically to prove the
kernel through its public API, and CI builds every one. It is the reason the plugin API can be
trusted, and the best source of idiom.

## Open questions

1. Can a gate redirect, or only refuse? If only refuse, how many turns does compliance cost?
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
