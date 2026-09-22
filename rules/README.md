# rules/

Standing constraints. Always on, applied every turn, never invoked.

One file per rule. Format is in [`../docs/rule-format.md`](../docs/rule-format.md); a rule
whose frontmatter does not parse is skipped with a warning, so `--list` is the check.

## Which layer does a rule belong in?

| Layer | Test | Example |
| --- | --- | --- |
| `core/` | Would I want this for a Go project written by someone else? | Fix the cause, don't comment out the code |
| `rust/` | Is it true of Rust the language, or of the Rust tooling I use? | Tooling lives behind the Nix devshell |
| `org/defrag/` | Is it true across the defrag ecosystem but not of my work generally? | D1 reads are cheap and writes are expensive |
| `projects/<repo>/` | Is it true of exactly one repository? | archivist's redb cache is single-process |

A rule in `org/defrag/` activates on one of two facts, and the difference matters:

- **`org:defrag`** — the repository is operated by defrag. Use it for rules about our services.
- **`ecosystem:defrag`** — the repository is *built* the defrag way, wherever it lives: the same
devshell, the same toolkit, the conventions that follow. `~/code/hodlcroft/compositor` is this
case.

See the activation table in [`../docs/rule-format.md`](../docs/rule-format.md).

Move a rule **up** the moment a second consumer needs it. Move it **down** the moment only
one consumer needs it. A rule at the wrong layer is how the duplication this repo exists to
fix got started.

## What does not belong here

- **A procedure with steps and a trigger** → `skills/`
- **A fact about the environment** → `memory/`
- **A crate's API surface** → `references/crates/`
- **A per-model or per-harness quirk** → `models/`

The most common misfiling is a constraint written as a skill. Eight of them existed in
`~/.agents/skills/` — all with `description: (no description)` and
`disable-model-invocation: true`, which means none of them could ever be invoked. A rule
that can never fire is not a rule, it is a note.

## Rules with a target

`activation: manual` rules are never included automatically. They exist for constraints that
are real but situational — a one-off migration, a rule that is only correct while a
temporary constraint is in force. Reference them from a target overlay's `include:`.

Do not use `manual` as an alternative to deleting a rule you are unsure about. If it is not
currently true, it is currently wrong.
