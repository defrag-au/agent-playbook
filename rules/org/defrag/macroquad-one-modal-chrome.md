---
id: defrag-macroquad-one-modal-chrome
title: Every macroquad modal draws through the widgets' sheet chrome, and a better look upgrades it
layer: org
activation: renders:macroquad
priority: 54
overrides:
targets:
---

## Directive

Every macroquad modal draws its card with `macroquad_widgets::sheet_card` and a `Sheet` spec
(title, subtitle, tint, footer). Never paint a scrim, a card or a close button of its own. When
one modal looks better than the shared chrome, upgrade `sheet_card` and its parts to draw that
way, then move every modal onto them. Do not fork a one-off chrome for the good-looking one.

Inside the card:

- **Facts** go in a `macroquad_widgets::well`, as `WellRow`s: inline for label and value,
  stacked for a label over its value, with a trailing figure or Copy, a meter, or `.strong(true)`
  for a total.
- **Actions** are `Pill`s (filled, ghost or danger) in `SheetCard::footer`, laid out with
  `pill_row`. Pass `.live(card.live)` so a sheet that is still sliding takes no tap.
- **Closing** is the header's cross plus a tap on the scrim, both reported as
  `SheetCard::dismissed`. A sheet has no Close button.
- **A modal with its own motion and layout**, like the gamekit's wallet modal, still paints with
  `sheet_surface` and closes with `sheet_close`, so it cannot drift from the rest.

The rounded shapes and the gradient they draw with live in `macroquad_widgets::shape`. The
gamekit re-exports them.

## Rationale

On 2026-10-30 the user set the gamekit wallet modal beside the staking site's Perp detail. They
called the first excellent and the second lacklustre. The wallet modal had its gradient card,
glow, border, facts panel and pills as private code, while every other modal used a flat
`sheet_card` with a slab Close button. The user's direction was to upgrade the reusable
implementation to draw the way the wallet modal does, and have everything use it. The wallet
modal's pieces moved into `macroquad-widgets` that day. The cart, the Perp detail, the purchase
sheet and fastbuy's detail sheet all moved onto them.

## Applies to

Modal sheets and cards. A surface that is deliberately not a modal keeps its own chrome and says
why in its module docs. `arcade-admin-quad`'s operator console and the pump client's side drawer
are the two that do today.
