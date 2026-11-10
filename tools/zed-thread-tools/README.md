# zed-thread-tools

Four commands over Zed's agent thread store:

| Binary | Reads | Writes |
| --- | --- | --- |
| `zed-thread-audit` | `threads.db`, read-only | Never |
| `zed-thread-export` | `threads.db`, read-only | A new snapshot directory |
| `zed-thread-tui` | both stores, read-only until an action is confirmed | Archive flag, one delete, one import, or one `Compaction(Summary)`; backup first |
| `zed-thread-recover` | one thread | One `Compaction(Summary)` message, plus a backup |

The store is `~/Library/Application Support/Zed/threads/threads.db` on macOS. Each
row is a zstd-compressed JSON document whose top-level shape is Zed's `DbThread`
plus a `version` field.

## Audit

```sh
zed-thread-audit                  # 20 largest threads
zed-thread-audit --top 0          # every thread
zed-thread-audit --json           # typed JSON report
zed-thread-audit --older-than 2026-10-01
zed-thread-audit --project compositor
```

The audit reports compressed and decoded size, message and tool-call counts,
tool-output bytes, the model, project paths, and how many messages have
accumulated since the last compaction. The largest tool output is attributed to
the tool that produced it and the message index it sits at (`Largest` and
`Top tool`), which is what turns "a big output happened" into "this tool caused
it". It never prints message text.

### Explain one thread

```sh
zed-thread-audit --explain <thread-id-or-title>
zed-thread-audit --explain "mithril snapshot" --limit 20
zed-thread-audit --explain <thread-id> --json
```

`--explain` lists a thread's tool results largest first, with the message index,
the tool, the byte count and the **request argument** (the URL, path or
command). It never reads or prints the result body. The thread can be named by
id — from the audit table — or by a unique substring of its title; an ambiguous
substring is an error rather than a guess. `--limit 0` shows every result.

This is the first command after a session has compacted unexpectedly: it names
the page or file that cost the context, not just the tool that fetched it.

### Behaviour

```sh
zed-thread-audit --behaviour <thread-id-or-title>
zed-thread-audit --behaviour "mithril snapshot" --limit 0 --json
```

`--behaviour` counts what the agent *did*, not what it cost. Four measures, each
with the message index and request argument behind it:

| Measure | Definition |
| --- | --- |
| Dead ends | results the tool reported as an error, and results that carried no text at all |
| Repetitions | a call whose `(tool, argument)` already appeared earlier in the thread |
| Habits | a terminal command piping into `head` or `tail`, or echoing a `===`/`---` separator |
| Exits | `# next:` lines a tool printed, whether a later call ran one as printed, and the call that came next |

A read repeated after a write to the same path is a re-read, not a re-derivation,
and is not counted. Only tools that carry a structured `path` can be paired this
way.

`--limit` truncates each site list; `0` lists every site. Counts are always
complete.

This verb reads result bodies, unlike `--explain`, because a followed exit exists
only in the text a tool printed. It never stores or prints a body: the report
carries counts, message indices and request arguments.

## TUI

```sh
zed-thread-tui                    # all threads, largest decoded first
zed-thread-tui --summary-dir .tmp --archive-dir ~/Documents/zed-thread-archives
```

`--zed-db` defaults to the active `db/<channel>/db.sqlite` under Zed's data
directory. `--archive-dir` defaults to `~/Documents/zed-thread-archives`.

The TUI is a unified library: `sidebar_threads` from `db.sqlite` joined to the
native audit in `threads.db`. ACP threads appear with metadata only. It never
reads or prints message text.

| Key | Action |
| --- | --- |
| `j` / `k`, arrows | move |
| `g` / `G` | first / last |
| `PgUp` / `PgDn` | page |
| `/` | filter by title, id, agent, model or project path |
| `s` | cycle sort: decoded, updated, messages, tool bytes |
| `v` | view all / active / archived |
| `a` | archive or unarchive the selected thread |
| `e` | export the selected thread to a bundle |
| `d` | delete the selected thread only |
| `i` | import a bundle from the archive directory |
| `R` | reload both stores |
| `r` | edit a summary and confirm a recovery |
| `q` | quit |
| `?` | help |

`r` writes a template to `--summary-dir` and opens `$EDITOR`/`$VISUAL` on it. Replace the
placeholder with the recovery summary, save, and the TUI shows a confirmation
screen with the insertion point. `y` applies it through the same
`write_recovery` path as the CLI: Zed must be quit, a `VACUUM INTO` backup is
made first, and an existing summary file is reused rather than overwritten.

