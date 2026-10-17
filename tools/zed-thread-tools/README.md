# zed-thread-tools

Three commands over Zed's agent thread store:

| Binary | Reads | Writes |
| --- | --- | --- |
| `zed-thread-audit` | `threads.db`, read-only | Never |
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
accumulated since the last compaction. It never prints message text.

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
