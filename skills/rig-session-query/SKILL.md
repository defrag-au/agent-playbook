---
name: rig-session-query
description: Use when you need to know what a rig agent session actually did — which tools it called, how long each took, which gate refused what, or how many turns a task took — by recording rig's effect log and querying it with DuckDB. Covers enabling the log, finding the session, checking the wire shape, and the queries for the usual questions.
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

Field names come from `EffectRecord` (`crates/rig-core/src/effect/mod.rs:824`), but the file is the
authority:

```sh
duckdb -c "DESCRIBE SELECT * FROM read_json_auto('/Users/you/.rig/sessions/<id>/files/effects.jsonl')"
duckdb -c "SELECT kind, outcome, decided_by FROM read_json_auto('…/effects.jsonl') LIMIT 1"
```

Three shapes surprise people, and all three are visible in that one line:

- `kind` is internally tagged: `{"effect":"tool_call","name":"shell","args":"…"}`.
- `outcome` is a serde `Result`: `{"Ok": …}` or `{"Err": …}`.
- `published` is renamed on the wire to `tool_output`.

If a field is missing from the first rows, DuckDB's inference can miss it for the whole file. Pass
an explicit `columns` map to `read_json` rather than guessing.

## 4. The four questions worth asking

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

-- Who said no, and to what. This is enforcement, measured.
SELECT coalesce(json_extract_string(decided_by, '$.plugin'),
                json_extract_string(decided_by)) AS decider,
       json_extract_string(kind, '$.name')       AS tool,
       count(*)                                  AS decided
FROM read_json_auto('effects.jsonl')
WHERE decided_by IS NOT NULL
GROUP BY 1, 2 ORDER BY decided DESC;

-- Time to first token, beside total time.
SELECT quantile_cont(first_item_ms - started_ms, 0.5) AS ttft_p50_ms,
       quantile_cont(finished_ms - started_ms, 0.5)   AS total_p50_ms
FROM read_json_auto('effects.jsonl')
WHERE first_item_ms IS NOT NULL;
```

The call tree under one turn is a recursive CTE over `id`/`parent`; write it only when the tree is
not obviously flat.

## 5. Reading the answers

- **`decided_by`** is the gate that answered an effect before it ran: `"person"` or
  `{"plugin":"name"}`. An allow is not a decision and leaves it absent, so this counts refusals and
  gate-supplied answers, not approvals.
- **`disposition`** says how: `ran` (a handler served it), `answered_before_start`,
  `answered_while_running`, `superseded`, `cancelled`.
- **`first_item_ms`** is time to first token, but only for a streamed dispatch that produced an
  item. It is `null` for unary calls, and for a recorder that keeps no times.
- **`scope`** is the agent id, so a subagent's calls appear under their own scope; `parent` links
  them back to the call that spawned them.

## Traps

- **An empty log is not a quiet session.** The plugin is off by default; check it before drawing a
  conclusion.
- **Timings can be absent.** `started_ms`, `first_item_ms` and `finished_ms` are `null` when the
  recorder keeps no times. A p95 over a partial column is a p95 over the subset that had times.
- **A crash can leave a torn last line.** It is not a schema change; drop the line and read the
  rest, and say you did.
- **`RedactPlugin` changes what is in the log**, because records are redacted before they are
  written. A secret-shaped argument may appear as a placeholder.
- **DuckDB is in this repository's devshell.** Outside it, `nix shell nixpkgs#duckdb` gives the
  same CLI; there is no need to install anything else.
