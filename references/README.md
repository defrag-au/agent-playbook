# references/

Looked-up material. Not loaded every turn — a rule or a skill cites a path here, and the
agent reads it when the task needs it.

```
crates/       one file per crate@version — API cheat sheets
templates/    scaffolds for the above
```

## `crates/`

Named `<crate>-<version>.md`, e.g. `liquid-0.26.md`. **The version is in the filename on
purpose:** a cheat sheet for `0.26` is wrong the moment `0.27` renames a signature, and the
filename is what makes that visible. A file called `liquid.md` is a claim about `latest`, and
`latest` is not a version.

Write one when a crate is adopted, using the `crate-research` skill. Two real examples to
follow: `crates/liquid-0.26.md` and `crates/rmcp-1.3.md`.

### What makes one useful

It is written for **an agent that was not trained on this version**. That is the whole point —
the model's recollection of the crate is some older release, so the highest-value section is
the one that says what changed:

```markdown
## Key Differences from 0.12
```

Followed by concrete renames and signature changes. Below that, task-shaped sections with
complete, compilable blocks rather than an API listing — "how to write a custom filter" beats
"here are the 14 filter traits".

## What does not belong here

- **A constraint.** If it says "always" or "never", it is a rule.
- **A procedure with a trigger.** That is a skill.
- **A fact about the environment.** That is `memory/`.

References go stale silently, which is the risk of this layer. The version in the filename and
the pin in `Cargo.toml` are the two things that keep it honest.
