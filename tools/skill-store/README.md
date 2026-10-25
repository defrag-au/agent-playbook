# skill-store

An OCFL store for agent skills, and the tool that publishes into one and exports out of one.

The problem it solves is the one the composer solved for rules: **a skill that only exists as a
copy in `~/.agents/skills` has no version, no digest and no provenance**, so "which `inspect-code`
is this session running?" has no answer. Here a skill is an OCFL object with immutable version
directories, `sha512` content addressing and an inventory digest — and a consumer records the
version and tree digest it delivered.

Two consequences, and they are the whole design:

- **Artifact ≠ delivery.** The store is the artifact: immutable, verifiable, shareable. The export
  is the delivery: a plain directory an agent can read, plus a flat manifest. Nothing is
  installed by being published, and nothing is published by being installed.
- **Author in plain directories; publish to make a version.** OCFL is never the editing surface.
  A version is a release, not a keystroke.

## Why a sibling binary, not part of `playbook`

`playbook` is std-only, has no dependencies, and never touches the network — that is a property,
not an accident: it is the tool you reach for when a repository's toolchain is what is broken.
`skill-store` hashes, parses JSON, reads YAML-ish frontmatter and runs `git`. Those cannot both
be true of one crate, so `playbook skills sync` **calls** `skill-store export` and reads the
envelope it writes.

For the same reason it is **not** in `agent-tools`: that derivation's documented contract is
read-only binaries with no write path and no network. `skill-store` writes a store and fetches
from git.

## Store layout

A store is a directory holding `0=ocfl_1.1` whose body is `ocfl_1.1`. One object per skill:

```text
store/
├── 0=ocfl_1.1                     # "ocfl_1.1"
├── widget-screenshot/
│   ├── 0=ocfl_object_1.1          # "ocfl_object_1.1"
│   ├── inventory.json             # OCFL 1.1 inventory, head = v2
│   ├── inventory.json.sha512      # "<digest> inventory.json"
│   ├── v1/
│   │   ├── inventory.json         # snapshot: head = v1
│   │   └── content/SKILL.md
│   └── v2/
│       └── content/SKILL.md       # only what changed from v1
└── container-compose/…
```

- Object id is `urn:skill:<name>`. The object directory is `<name>`, and publishing refuses a
  directory whose `SKILL.md` names something else.
- Logical paths are the skill's own: `SKILL.md`, `references/…`. There is no `skill/` prefix
  inside the store; the prefix exists only in an export.
- Content is deduplicated by digest across versions, so a version that changes one sentence
  stores one file.
- The object-root inventory is written **last**, so an interrupted publish shows as a missing
  head rather than as a version that looks complete.

**A store is either the directory with `0=ocfl_1.1` or a `.skill-store/` inside it.** That is what
lets a tool repository carry its skill next to its code without a `?dir=`:

```text
container-compose/
├── crates/
└── .skill-store/
    ├── 0=ocfl_1.1
    └── container-compose/…
```

## Source grammar

Flake-shaped, deliberately. Nix already has the grammar everyone has muscle memory for, and a
second URI scheme would be one more thing to learn for no new capability.

```text
<store>#<skill>[@v<N>]
```

| Written | Resolves to |
| --- | --- |
| `github:defrag-au/container-compose#container-compose@v1` | `https://github.com/defrag-au/container-compose.git`, default branch, then `.skill-store/` |
| `github:defrag-au/container-compose?rev=<sha>#container-compose@v1` | the same, pinned to a commit |
| `github:defrag-au/skill-store/main?dir=store#widget-screenshot@v2` | a branch in the path, an explicit store directory |
| `git+https://example.org/skills.git?rev=<sha>#inspect-code@v3` | any git host |
| `path:~/code/defrag/skill-store#widget-screenshot@v2` | a local store, no network, no cache |
| `path:~/code/defrag/container-compose#container-compose@v1` | a local repository, `.skill-store/` found automatically |

Rules that fail loudly rather than guessing:

- `github://` is normalised to `github:` (a flake ref has one colon).
- A bare `https://` is **rejected** — in a flake ref that means a tarball. Use `git+https://`.
- `?ref=` and `?rev=` together, or a ref given both in the path and in the query, is an error.
- An unknown query parameter is an error. Supported: `ref`, `rev`, `dir`.
- `@vN` names an OCFL version directory exactly: `v` followed by a positive integer, no leading
  zero.
- Omitting `@vN` selects the object's **head**. A consumer that cares records the version
  explicitly; a lock that says "head" is not a lock.

## Commands

```sh
skill-store export <source> --out <dir>       # one version → envelope
skill-store list <source>                     # every object, with head and description
skill-store verify <source> [<skill>]         # inventories, digests, content paths
skill-store status --store <dir> --worktree <dir>   # is the head still what the directory says?
skill-store publish <dir> --store <path>      # plain directory → a new version
skill-store publish-changed --store <dir> --worktree <dir>  # publish every skill that moved
skill-store source <source>                   # how a source resolves (debugging)
```

Options: `--out`, `--store`, `--worktree`, `--id`, `--message`, `--user`, `--address`, `--skill`,
`--version`, `--init`, `--offline`, `-h`.

Exit codes: `0` success, `1` an operation failed, `2` a usage error (bad source, missing flag).

`export` also accepts a `path:` to a **plain skill directory** — no store, no inventory, no
version. That is what a development override points at: the envelope is the same, with
`version: working` and the digest taken over the copied tree. `working` is not a version directory
and the source grammar rejects `@working`, so nothing can pin one; the next `publish` is what
makes an edit permanent.

