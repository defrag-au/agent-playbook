# skills/

On-demand procedures. Each is a directory with a `SKILL.md`, in the format Zed and Claude
Code both read:

```markdown
---
name: skill-name          # must match the directory name
description: one specific, actionable line — this is what the model sees when deciding
---

Instructions.
```

## How these differ from `rules/`

| | `rules/` | `skills/` |
| --- | --- | --- |
| Fires | Every turn | When a task matches the description |
| Shape | A constraint | A procedure with steps |
| Length | Under a screenful | As long as the procedure needs |
| Loaded by | Being concatenated into the agent file | Being discovered from its `description` |

The test: **does it have a trigger?** "Never edit an assertion to make a test pass" has no
trigger — it is true always, so it is a rule. "When adopting a crate, research it this way"
has a trigger, so it is a skill.

## Installing

Copy the directory, or symlink it:

```sh
cp -R skills/crate-research ~/.agents/skills/            # global
cp -R skills/widget-screenshot <repo>/.agents/skills/    # project-local
```

Prefer project-local when the skill only makes sense in one repo. `widget-screenshot` needs
the storybook and Brave, so it is a candidate for `shared-crates/.agents/skills/` rather than
the global directory.

## The failure this layout exists to prevent

The eight predecessor files in `~/.agents/skills/` were all written with:

```yaml
description: (no description)
disable-model-invocation: true
```

`disable-model-invocation: true` removes a skill from the model's catalogue; a missing
description leaves nothing to match on. Between them, **none of the eight could ever fire.**
They were rules stored where rules are not read.

So: a skill needs a real `description`, and `disable-model-invocation` is only for a skill you
intend to invoke by hand. If neither is true of what you are writing, it is a rule — put it in
`rules/`.
