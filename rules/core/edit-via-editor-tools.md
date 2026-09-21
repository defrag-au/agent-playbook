---
id: edit-via-editor-tools
title: Edit files with the editor tools, never with a shell script
layer: core
activation: always
priority: 80
overrides:
targets:
---

## Directive

- Edit files with the read/edit/write tools. Never `python`, `sed`, `awk`, `perl`, `truncate`
  or a heredoc — not for a big change, and not for "just one small change". This applies to
  every file: source, config, docs, rules, memory.
- A change that touches ten places is ten edit calls, not one script.
- The editor tools verify the file was read first, fail loudly on a stale or ambiguous
  match, and show a reviewable diff. A script's `str.replace` **silently does nothing** when
  the anchor text has moved.
- Still fine: generating a file's *content* with a script when the content is genuinely
  computed (a catalogue from source headers, a table derived from data) — writing it to disk
  still goes through the write tool. And reads through the shell (`cat`, `grep`, `find`) to
  gather information, before editing with the editor tools.

## Rationale

The "silently does nothing" case is not hypothetical. A write was reported as done after
`cargo fmt` reflowed the anchor the script was matching against — the script found nothing,
wrote nothing, exited zero, and the change was never made. The failure was invisible until
much later.

That is the whole argument: a tool that fails loudly costs a retry, and a tool that fails
quietly costs the trustworthiness of every subsequent claim about the file.
