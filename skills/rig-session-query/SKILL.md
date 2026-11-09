---
name: rig-session-query
description: Use when you need to know what a rig agent session actually did — which tools it called, how long each took, which gate refused what, or how many turns a task took — by recording rig's effect log and querying it with DuckDB. Covers enabling the log, finding the session, checking the wire shape (including header and delta lines), and the queries for the usual questions.
---

# Querying a rig session with DuckDB

The situation that should have brought you here: there is a rig run and a question about its
behaviour that the transcript cannot answer — which tool was slow, whether a gate refused anything,
how many turns it took before the agent read the right file. rig records every settled effect; this
is how to read that record.

The design record behind this, including what each field means and why DuckDB, is
[`docs/design/rig.md`](../../docs/design/rig.md).

## 1. Turn the log on

```sh
rig plugin enable rig_basics::telemetry::effect_log::EffectLogPlugin
```

`/plugins` inside the agent does the same and restarts at once; `/reload` is not needed for a
plugin-only change at the next start.

It is **off by default**. A session recorded before this has no `effects.jsonl`, and an empty query
result is not evidence that nothing happened.

## 2. Record a session

```sh
rig                              # interactive; do the thing you want to measure
rig -p "fix the failing test"    # headless, one turn, exits
```

Everything lives under `RIG_HOME` (default `~/.rig`). The session's log is:

```sh
~/.rig/sessions/<session-id>/files/effects.jsonl
```

The newest session is the newest directory:

```sh
ls -t ~/.rig/sessions | head -1
```

`logs/<agent-id>.jsonl` in the same session holds messages, usage and compactions. Cost questions
join the two; nothing else is a third source.

## 3. Check the wire shape before trusting a query

`EffectRecord` names the fields; the file decides how they are written, and a session log is **not
one effect per line**:

- `{"header": …}` lines describe the handlers in play. They carry no `id` and no effect.
- `{"id":…, "after":…, "keep":…, "then":…, "same_tools":…, "record":{…}}` is a **delta**: the
  effect's fields are under `record`, not at the top. A completion the log can express as a change
  from an earlier one is written this way, so `WHERE kind.effect = 'completion'` misses it.

The one query that shows every shape:

```sh
duckdb -c "WITH t AS (SELECT unnest(string_split(content, chr(10))) AS l FROM read_text('…/effects.jsonl')) SELECT row_number() OVER () AS n, json_keys(l) FROM t WHERE length(l) > 0;"
```

Then, in the records themselves:

- `kind` is internally tagged: `{"effect":"tool_call","name":"shell","args":"…"}`.
- `outcome` is a serde `Result`: `{"Ok": …}` or `{"Err": …}`. DuckDB normalises the struct keys to
  `ok`/`err`, so the check is `outcome.err IS NOT NULL`, not `json_extract(…, '$.Err')`.
- `published` is renamed on the wire to `tool_output`.

If a field is missing from the first rows, pass `union_by_name = true` (below). Without it, DuckDB
infers from the first line — a header — and turns the rest into NULL rows.

## 4. One view, then the questions

```sql
-- Run once per session, after `USE`/opening the file's directory.
CREATE TEMP VIEW log AS
SELECT id,
       coalesce(kind.effect, record.kind.effect)     AS effect,
       kind.name                                     AS tool,
       coalesce(key, record.key)                     AS handler,
       coalesce(purpose, record.purpose)             AS purpose,
       coalesce(turn, record.turn)                   AS turn,
       coalesce(scope, record.scope)                 AS scope,
       coalesce(started_ms, record.started_ms)       AS started_ms,
       coalesce(first_item_ms, record.first_item_ms) AS first_item_ms,
       coalesce(finished_ms, record.finished_ms)     AS finished_ms,
       tool_output, decided_by, outcome
FROM read_json_auto('…/effects.jsonl', union_by_name = true)
WHERE id IS NOT NULL;
```

`effect` is `completion`, `tool_call`, `embed`, `rerank`, `retrieve` or `custom`; `handler` is the
concrete thing that served it (`model:…`, `tool:…`, `message`, `exit`). `tool` is the model-facing
name and is NULL for everything but a tool call.

