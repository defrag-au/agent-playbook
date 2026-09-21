---
id: defrag-commit-conventions
title: Commit and PR conventions for this ecosystem
layer: org
activation: org:defrag
priority: 40
overrides:
targets:
---

## Directive

Applies only when asked to prepare a commit or draft a PR — see
[`core/git-is-the-users-domain`](../../core/git-is-the-users-domain.md) for the standing rule
that you do not commit uninvited.

### Commits

Prefix with a type, then the change, then the PR number:

```
feature: indexer retries (#164)
fix: wrap the stake address in the compact breakpoint (#171)
chore: bump pallas to 0.31 (#168)
```

- Prefixes are `feature:`, `chore:`, `fix:` · never `feat:`/`refactor:`/`perf:` — match the
  existing log, not conventional-commits defaults.
- Append the PR number as `(#164)`.
- One concern per commit — never bundle a formatting sweep with a behaviour change.
- Imperative, present tense: "wrap the address", not "wrapped the address".

### Pull requests

- Include a concise summary, the linked issue, test coverage for the change, and example output or
  screenshots where they apply.
- Note any feature flags involved (`native`, `wasm`).
- `[skip ci]` → needs a clear justification in the description, and any manual deployment or data
  task named (CI cannot catch what CI did not run).

## Rationale

The log is read as a list of changes and their discussions, which is why the PR number belongs in
the subject rather than as a trailer — a commit without one is a change with no discussion to find.

The prefix set is the three this ecosystem actually uses, not the conventional-commits defaults.
`feat:`/`refactor:`/`perf:` do not appear in the log, so a commit that uses them is the only one of
its kind and cannot be filtered alongside the rest.

Narrow scope is a reviewability constraint rather than tidiness: a formatting sweep bundled with a
behaviour change makes the behaviour change unreviewable, because the diff no longer isolates it.

`[skip ci]` is the one thing CI cannot verify, so the description has to carry it — a manual
deployment or data task that is not named is a step nobody knows to run.
