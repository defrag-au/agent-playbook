# Tool names

- **Files are edited with `Read` / `Edit` / `Write`.** Never `python`, `sed`, `awk`, `perl`
  or a heredoc. See [`core/edit-via-editor-tools`](../../../rules/core/edit-via-editor-tools.md)
  for why this is a hard rule rather than a preference.
- **`Bash` for commands.** Read-only git is fine; history-mutating git is not.
- **Read-only git takes `--no-pager`** — `git --no-pager log`, not `git log`. Without it the
  command blocks waiting for a pager that will never receive input.
- **Anything that may open an editor takes a prefix**: `GIT_EDITOR=true git rebase …`,
  `PAGER=cat`, `EDITOR=true`.

## Crate references

When working with a crate that may have moved on from training data, check
`references/crates/` in the playbook for a cheat sheet before assuming an API. The
`crate-research` skill writes one when a crate is adopted.
