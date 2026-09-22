---
name: inspect-code
description: Use when finding, reading, counting or locating code — replaces rg, grep, sed, cat, head and wc for that job with the read-only at-peek verbs, and covers what to do when the toolkit is not installed.
---

# Reading code and history with the agent toolkit

The situation that should have brought you here: you are about to run `rg`, `grep`, `sed`, `cat`,
`head` or `wc` to look at code. Don't — reach for `at-peek` instead. It is read-only, bounded by
construction, and its output says what it did not show.

`at-describe` lists everything the toolkit can do, on one screen. This skill is the mapping and
the traps.

## The mapping

```sh
at-peek search 'render_claim'                 # every mention, walked from the repo root
at-peek search 'fn \w+' tools/ --count        # matches per file, with the real total
at-peek search 'FIXME' --files-only           # just the paths
at-peek slice crates/x/src/lib.rs:40-60       # these exact lines, numbered
at-peek slice crates/x/src/lib.rs:40+20       # twenty lines from 40
at-peek stat crates/x/src/lib.rs              # lines, bytes, language, mtime
```

`--limit N` caps the output (default 200 lines / 50 matches); `--root <path>` points the tool at
another tree; `--max-files N` bounds a walk. Every ceiling is announced when it clamps.

## What the output tells you

A `# ` line is metadata: the header names what was read, and the trailing bounds name what was
not. `# 50 of 143 matches in 27 files` means what it says — 143 is the count over every file the
walk considered, not the number it happened to print. Repeat the bound when you report the
finding; the whole point of the tool is that a truncated answer stops being mistakable for a
complete one.

`# skipped by rule: target, node_modules` and `# skipped: 3 secret-shaped · 2 binary` are the same
discipline: what it refused to read, named rather than dropped.

## Traps

- **`at-peek` is not a read for the purposes of editing.** It does not satisfy the editor tools'
  read-before-edit check, so a `slice` followed by an edit gets a stale-read refusal. Use it to
  understand, then read the file with the editor tools before changing it.
- **`path:40-60` is one token, not two arguments**, and ranges are 1-based and inclusive.
- **`at-peek` never opens `.git`.** "Is this file tracked", "what branch is this" and "who wrote
  it" are not `at-peek` questions, and the tool that answers them does not exist yet — ask.
- **`search` does not honour `.gitignore`.** It refuses a fixed list of directory names
  (`target`, `node_modules`, `.direnv`, `dist`, `result`, `.tmp`, `.git`) and names what it
  skipped. If a search looks incomplete, read the trailer before concluding the code is absent.
- **A walk is bounded by `--max-files`** (default 20000) and says so when it stops. Narrow the
  path rather than raising it.
- **If `at-peek` is not on `PATH`**, say so and fall back to the shell tools — do not silently
  reproduce the pipeline this exists to replace.
