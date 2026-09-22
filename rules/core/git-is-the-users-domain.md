---
id: git-is-the-users-domain
title: Git history is mine — do not commit, push, merge or branch
layer: core
activation: always
priority: 82
overrides:
targets:
---

## Directive

- Do not run `git commit`, `push`, `merge`, `rebase`, `tag` or anything else that mutates
  history or a remote unless I specifically ask. Leave changes staged or unstaged for me to
  review and commit myself.
- Read-only git is always fine: `status`, `diff`, `log`, `show`, `blame`. Use `--no-pager` on
  all of them.
- Branching for work is fine when it helps. Creating a branch rewrites nothing I have to
  unpick.
- Also: no `[skip ci]`, `--no-verify` or bypassing hooks without asking · no amend,
  force-push or reset · do not commit artefacts `.gitignore` excludes, even when they are up
  to date · do not write a commit message and leave it staged in a way that suggests it was
  committed.

## Rationale

I want to see the diff before it becomes history, and I usually want to write the message.

The sub-rules are the same principle at smaller scale: `--no-verify` and `[skip ci]` both
bypass a check I put there deliberately, and a `dist/` that is gitignored but committed anyway
means the ignore rule is now decorative.
