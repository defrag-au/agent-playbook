---
id: defrag-look-at-the-game
title: Capture the exact moment, read its report, then check the real window
layer: org
activation: renders:macroquad
priority: 46
overrides:
targets:
---

## Directive

A macroquad screen is not done until a capture of each changed state has been looked at, its
report's problems dealt with, and the real window checked once after any camera or input change.

```sh
nix develop -c cargo build -p <game>
./target/debug/<game> --shot .tmp/x.png [--at 1.25] [--hover N | --drag N | --rclick N]
./target/debug/<game> --sheet .tmp/sheet.png     # every face at once, where the game has one
```

- Each capture writes `x.png` and `x.txt`. **The image is the review.** The report is the second
  check: it leads with what a glance misses (text cut short, too small, off its surface,
  overlapping), then lists every line of text with its box and size.
- Capture **the moment**, not the end state: `--at` on the fixed clock catches a card mid-lift or a
  sheen mid-sweep. An animation is reviewed at three moments, not one.
- Problems listed in the report are fixed or explained before the work is reported done.
- **The window is not a capture.** After touching the camera, input or window setup, run the game
  without `--shot` and screenshot the real window (the `game-spike` skill has the command) before
  calling it done.
- Not evidence: it compiles · the soak passes · a capture from before the change.

## Rationale

The card pass on `fight-macroquad` was done by capturing exact moments and fixing what they showed:
the missing title on the zoomed card, the vanished spaces in shrunk names, and a sheen that tinted
instead of glinting were each invisible in code and obvious in a capture.

The window rule exists because captures draw into a render target, which macroquad orients
differently from the window: the game was upside down on screen while every capture was right.
This is the macroquad counterpart of `defrag-look-at-what-you-built`, which covers egui.
