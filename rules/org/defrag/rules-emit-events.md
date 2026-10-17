---
id: defrag-rules-emit-events
title: A game's rules crate reports what happened; the client animates it and decides nothing
layer: org
activation: renders:macroquad
priority: 54
overrides:
targets:
---

## Directive

**The rules crate returns what happened as data, and the client only animates it.** A client never
re-derives a rule — not damage, not which card was drawn, not where a stripped card went.

- Each step returns typed events (a card moved from pile to pile, an effect resolved with an
  amount) beside the new view. The client queues animations from the events, then shows the view.
- Anything the screen shows as a number or an icon arrives as data, not only as a sentence:
  `effects: Vec<EffectView>` beside `text: Vec<String>`, so the face draws icons without parsing
  prose.
- The client checks itself against the engine: after each step it compares what it shows (the
  hand, the pile counts) with the view, and the soak fails on any mismatch.
- The rules crate is pure (see `defrag-runtime-pairs`): one crate serves the macroquad client, a web
  bridge and a replay verifier.

## Rationale

`deck-fight` feeds three clients — macroquad, PixiJS and Phaser — from one session type. When the
pile counts on screen went wrong (a card stripped from the hand, a discard one short after the
killing card), the fix each time was in the engine: report the event it had not been reporting
(`PileOp::Stripped`) or count the card it had missed, with a test. Had a client inferred the piles
itself, each of the three would have needed its own fix, and could have disagreed with the others.
