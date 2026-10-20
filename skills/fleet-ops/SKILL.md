---
name: fleet-ops
description: Use when inspecting, probing or rebuilding a machine in a private fleet — especially when the inventory lives outside the current repository and the question is "which host, how do I reach it, and what is its state?".
---

# Fleet operations

Use `at-reach` for the map. Do not start with an ad-hoc SSH command that re-derives what the
inventory already knows.

## The three read-only verbs

```sh
at-reach root                 # which fleet tree is in use, and how it was found
at-reach hosts                # id, role, OS/arch, Tailscale, SSH alias, writer
at-reach probe <host>         # OS, Nix/Home Manager, Tailscale, disk, repo HEAD/change count
```

`at-reach` has no `run` or `apply` verb. If those arrive, they are a separate approval, not a
flag on `probe`.

## Resolve the fleet, don't guess it

Resolution order:

1. `--fleet-root <path>`
2. `AGENT_FLEET_ROOT`
3. the current directory, if it contains `machines/`
4. `./fleet` in the current repository
5. `~/.config/agent-playbook/fleet`
6. fail

If it fails, stop and ask for the root. Do not guess a hostname, an IP or a path. The fleet tree
is private; the tool is public.

## Boundaries

- **Private inventory, public tool.** Never commit a real inventory to `agent-playbook` or a
  public host-config repository such as `nix-config`.
- **No secrets in the inventory.** No private keys, tokens, PINs, PUKs or decrypted values.
- **One writer per repo.** The profile's `writes:` field records it. A client satellite reads;
  it does not own the source of truth.
- **Per-machine identity.** Each machine has its own SSH host key, Tailscale node key and
  optional age key. Do not copy private keys or enable agent forwarding.
- **SSH aliases, not raw addresses.** The profile's `ssh:` is the alias; the user's SSH config
  and agent supply the credentials.
- **Read-only means read-only.** `probe` uses one fixed script, `BatchMode=yes`, no TTY, no
  sudo and no rebuild. `git status` may refresh the index; that is git's own write.
- **Logs stay out of the transcript.** If a probe's output is too large, redirect it to `.tmp/`
  and read the relevant part; `at-reach` itself writes no log file.

## When adding a machine

1. Add a profile under the private fleet root's `machines/`.
2. Fill the required keys; `examples/fleet/machines/example-host.md` is the fictional schema
   example.
3. Run `at-reach probe <id>` from the admin machine first.
4. Only then add the SSH alias and any Tailscale/ACL changes.
5. Do not add a second writer for a repo that already has one; use a branch and a review.
