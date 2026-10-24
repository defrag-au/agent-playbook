---
id: augminted-browser-capture
title: Browser captures and scenarios run through browser-shot
layer: project
activation: project:augminted-bots
priority: 55
overrides:
targets:
---

## Directive

**Browser captures and scenarios in this repo run through `browser-shot`, from the repo root so
artefacts land in `.tmp/`.** Never hand-launch Brave, reuse a debugging port, run `cdp-shot.mjs`,
or clean up with `pkill`.

```sh
browser-shot "<url>" 390 844 .tmp/shot.png --settle-ms 4000
```

- Interactions, assertions or two peers → `run --scenario <file.dsl>`; the grammar and field tables
  are `~/code/defrag/agent-playbook/tools/browser-shot/SCENARIO.md`.
- Animation → `burst <peer> out=frames/<name> frames=120 mode=screencast` (~59 fps; sequential
  captures are ~7.5 fps).
- A frame rate is a condition → `wait <peer> rate=30`. The sidecar's `frame rate` line names the
  measured rate or the counter/clock that could not be read, and warns below 20 Hz — headless
  software rendering is single-digit fps, so animation and two-peer runs need `--headed`.
- A measurement series → `eval <peer> "<expression>" out=samples.json`. It writes the value as JSON
  beside the images; `capture=` only binds a value for `${name}` interpolation. `run.json`'s
  `outputs` lists the files. A `Map`, a cyclic object or a `BigInt` fails the step by name.
- A macroquad web client → `--select canvas --dpr 3 --touch`. `--window-size` is not emulation.
- Exit `0` passed · `2` captured but an assertion failed · `1` operational. Read `<out>.txt` —
  `warnings`, `renderer … SOFTWARE`, `image` and `layout` — before claiming a capture rendered.
- The procedure and its traps are the `browser-shot` skill.

## Rationale

The predecessor was a Node script plus a hand-launched browser on port 9222, prose instructions to
`pkill` it, and a fixed sleep. It could not do touch, right-click, drag, held modifiers or
animation, and a wrong-DPR shot or a swallowed 404 looked exactly like evidence. `browser-shot`
owns the browser process, emulates the viewport, stamps input against the page's frame counter, and
states what it did not prove (2026-10-23/24). It is packaged from `agent-playbook` and on `PATH` in
every defrag devshell, so the invocation above needs no checkout.
