---
id: defrag-commit-conventions
title: Commit and PR conventions for this ecosystem
layer: org
activation: org:defrag
priority: 40
overrides:
targets:
---

Applies when you are asked to prepare a commit or draft a PR — see
[`core/git-is-the-users-domain`](../../core/git-is-the-users-domain.md) for the standing rule
that you do not commit uninvited.

## Commits

Prefix with a type, then the change, then the PR number:

```
feature: indexer retries (#164)
fix: wrap the stake address in the compact breakpoint (#171)
chore: bump pallas to 0.31 (#168)
```

- The prefixes in use are **`feature:`, `chore:`, `fix:`** — not `feat:`/`refactor:`/`perf:`.
  Match what is already in the log rather than conventional-commits defaults.
- **Append the PR number** as `(#164)`. The log is read as a list of changes and their
  discussions.
- Scope narrowly. One concern per commit — a formatting sweep bundled with a behaviour change
  makes the behaviour change unreviewable.
- Imperative, present tense: "wrap the address", not "wrapped the address".

## Pull requests

Include a concise summary, the linked issue, test coverage for the change, and example output
or screenshots where they apply. Note any feature flags involved (`native`, `wasm`).

`[skip ci]` needs a clear justification in the description, and any manual deployment or data
task has to be named — CI cannot catch what CI did not run.
