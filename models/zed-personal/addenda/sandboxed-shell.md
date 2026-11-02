# The sandboxed shell

Zed's agent terminal runs sandboxed by default. The restrictions are not errors to work
around — they are boundaries to recognise early, because each one presents as something else.

## No shell substitution in permission-protected commands

`$VAR`, `${VAR}`, `$(...)`, backticks, `$((...))`, `<(...)` and `>(...)` are rejected in any
command that needs approval. A loop like `for f in ~/x/*/SKILL.md; do cat "$f"; done` will be
refused outright. Resolve the values first, or pass the files as literal arguments:
`cat ~/x/a.md ~/x/b.md`.

## Git metadata is never writable

`.git` is read-only while sandboxed — no `commit`, no `branch`, no `stash`. That is consistent
with [`rules/core/git-is-the-users-domain`](../../../rules/core/git-is-the-users-domain.md):
history is the user's. Use `git --no-optional-locks status` and prefer read-only git flags
that avoid optional metadata writes.

## The nix daemon socket is refused

`nix develop` fails with `cannot connect to socket … Operation not permitted`, which reads
like a permissions problem and is actually the sandbox. Use `direnv exec . <cmd>` instead —
it reads the already-realised devshell out of `.direnv/` and needs no daemon. See
[`rules/rust/devshell-first`](../../../rules/rust/devshell-first.md).

## A spawned shell does not inherit the devshell

`direnv` loads the devshell on `cd`, which only happens in an interactive shell. Commands an agent
spawns run without it, so a tool that is "on `PATH` in the repo" is not on `PATH` for the agent —
`which <tool>` returns nothing, and the failure reads as "not installed". Reach for it explicitly
with `direnv exec . <tool> …` (which needs no daemon), or install it into the nix profile so it is
on `PATH` unconditionally.

## Network access is per-host

Outbound network is blocked by default. Access is granted per host through an HTTP/HTTPS
proxy, so `https://` URLs work where `git@`/`ssh://` do not. Request the specific hosts a
command needs rather than reaching for blanket access — the user sees and approves the list.

## Writes outside the project

Writable by default: the project roots and a per-thread temp directory (`$TMPDIR`). Anything
else needs an explicit grant. Prefer enumerating the paths over requesting blanket write
access.