```sql
-- What each tool costs, in calls and wall time.
SELECT tool, count(*)                                             AS calls,
       round(quantile_cont(finished_ms - started_ms, 0.5))        AS p50_ms,
       round(quantile_cont(finished_ms - started_ms, 0.95))       AS p95_ms,
       count(*) FILTER (WHERE outcome.err IS NOT NULL)            AS errors
FROM log
WHERE effect = 'tool_call'
GROUP BY 1 ORDER BY calls DESC;

-- A turn in one row: model calls, tool calls, gates, wall time.
SELECT scope, turn,
       count(*) FILTER (WHERE effect = 'completion') AS model_calls,
       count(*) FILTER (WHERE effect = 'tool_call')  AS tool_calls,
       count(*) FILTER (WHERE decided_by IS NOT NULL) AS gated,
       max(finished_ms) - min(started_ms)             AS wall_ms
FROM log
WHERE turn IS NOT NULL
GROUP BY 1, 2 ORDER BY 1, 2;

-- Who said no, and to what. This is enforcement, measured.
SELECT coalesce(json_extract_string(to_json(decided_by), '$.plugin'), 'person') AS decider,
       tool, count(*) AS decided
FROM log
WHERE decided_by IS NOT NULL
GROUP BY 1, 2 ORDER BY decided DESC;

-- Time to first token, beside total time.
SELECT quantile_cont(first_item_ms - started_ms, 0.5) AS ttft_p50_ms,
       quantile_cont(finished_ms - started_ms, 0.5)   AS total_p50_ms
FROM log
WHERE first_item_ms IS NOT NULL;

-- How many turns the task took.
SELECT count(DISTINCT turn) AS turns FROM log WHERE turn IS NOT NULL;

-- What a plugin's tool published with the call. This key is the playbook plugin's; substitute
-- the key the plugin declares.
SELECT tool_output."agent_playbook.search".pattern          AS pattern,
       tool_output."agent_playbook.search".total            AS total,
       tool_output."agent_playbook.search".walk.considered  AS walked,
       len(tool_output."agent_playbook.search".counted)     AS files
FROM log
WHERE tool_output."agent_playbook.search" IS NOT NULL;
```

The call tree under one turn is a recursive CTE over `id`/`parent`; write it only when the tree is
not obviously flat.

## 5. Reading the answers

- **`decided_by`** is the gate that answered an effect before it ran: `"person"` or
  `{"plugin":"name"}`. An allow is not a decision and leaves it absent, so this counts refusals and
  gate-supplied answers, not approvals. A refusal also sets `outcome.err.kind = 'denied'` and
  `outcome.err.refusal = true`.
- **`first_item_ms`** is time to first token, but only for a streamed dispatch that produced an
  item. It is `null` for unary calls, and for a recorder that keeps no times.
- **`scope`** is the agent id, so a subagent's calls appear under their own scope; `parent` links
  them back to the call that spawned them.
- **`handler` is the stable name.** `tool` is what the model called; `handler` is `tool:<name>`, so
  a rename in a plugin's schema is visible without guessing from `kind`.

**Verified 2026-11-09** against rig 0.46.0, two headless sessions on `agent-playbook`:

| Run | Turns | Completions | Tool calls | Gates |
| --- | --- | --- | --- | --- |
| natural search task | 1 | 2 | 1 `at_peek_search` | 0 |
| forced `shell rg` | 1 | 2 | 1 `shell`, refused | 1, `{"plugin":"playbook"}` |

The first run's published facts read back as `total = 4`, `walked = 443`, `files = 4`. The second
run's refusal is `outcome.err.kind = 'denied'`, `refusal = true`, and the model read the message and
offered `at_peek_search` rather than retrying the shell.

## Traps

- **An empty log is not a quiet session.** The plugin is off by default; check it before drawing a
  conclusion.
- **A delta is not a header.** Both lack `kind`; only the delta has an `id` and a nested `record`.
  Filtering `id IS NOT NULL` keeps the delta and drops the header, which is why the view above
  starts there.
- **A naive `kind` query undercounts completions.** In the verified run, one of the two completions
  was a delta. Always coalesce with `record.kind`.
- **`read_json_auto` infers from the first line.** That line is a header, so pass
  `union_by_name = true` or most records become NULL.
- **Timings can be absent.** `started_ms`, `first_item_ms` and `finished_ms` are `null` when the
  recorder keeps no times. A p95 over a partial column is a p95 over the subset that had times.
- **A crash can leave a torn last line.** It is not a schema change; drop the line and read the
  rest, and say you did.
- **`RedactPlugin` changes what is in the log**, because records are redacted before they are
  written. A secret-shaped argument may appear as a placeholder.
- **DuckDB is in this repository's devshell.** Outside it, `nix shell nixpkgs#duckdb` gives the
  same CLI; there is no need to install anything else.
