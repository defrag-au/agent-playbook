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

That tree is data. `at-reach` only reads it. The public repository ships the tool and the schema,
with a fictional example in `examples/fleet/`; the `fleet-ops` skill lives in the fleet repository
itself, in `skills/fleet-ops/`, published into `fleet/.skill-store/`. Do not commit a real
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
sync: bundle
sync_clean: artifacts
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
- `sync` is optional: `bundle` (the default) makes the host a `fleet-sync` target, and `none`
  opts it out. A host with `writes` other than `none` is not a target even when this says
  `bundle`; `fleet-sync apply` refuses it without `--allow-writer`.
- `sync_clean` is optional: what `fleet-sync apply` may remove in a checkout — `artifacts` (the
  default), `reset` (artifacts plus `git clean -xfd`, protecting `.env`) or `none`.

## Sync to a satellite

`fleet-sync` is the write-side counterpart, and it is a **separate binary and package**:
`at-reach` stays read-only, and a read-only probe can never become a write by adding a flag.

```sh
fleet-sync bundle <host> --out <dir>   # on the machine that has the work
fleet-sync plan   <host> --from <dir>  # on the target: what apply would do
fleet-sync apply  <host> --from <dir>  # on the target: clean and reset
```

`bundle` reads the profile's repos against `repo_root` (or `--source-root`), writes one `.bundle`
per repo and a flat `manifest` carrying the host, ref, commit, size and SHA-256. It carries
commits, not working-tree changes; a dirty source checkout is reported, not copied. The target is
not contacted.

`plan` and `apply` resolve the same profile on the target and refuse unless the local hostname
matches the profile's `id` and `writes` is `none` (or `--allow-writer`). There is no SSH
path in either direction. Before writing, every bundle's size and digest is verified; a checkout
with tracked changes is refused until `--force`; and the plan refuses when the payload plus 256
MiB of headroom will not fit after the clean. The artifact list is fixed: `target`, `.direnv`,
`node_modules`, `.tmp`, `.venv`. A symlinked artifact is left alone.

The payload contains git history, so a private repository's bundle belongs on an encrypted
volume. The password store is deliberately not part of this tool.

### Credentials

`travel-secrets` builds the other payload for the same trip, and is a separate binary for the
same reason: re-encrypting a password store is a different approval from moving commits.

```sh
travel-secrets --store ~/.password-store --whitelist <selection> \
               --recipient <travel-fingerprint> --out <dir>
```

The selection is a flat file whose `pass:` line names areas or individual entries under the
store. The tool decrypts each one with the current keyring, re-encrypts it to the travel
recipient through a pipe, and writes a self-contained store plus a manifest of entry, size and
SHA-256. Plaintext is never written to disk, symlinks are refused, and the source store is never
modified. The root `.gpg-id` of the output is the travel fingerprint; nested ids are not copied.

The selection lives in `fleet/travel/<host>.md` and is built for one trip. It names paths, never
values, and it does not make the satellite a writer: the travel key's secret half is generated on
the satellite, and only its public half is imported on the admin machine.

**An agent must not run `travel-secrets`, `pass` or `gpg --decrypt` against a real keyring.**
Preparing the selection and reading the manifest are agent work; the command is the human's.

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

## Write verbs

`run` and `apply` are not part of `at-reach`, and that has not changed: `fleet-sync` is the
separate tool that writes. It takes its host from the inventory, refuses to run anywhere but the
named target, refuses a dirty checkout without `--force`, verifies every bundle before touching a
checkout, and does not prompt for a password. A read-only probe cannot become a write by adding a
flag. `travel-secrets` is a third binary for the credential half, not a flag on either of these.

A future remote push would be another tool with its own approval, not a flag on any of these.
