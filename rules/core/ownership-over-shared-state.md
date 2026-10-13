---
id: ownership-over-shared-state
title: Prefer ownership over shared mutable state
layer: core
activation: always
priority: 58
overrides:
targets:
---

## Directive

Prefer an ownership design to shared mutable state. Do not reach for `Rc<RefCell<T>>`,
`Arc<Mutex<T>>` or a `static` lock to get around a borrow, lifetime or callback problem —
restructure the value instead, and say what the restructure is.

Three shapes that usually replace it:

- **Split the value.** The thing being mutated and the things reading it become separate
  objects, wired together by the caller that owns both.
- **Move the state to the caller.** The count, the cache, the cursor is owned by whoever
  drives the loop and passed in, so there is one writer by construction.
- **Replace the check, not the state.** If the shared state existed only to catch a misuse, a
  type parameter, a consumed token, or a `Result` from the one place that can see the misuse
  costs less and cannot be reached around.

If shared mutable state is genuinely the answer, name what breaks without it and ask before
building on it. Do not offer it as the low-effort option in a list of designs either.

Narrow existing uses are not this rule: a counter or a memo already behind `&self` inside one
type is an implementation detail, not a design, and it does not need a decision from me.

## Rationale

The failure is not the borrow panic; it is that the state becomes reachable from everywhere,
so its invariants stop being checkable by reading one call site. A design that wants
`Rc<RefCell<T>>` is usually one where two owners were allowed to exist, and the cheapest
moment to fix that is before it is written.

Incident, 2026-09-29, building `crates/fetch-driver` in `mitos`: a request could not be tied
to the queue that issued it at compile time without a lifetime that made the intended driver
loop — take several requests, await them, settle them one at a time — uncomputable. Putting
an `Rc<RefCell<Queue>>` in the request was offered as the way to close it, and was declined —
*"I feel like I've been bitten by Rc<RefCell<T>> implementations in the past."* The design
kept the loop and took a documented `Result` from the one method that can detect the misuse.
Recorded in `docs/inventory.md`.

## Applies to

Choosing a pattern for a borrow, lifetime or callback problem; designing state that more than
one place needs to reach. Not a general ban on `RefCell` — the rule is about what a design is
built on, not about a lock taken inside one function.
