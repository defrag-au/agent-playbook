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

A skill a repository needs is delivered from an **OCFL store**, not copied by hand. The store
lives next to the tool the skill operates — `container-compose/.skill-store/` — and the
repository declares what it wants:

```conf
# <repo>/.playbook/project.conf
skills: github:defrag-au/container-compose#container-compose@v1
```

```sh
playbook skills sync  --target zed      # export, copy into .agents/skills/, record the lock
playbook skills check --target zed      # what CI runs
```

The version and the exported tree's digest go into `.playbook/skills.lock`, and a directory that
the lock does not name is refused rather than overwritten. `skill-store` publishes a plain
directory as a version:

```sh
skill-store publish <repo>/<skill> --store <repo>/.skill-store --init
skill-store verify  path:<repo>/.skill-store
```

This directory holds the skills whose subject **is** this repository's toolkit — `at-peek` and
`at-crate` for `inspect-code`, the thread tools for `crate-research`, the composer for
`capture-rule`. They are published into [`.skill-store/`](../.skill-store) at the repository root,
which is what `github:defrag-au/agent-playbook#inspect-code@v1` resolves to.

## Authoring here, publishing there

`skills/<name>/` is the editable source; `.skill-store/<name>/vN/` is the published artifact. Edit
here, then publish — a version is a release, not a keystroke, and publishing identical content is
refused.

```sh
skill-store publish skills/inspect-code --store .skill-store --message "what changed"
skill-store verify  path:.skill-store
```

To see whether a publish is owed, export the head and diff it against the source (the export
refuses a non-empty `--out`, so pick an unused directory):

```sh
skill-store export 'path:.#inspect-code@v1' --out .tmp/check-inspect-code
diff -r .tmp/check-inspect-code/skill skills/inspect-code
```

An edit here does not need a publish to be seen somewhere. A consuming manifest's
`.playbook/skills.patch` maps a skill name to this directory, and `playbook skills sync` delivers
the working copy as `working` — no version, no store entry. See
[`docs/skills.md`](../docs/skills.md#developing-against-a-skill).

`fleet-ops` is still here but belongs in `fleet`, and moves when that repository gets a store.
`widget-screenshot` and `game-spike` left for `shared-crates/.skill-store/` — the storybook and
`macroquad-gamekit` live there.

Delivering the global set is wired: the personal catalogue is
`fleet/personal/.playbook/project.conf`, with `skills_scope: global` and one `playbook skills sync
--target all`. It lists only the skills that are published; `widget-screenshot`, `game-spike` and
`fleet-ops` are not, so the hand-installed copies in `~/.agents/skills` are left alone until they
move to the repository that owns them.

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
