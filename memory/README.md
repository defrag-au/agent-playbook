# memory/

Durable facts — about the environment and about how I want to be worked with.

This layer is concatenated into the agent file alongside `rules/`, so it is always loaded. It
is separate from `rules/` because a fact and a constraint have different lifetimes: a rule
changes when a decision changes, a memory changes when the *world* changes.

## What belongs here

| Kind | Example |
| --- | --- |
| Environment | "`cargo` is not on `PATH` outside the devshell" |
| Tooling quirks | "Chromium headless floors the window near 620px" |
| Durable preference | "Screenshots I send are the newest file in `~/Desktop`" |
| Naming or path conventions | "Repos live under `~/code/<org>/<repo>`" |

## What does not

- **Anything with an "always" or a "never" in it** → `rules/`. A constraint belongs where it
  can be layered, overridden and enforced; memory cannot.
- **A procedure** → `skills/`.
- **A crate's API** → `references/crates/`.

## Keeping it honest

Memory is the layer most likely to rot, because it records things that were true once. When an
entry stops being true, **delete it** — do not append a correction below it. A memory file
with a stale fact and a note saying so is read as two facts, and the agent has no way to know
which one is current.

The one exception is a fact whose *replacement* is unintuitive, where the stale version is
worth naming so it is not rediscovered. `projects/archivist/rules/devshell-commands.md` does
this for the profile-sourcing advice: it says what the old advice was and why it is wrong,
because the symptom it was written for still occurs. That belongs in a rule, not here.
