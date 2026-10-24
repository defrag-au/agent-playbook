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
  specifically ask. The same prohibition covers discarding working-tree changes — `git restore
  --source`, `git checkout -- <paths>`, `git apply --reverse`, `git clean` — because the working
  tree is mine to review too. Leave the index exactly as you found it; do not stage or unstage
  anything.
- For reads, `at-recall` and `at-peek` are the default, not the alternative. An agent shell may
  not inherit the devshell; if `which at-recall` is empty, reach it as `direnv exec . at-recall …`
  (and the same for `at-peek`). A bare `git status`, `git diff` or `git log` is a last resort:
  only after that invocation fails, and say which one you tried. `git show` and `git blame` have
  no `at-recall` verb yet — say so rather than widening the habit silently. Use `--no-pager` for
  any git read that remains.
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

The earlier wording said read-only git was fine when the inspectors were not on `PATH`. That is
true in every agent shell — the devshell is not inherited — so the exception became the rule and
`git status` stayed the first reach. The default is now the tool; git is the documented failure
path, and it is named as one rather than left as an open permission.
