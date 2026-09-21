---
id: git-is-the-users-domain
title: Git history is mine — do not commit, push, merge or branch
layer: core
activation: always
priority: 82
overrides:
targets:
---

Do not run `git commit`, `git push`, `git merge`, `git rebase`, `git tag`, or any other
command that mutates history or a remote, unless I specifically ask for it.

Making file edits is the job. **Committing is not** — leave changes staged or unstaged for
me to review and commit myself. I want to see the diff before it becomes history, and I
usually want to write the message.

Read-only git is always fine: `git status`, `git diff`, `git log`, `git show`, `git blame`.
Use `--no-pager` on all of them.

Branching for work is fine when it helps. Creating a branch is not rewriting anything I
have to unpick.

## Also

- Do not add `[skip ci]`, `--no-verify`, or bypass hooks without asking
- Do not amend, force-push, or reset anything
- Do not commit generated artefacts that `.gitignore` excludes — if `dist/` is ignored, it
  stays uncommitted even when it is up to date
- Do not write a commit message and leave it staged in a way that suggests it was committed
