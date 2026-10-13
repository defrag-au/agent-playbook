---
id: kill-only-what-you-started
title: Kill only the processes you started — never by name pattern
layer: core
activation: always
priority: 78
overrides:
targets:
---

## Directive

- Never clean up with `pkill -f "<app>"`, `killall <app>`, or any pattern that matches a
  binary by name — it also matches the copy the user is running interactively. Kill only the
  instances you started, by an identifier unique to them: a PID you captured, a flag you
  passed, or a port only your process bound.
- A long-running helper you spawn — a headless browser, a dev server, a watcher — gets a
  unique handle on its own command line: `--user-data-dir=.tmp/<task>-profile`, a `--port` you
  chose, an output path under `.tmp/`. Clean up with *that string*, not the app's name.
- The handle must be one only your process carries. `pkill -f "Brave Browser"` is the failure;
  `pkill -f "brave-profile-<task>"` is the fix, because the user's own Brave has no such flag.
- Cannot give it a unique handle → kill nothing. Say what you left running instead.
- Put scratch profiles and artefacts under `.tmp/` in the repo, so the handle and the cleanup
  target are unambiguous.

## Rationale

Screenshots were being taken with headless Chromium (Brave), and `pkill -f "Brave Browser"`
was used to tidy up between runs. It killed the user's own browser session — the same binary,
matched by the same name. The intent was right; the pattern was not scoped to the process it
was meant to reach.

A process name is shared by every instance of that program, including the interactive one the
user has open. The only reliable cleanup target is something you put on the command line
yourself.
