# Fleet operations

`at-reach` is the read-only fleet tool. It answers three questions:

```sh
at-reach root                         # which fleet tree am I reading?
at-reach hosts                        # what machines are in it?
at-reach probe <host>                 # what is that machine doing?
```

It is deliberately small. It has no `run`, no `apply`, no sudo path and no secret access. A
write verb, if it is added later, belongs behind a separate approval.

## The split: public tool, private inventory

This repository is public. A real fleet inventory names hosts, SSH aliases, Tailscale nodes,
repo paths and rebuild commands, so it belongs in a **private** repository, for example:

```text
~/code/<org>/fleet/
  machines/
    example-host.md
  tailscale/
    README.md
```

That tree is data. `at-reach` only reads it. The public repository ships the tool, the schema,
a fictional example in `examples/fleet/`, and the `fleet-ops` skill. Do not commit a real
inventory here, and do not put one in a public host-config repository either.

The inventory is **discovery, not authorization**. Knowing a host exists does not grant access;
SSH keys, Tailscale ACLs and host configuration do.

## Resolution

`at-reach` resolves the fleet root in this order:

1. `--fleet-root <path>`
2. `AGENT_FLEET_ROOT`
3. the current directory, if it contains `machines/`
4. `./fleet` in the current directory
5. `~/.config/agent-playbook/fleet`
6. fail with an error naming those options

The path names the directory that contains `machines/`, not `machines/` itself.

Start with the inventory on the admin machine only. Satellites do not need a map of the fleet
unless an agent running on them actually needs to reach another host. The machines do not
communicate through this repository.

## Profile format

A profile is flat `key: value` text, the same shape as a rule's frontmatter. Blank lines and
lines beginning `#` are ignored. Duplicate keys are an error.

```text
id: example-host
role: client
os: linux
arch: x86_64
nix_system: x86_64-linux
ssh: example-host
tailscale: example-host
tailscale_ip: 100.64.0.10
tailscale_tag: tag:client
shell: /bin/zsh
sudo: required
repo_root: /home/user/code
repos: example-org/nix-config,example-org/shared-crates
rebuild_repo: example-org/nix-config
rebuild: home-manager switch --flake .#user@example-host
writes: none
notes: Example only.
```

Required keys: `id`, `role`, `os`, `arch`, `ssh`, `sudo`, `rebuild`, `writes`.

- `role` is `admin`, `build` or `client`.
- `repos` is comma-separated. Each entry is relative to `repo_root` unless it starts with `/`.
- `rebuild` is run from `repo_root/rebuild_repo`, or from `rebuild_repo` when that is absolute.
  It is documentation for now; `at-reach` does not run it.
- `sudo` is `none`, `passwordless` or `required`. `required` means a future apply verb must not
  attempt a non-interactive rebuild.
- `tailscale` is the desired MagicDNS name; `tailscale_ip` is the fallback while a name is
  being corrected in the admin console.
- `writes` records the repo(s) this host is the authoritative writer for. One writer per repo.

## What `probe` does

`probe` runs one fixed POSIX shell script. If the profile's `id` matches the local hostname, it
runs with `sh`. Otherwise it runs:

```sh
ssh -T -o BatchMode=yes -o ConnectTimeout=5 \
    -o ServerAliveInterval=15 -o ServerAliveCountMax=2 <ssh> -- <probe>
```

The script reports:

- hostname, OS and architecture
- Nix and Home Manager versions
- the Tailscale IP and first status line
- free disk space on `/`
- for each repo: `head=<short sha> changes=<porcelain count>`, or `missing`

It never uses sudo, never runs a rebuild and never reads a secret. `git status` may refresh the
index stat cache; that is git's own write, not the tool's.

The output follows the `at-*` contract: content lines are the facts, and `#` lines are the
frame — which fleet root was read, whether the probe was local or SSH, and any stderr caveat.

## Failure behaviour

- No fleet root: fails closed and names the four resolution options.
- Missing `machines/`: fails rather than pretending the fleet is empty.
- Duplicate `id`: fails and names both files.
- Missing required key or duplicate key: fails with the file and line.
- Unknown host: fails and lists the known ids.
- SSH failure: fails with the exit status and the first stderr line.

No hostname or IP is guessed. No profile is created. No secret is read.

## Secrets

The inventory is private because it describes the fleet, not because it contains secrets. Do
not put private keys, tokens, PINs, PUKs, decrypted passwords or wallet material in it. Those
belong in the YubiKey-backed tier or `sops`.

## Future write verbs

`run` and `apply` are not part of this tool. If they are added, they should be a separate
approval: host allowlist from the inventory, per-host lock, preflight for dirty Git trees and
staged Nix files, postflight for services and generations, and no password prompt. A read-only
probe should never be able to become a write by adding a flag.
