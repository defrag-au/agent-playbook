---
id: defrag-look-at-the-tui
title: A terminal screen is not done until its captures have been read
layer: org
activation: renders:tui
priority: 46
overrides:
targets:
---

## Directive

A TUI screen is not done until a capture of each changed state has been read — the `.txt` first,
then its `.json`.

- Capture each state that changed: arrival, a narrowed terminal, the modal sheet, a scrolled view,
  a failed turn, a disconnected header. The moment, not only the end state.
- **The `.txt` is the review.** The `.json` is the second check: `warnings` (a wide character at
  the edge, a control character, a blank screen), `edge_rows` (a row whose last cell is prose, a
  candidate for text cut at the edge), and `styles` (inverse, bold and colour runs that plain text
  loses — the selected row, the permission sheet).
- `actions` in a harness run is the input check: the verb each key would have sent. A key that
  resolves nothing reads as `none`, so a plan that meant to answer is caught there.
- Problems the report lists are fixed or explained before the work is reported done.
- Not evidence: it compiles · its tests pass · a capture from before the change.

## Rationale

The first PTY capture of the factotum TUI read `│untitled session│` and
`Tidy the session fold soapermissioncanoutlivetheturn…`. The words were right and the screen was
wrong, because the tool skipped every cell the app had never written. It was found by reading the
capture against the in-process one — no test failed, and the JSON said `0 warnings`.

This is the terminal counterpart of `defrag-look-at-what-you-built` (egui) and
`defrag-look-at-the-game` (macroquad).
