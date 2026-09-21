---
id: edit-via-editor-tools
title: Edit files with the editor tools, never with a shell script
layer: core
activation: always
priority: 80
overrides:
targets:
---

Edit files with the read/edit/write tools. Never shell out to `python`, `sed`, `awk`,
`perl`, `truncate`, or a heredoc to modify a file — not for a big change, and not for "just
one small change".

This applies to every file: source, config, docs, rules, memory.

Multi-edit convenience scripts are included. If a change touches ten places, that is ten edit
calls, not one script.

## Why

- The editor tools verify the file was read first and **fail loudly** on an ambiguous or
  stale match.
- They show a reviewable diff.
- A script's `str.replace` **silently does nothing** when the anchor text has moved.

That last one is not hypothetical. A write was reported as done after `cargo fmt` reflowed
the anchor it was matching against — the script found nothing, wrote nothing, exited zero,
and the change was never made. The failure was invisible until much later. Editor tools make
that class of bug impossible.

## What is still fine

Generating a file's *content* with a script is fine when the content is genuinely computed —
a catalogue built from source headers, a table derived from data. Writing it to disk still
goes through the write tool.

Reads through the shell are fine too: `cat`, `grep`, `find` to gather information, then edit
with the editor tools.