## Is a publish owed?

`status` is the read-only half of `publish`, and the command a pull request runs:

```sh
skill-store status --store .skill-store --worktree skills
```

```text
current     capture-rule v1
current     crate-research v1
ahead       browser-shot v1 — 1 changed
missing     fleet-ops — no object
store-only  orphan v2 — no directory
```

It exits non-zero on anything but `current`, and both directions count. `ahead` is an edit nobody
published — the version is immutable, so that work exists only on the machine that made it.
`missing` is a directory nobody can consume. `store-only` is an object nobody can edit, which is
what a repository looks like when it inherited a store without the directory it is published
from.

It compares bytes against the head's `state` and `manifest` directly, so it never writes and needs
no export directory.

## Publishing everything that moved

`status` tells you a publish is owed; `publish-changed` does it, for every skill at once:

```sh
skill-store publish-changed --store .skill-store --worktree skills
```

```text
published urn:skill:browser-shot v2 (+0 ~1 -0 =1)
nothing to publish
```

- It publishes exactly the skills that are `ahead` or `missing`, so an unchanged skill is never
  given an empty version — and running it twice publishes once.
- With nothing to do it prints `nothing to publish` and exits `0`, which is what makes it safe as
  a routine step before a commit and as a CI check.
- A `store-only` object is reported and fails the command: there is no directory to publish from,
  so it is the one drift this cannot repair.
- `--message`, `--user` and `--address` apply to every version it writes. `--id` is refused — one
  id cannot name several skills.

## Authoring and publishing

The working copy is a plain directory — the same shape an agent reads:

```text
widget-screenshot/
├── SKILL.md
└── references/…
```

```sh
skill-store publish path/to/widget-screenshot --store .skill-store --init \
  --message "first published version"
```

`SKILL.md` must open with frontmatter carrying `name` and `description`, and `name` must be the
directory name. Publishing content identical to the head is refused: a version that changes
nothing is a mistake, not a release. `.git` and `.DS_Store` are skipped; a symlink anywhere in
the tree is refused, because OCFL forbids them in a storage hierarchy and a skill containing one
is not portable.

No `user` block is invented. OCFL wants one (W007b) and, when present, wants it to carry an
address (W008) that begins with a scheme (W009) — so the block records the **repository the store
lives in**: the name from `git config user.name`, the address from `origin`, normalised to
`https://`. `--user` and `--address` override both. A store outside a git checkout has no address
to give and records nothing, which is why its inventories carry W007b: a warning is the honest
answer where a value would be invented.

Then verify and publish the repository as usual — the store is committed with the code, which is
what makes `github:defrag-au/container-compose#container-compose@v1` resolvable.

## The export envelope

`export` writes a **directory**, not the skill itself. This is the contract `playbook` reads:

```text
out/
├── export.conf      # flat: key: value, one per line
└── skill/           # SKILL.md, references/… — exactly what an agent reads
```

`export.conf`, in this order:

| Key | Meaning |
| --- | --- |
| `name` | the skill name, from `SKILL.md` |
| `id` | the object id, `urn:skill:<name>` |
| `version` | the version exported, e.g. `v2` |
| `source` | the store as the consumer wrote it, fragment removed |
| `store_rev` | the commit the store was read at, for a git source |
| `inventory_digest` | `<algorithm>:<hex>` of the object's `inventory.json` |
| `tree_digest` | `sha256:<hex>` over the sorted `(logical path, file digest)` pairs |
| `files` | how many files the version holds |

`tree_digest` is ours and is independent of the store's OCFL digest algorithm, so two exports can
be compared without knowing which store produced them. It is the value a consumer puts in its
lock: same tree, same digest, whichever version directory it came from.

The format is flat `key: value` on purpose — the composer parses it with the same reader it uses
for every other config file, so `playbook` needs no JSON parser to consume an OCFL store.

Export writes into a directory that does not exist or is empty; re-exporting over a previous
export is refused rather than merged. Every digest is re-checked on the way out, so an export that
succeeds has hashed every byte it copied.

## Verifying

`verify` is not a full OCFL validator — it checks the part that makes an export trustworthy:

- the `inventory.json.sha512` sidecar against the inventory's own bytes;
- that versions are `v1…vN`, contiguous, with `head` naming the highest;
- that every digest in a version's `state` is in the `manifest`;
- that every manifest content path hashes to its key;
- that no content file in a version's `content/` is unaccounted for in the manifest;
- that a per-version `inventory.json` snapshot agrees with its own directory.

Fixity blocks and object extensions are read and ignored. A static HTTP store would need a
declared `ocfl_layout.json`; object lookup walks for `0=ocfl_object_1.1` instead, which is every
store this tool reads today.

## Cache

A git source is fetched once into a full clone per URL, keyed by the URL. A skill store is small
and every OCFL version lives in the working tree, so a full clone answers any version without a
second request.

```
$SKILL_STORE_CACHE      # explicit
$XDG_CACHE_HOME/skill-store
$HOME/.cache/skill-store
```

`--offline` uses the cache and never fetches; a rev or ref missing from it is an error naming
that, rather than a silent fallback to the head.

## Not this tool's job

Installing a skill into an agent's layout — `~/.agents/skills/` for Zed, `.claude/skills/` for
Claude Code, whatever the next harness reads — is `playbook skills sync`. It calls this binary,
reads the envelope, copies `skill/` into place and records the version and `tree_digest` in
`skills.lock`. Keeping the split means the store has one reader and one writer, and the
target-aware decisions live where every other target-aware decision lives.
