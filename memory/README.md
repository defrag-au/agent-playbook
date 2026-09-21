# memory/

Durable facts — about the environment and about how I want to be worked with.

This layer is delivered by the **`zed-personal` target**, into Zed's personal instructions
file (`~/.config/zed/AGENTS.md`), which loads for every project. It is not part of any
repository's block: a fact about the machine is not a fact about a repo, and a repo's
`AGENTS.md` is committed and shared.

## What belongs here

| Kind | Example |
| --- | --- |
| Environment | "Repositories are laid out as `~/code/<org>/<repo>`" |
| Tooling available on the machine | "`nix` and `direnv` are installed" |
| Durable preference | "Screenshots I send are the newest file in `~/Desktop`" |

## What does not

- **Anything with an "always" or a "never" in it** → `rules/`. A constraint belongs where it
  can be layered, overridden and enforced; memory cannot.
- **Anything that restates a rule.** The personal file and a repo's block are read *together*,
  so a fact that is already a rule is duplication with no reader. "Use the devshell" is a
  rule; "`direnv` is at `~/.nix-profile/bin/direnv`" is a fact.
- **A procedure** → `skills/`.
- **A crate's API** → `references/crates/`.

## Keeping it honest

Memory is the layer most likely to rot, because it records things that were true once. When an
entry stops being true, **delete it** — do not append a correction below it. A memory file
with a stale fact and a note saying so is read as two facts, and the agent has no way to know
which one is current.

The one exception is a fact whose *replacement* is unintuitive, where the stale version is
worth naming so it is not rediscovered. `projects/archivist/rules/devshell-commands.md` does
this for the profile-sourcing advice — but note that it lives in a **rule**, not here, because
it is a constraint rather than a fact.
