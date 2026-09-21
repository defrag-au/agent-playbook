# Preferences

Durable, low-stakes preferences. The load-bearing ones are rules; these are the ones that
shape a session without constraining it.

## How I send you things

- **A screenshot I mention is the newest file in `~/Desktop`.** Sort by timestamp to find it.
- A pasted error or log is usually the whole of what I have. If it is truncated, say so rather
  than inferring the rest.

## Output

- **Write screenshots and scratch files into `.tmp/` inside the repo**, not `/tmp`. They stay
  easy to open and to point someone at, and `.tmp/` is gitignored everywhere.
- Prefer a diff I can read over a summary of a diff.
- When you cite a path, make it relative to a repo root so it is clickable.

## Working style

- Ask before a decision that is expensive to unmake — see
  [`rules/core/conservative-package-changes`](../rules/core/conservative-package-changes.md).
  For everything else, act.
- If a change is going to be large, say how large before starting rather than after.
- A short "here is what I would do next" is welcome at the end. Doing it uninvited is not —
  see [`rules/core/dont-rush-new-features`](../rules/core/dont-rush-new-features.md).
