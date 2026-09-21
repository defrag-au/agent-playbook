# Tool discipline

**Status: provisional.** Written as a hypothesis about what a fast model needs told. Delete
anything here that a session does not demonstrate a need for — an unnecessary addendum is
paid for on every turn.

## Batch independent calls

If two reads do not depend on each other, they go in the same response. A file listing, a
`grep` for a symbol, and a read of a known path are three independent things: one response,
not three. Sequential exploratory calls are the most common way a fast model wastes its own
speed.

## Do not re-read after a successful write

A write that returned success has succeeded. Re-reading the file to confirm it is a wasted
turn — the tool would have failed loudly if it had not applied.

## Prefer one decisive command to a chain of guesses

`cargo check -p <crate>` answers "does this compile" better than reading three files. Where a
tool can decide the question, run the tool — see
[`rules/rust/build-often`](../../../rules/rust/build-often.md).

## Read the whole error before acting

Rust errors cascade: the first is usually the real one and the rest are consequences of it.
Fix the consequence first and the next round of errors looks new.

## Everything else is already a rule

If a failure mode is covered by a file in `rules/`, it does not belong here — a second copy
is a second thing to keep in sync. This addendum is only for mechanics that no rule covers.
