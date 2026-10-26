---
id: defrag-tui-renderer-is-pure
title: A TUI renderer is a function of the view, the UI state and a clock
layer: org
activation: renders:tui
priority: 50
overrides:
targets:
---

## Directive

**Draw `present(view, now_ms)` into a `Frame` and read nothing else.** A renderer that reads the
clock, the socket or the filesystem while drawing cannot be replayed, so its screens can only be
reviewed by running the whole system.

- The clock is an argument, not `SystemTime::now()` inside `draw`: a relative label or an elapsed
  turn counter is then reviewable at any moment with `at=`.
- The key handler returns what happened and the caller sends it:
  `UiAction::{Prompt, ResolvePermission, Resync, Dump, Quit}`; a key that only scrolls or edits the
  draft returns `None`.
- Split the crate so this is structural: `render.rs` (drawing and `UiState`), `harness.rs` (a
  `TestBackend` loop over a captured view), `main.rs` (client, terminal, send).
- The renderer takes `Status` as a value, so a capture can show `connecting`, `reconnecting` or
  `offline` without a socket.
- The review fixture is a `SessionView` captured from a live session with `--dump-state`, never
  written by hand.

## Why

`factotum-tui` read the clock inside its draw function and kept the view, the scroll position and
the socket in one `App`, so the only way to see a screen was to run it. Splitting it left two
callers of the same `draw`: the live loop and a `TestBackend` that draws a captured view in
180–280 µs. The two screens agree cell for cell — verified by diffing a PTY capture against a
harness capture, which differed only in the elapsed-time label.
