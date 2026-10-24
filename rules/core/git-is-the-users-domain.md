---
id: git-is-the-users-domain
title: Git state is mine — do not commit, push, merge or stage
layer: core
activation: always
priority: 82
overrides:
targets:
---

## Directive

- Do not run `git add`, `git reset`, `git restore --staged`, `git stash`, `git commit`, `push`,
  `merge`, `rebase`, `tag` or anything else that changes the index, history or a remote unless I
  specifically ask. Leave the index exactly as you found it; do not stage or unstage anything.
- Read-only git is fine when the inspectors are not on `PATH`: `status`, `diff`, `log`, `show`,
  `blame`. Use `--no-pager` on all of them. Reach for `at-recall`/`at-peek` first when the
  devshell provides them; they are bounded and say what they did not show.
- Branching for work is fine when it helps. Creating a branch rewrites nothing I have to
  unpick.
- If you change the index by accident, undo exactly that and say so.
- Also: no `[skip ci]`, `--no-verify` or bypassing hooks without asking · no amend or
  force-push · do not commit artefacts `.gitignore` excludes, even when they are up to date ·
  do not write a commit message and leave it staged in a way that suggests it was committed.

## Rationale

I want to see the diff before it becomes history, and I usually want to write the message. The
index is part of that review: a staged change is one I have to notice and unstage before I can
tell what you did.

Incident, 2026-10-24, `construct2`: to make a new `flake.nix` visible to `nix develop`, an agent
ran `git add -N` and then `git add` on the flake files and the README, staging the user's changes
without being asked. The correction was *“You seem to be using git liberally — which I'd prefer
you didn't.”* Nix requiring a flake to be tracked is a reason to ask the user to stage it, not a
reason to stage it for them.

The sub-rules are the same principle at smaller scale: `--no-verify` and `[skip ci]` both
bypass a check I put there deliberately, and a `dist/` that is gitignored but committed anyway
means the ignore rule is now decorative.
