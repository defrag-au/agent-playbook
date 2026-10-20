# references/

Looked-up material. Not loaded every turn — a rule or a skill cites a path here, and the
agent reads it when the task needs it.

```
crates/       one file per crate@version — API cheat sheets
  CATALOG.md  the index; read this before any individual reference
templates/    scaffolds for the above
```

## Start here

`crates/CATALOG.md` is the index. It names each file, the version, the purpose and the
first section to read. Read it first; do not guess a filename or open every crate file.

## How to read one

These files are cheap on disk and expensive in context. Read one reference at a time, and
read a section rather than the whole file:

```sh
at-peek stat references/crates/<file>.md
at-peek slice references/crates/<file>.md:1-25
at-peek search '<symbol>' references/crates/<file>.md
```

- Start with `## At a glance` when it exists. Otherwise read `## Cargo.toml` and the
  section named in the catalogue.
- Never load several references in one turn.
- Do not read a whole file over ~120 lines just to find one API; slice or search.
- Budget: <= 200 lines and <= 8 KB per file. The test suite enforces a 10 KB ceiling and
  checks that every file has a catalogue row.

## `crates/`

Named `<crate>-<version>.md`, e.g. `liquid-0.26.md`. **The version is in the filename on
purpose:** a cheat sheet for `0.26` is wrong the moment `0.27` renames a signature, and the
filename is what makes that visible. A file called `liquid.md` is a claim about `latest`, and
`latest` is not a version.

Write one when a crate is adopted, using the `crate-research` skill. Two real examples to
follow: `crates/liquid-0.26.md` and `crates/rmcp-1.3.md`. Add its row to
`crates/CATALOG.md` in the same change — an orphan reference is a reference nobody finds.

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
