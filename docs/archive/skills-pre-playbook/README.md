# Pre-playbook skills — archived

These eight `SKILL.md` files are the **predecessors** of the current rule set. They were
written for `~/.agents/skills/` before this repository existed, and they are kept here as the
primary source for the claims `docs/inventory.md` makes about them.

**Nothing loads these.** They are not rules — no composer walks `docs/`, and the live skills
are in [`skills/`](../../../skills/). Do not edit them: they are a historical record, and
editing one would break the thing they are here for.

## Why they were retired

Every one carried:

```yaml
description: (no description)
disable-model-invocation: true
```

`disable-model-invocation` removes a skill from the model's catalogue, and a missing
description leaves nothing to match on. Between them **none of the eight could ever fire
automatically** — they were standing constraints filed as skills, which is the misfiling
[`skills/README.md`](../../../skills/README.md) describes. They were still reachable by hand
from the `/` menu, which is why leaving them in place was a trap rather than just clutter.

## Where each one's content went

| Archived skill | Now lives as |
| --- | --- |
| `the-working-first-engineering-rule` | `rules/core/working-first` |
| `never-make-things-up` | `rules/core/never-make-things-up` |
| `dont-rush-new-features` | `rules/core/dont-rush-new-features` |
| `conservative-package-changes` | `rules/core/conservative-package-changes` |
| `build-often` | `rules/rust/build-often` |
| `mcp-constraint-rule-ban-serdejsonvalue` | `rules/core/typed-json-only` |
| `perform-useful-crate-research` | `skills/crate-research` — rewritten and extended |
| `rust-environment-verification` | **superseded** — see below |

`rust-environment-verification` is the one that was actively wrong, not merely redundant. It
said to run `source ~/.bash_profile && cargo --version` to find the toolchain, which
`rules/rust/devshell-first` exists to replace: it picks up an unpinned toolchain and makes the
failure mode depend on whether the shell was warm. What replaced it is recorded in
`projects/archivist/rules/devshell-commands.md` under "The advice this replaced", so the fact
that it was ever believed is not lost.

## Provenance

Archived 2026-09-22, from `~/.agents/skills/`, which is **not** under version control — this is
now the only tracked copy of the original wording. That is the reason for keeping them rather
than deleting them: `docs/inventory.md` quotes and characterises these files, and a reader
should be able to check.