Run the TUI from a terminal outside Zed when you intend to change either store;
quitting Zed also ends a terminal hosted inside it.

## Archive, export and delete

`a` flips `sidebar_threads.archived` in `db.sqlite`. It is reversible and does
not remove the transcript. Archive writes share one dated backup per day rather
than creating a 27 MB file per toggle.

`e` writes a bundle:

```
~/Documents/zed-thread-archives/2026-10-17T05-40-00Z-title-1a2b3c4d/
  manifest.json
  threads/<thread-id>.data
  threads/<subagent-id>.data
```

`manifest.json` is typed and versioned. The `.data` files are the exact stored
blobs, so an export/import round trip preserves unknown fields. A native thread
exports its whole subagent tree. ACP threads have only sidebar metadata in Zed,
so their bundle has no transcript.

`i` lists the bundles in the archive directory and imports one. Existing ids are
never overwritten; a conflict is an error, not a merge.

`d` deletes one thread, never a filtered set. For a native thread it removes the
`threads.db` tree and the matching `sidebar_threads` row; for an ACP thread it
removes the sidebar metadata only. Both databases get a timestamped backup
beside themselves first, and the action refuses while Zed is running.

## Snapshot for an external reader

`e` in the TUI writes raw `.data` blobs, which is what an import needs. A
visualiser needs decoded JSON instead:

```sh
zed-thread-export --out .tmp/zed-snapshot --max-mb 50
zed-thread-export --db <threads.db> --out <dir> --max-mb 0
```

The output is a new directory:

```
.tmp/zed-snapshot/
  README.md
  manifest.json
  threads/<thread-id>.json
```

Each thread file is the JSON Zed stored before compressing it, decoded without
rewriting; unknown fields are preserved. `manifest.json` records the source
database, the budget, one typed entry per exported thread (title, model,
message count, version, project paths, file) and every tree that was skipped.

Selection is newest `updated_at` first and by whole tree: a top-level thread and
all its subagents are included or left out together, so a `parent_id` in the
snapshot always resolves. A tree that would take the total past `--max-mb` is
skipped rather than truncated and listed under `skipped`; `--max-mb 0` removes
the budget. The store is opened read-only, and the output path must not already
exist.

The transcript is unredacted — prompts, tool output, absolute paths and file
contents — so a snapshot is as sensitive as the thread store itself.

## Recovery

A thread that has hit its context limit can be rescued without deleting
anything. Zed's `Message::Compaction(CompactionInfo::Summary)` is the message
`/compact` writes, and `extend_request_history_until` sends the latest summary
instead of the whole transcript. The recover command appends one of those:

```sh
# Write the summary as a file, then dry-run:
zed-thread-recover \
  --thread <id> \
  --summary-file .tmp/recovery-summary.md

# Quit Zed completely, then apply:
zed-thread-recover \
  --thread <id> \
  --summary-file .tmp/recovery-summary.md \
  --write
```

The summary is what the model sees as its compacted context. It should name the
task, the decisions made, the current state, and the next step. The command
refuses to insert before a trailing unanswered user message or `Resume`; those
stay after the summary so the next request still answers them.

Safety properties:

- `--write` is required; otherwise it is a dry run.
- Zed must be quit. The check probes both `pgrep -x Zed` and `pgrep -x zed`,
  because the macOS bundle's executable name has varied between versions;
  `--allow-running` overrides it, but Zed can overwrite the change.
- A `VACUUM INTO` backup is made first. `--backup` chooses the path; the default
  is a timestamped file beside the database.
- `--fork` inserts a new top-level thread and leaves the original byte-identical.
- The tool refuses unknown thread format versions rather than guessing.
- Existing messages are preserved as raw JSON, including fields this tool does
  not model.

## Why this is not an `at-*` tool

The `at-` prefix promises that the whole grammar is safe to allowlist. The audit
reads a private store outside the worktree, and the recover command and TUI write
to it. That is a different capability from `at-peek` and `at-recall`, so it lives
outside that namespace and outside the `agent-tools` package.

## Build and test

```sh
direnv exec . cargo test --offline -p zed-thread-tools
direnv exec . cargo clippy --offline -p zed-thread-tools --all-targets -- -D warnings
```

The TUI dependency is cached locally, so `--offline` works.
