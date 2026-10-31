# travel-secrets

A whitelisted `pass` subset, re-encrypted to a key the road machine holds.

The problem it solves is the one a copied `~/.password-store` cannot: **a store copied to a
satellite is encrypted to the wrong key**, so either the everyday secret key travels with it (a bad
idea in every direction) or the subset has to be re-encrypted for the device that will read it.
This tool does the second, without ever writing plaintext to a file.

It is deliberately separate from `fleet-sync`. Moving commits and decrypting credentials are
different approvals; a single tool that did both would need both permissions at once, and a
mistyped destination would put both payloads on the same volume without a second look.

## The shape of a run

```sh
travel-secrets \
  --store      ~/.password-store \
  --whitelist  <travel-selection>.md \
  --recipient  <TRAVEL_FPR> \
  --out        /Volumes/travel/<host>-secrets
```

```text
~/.password-store/          read-only
        │
        └── whitelisted .gpg ── gpg --decrypt ── pipe ── gpg --encrypt ──> out/store/<same path>.gpg
                                                              │
                                                              └── recipient: the travel public key

out/store/.gpg-id ── the travel fingerprint
out/manifest      ── recipient, entry count, size and SHA-256 per entry
```

Plaintext exists only between the two `gpg` processes. The source store is opened read-only, and
the output is a **self-contained pass store**: restoring it is an `rsync`, not a merge with the
old ciphertext.

## Inputs

### The store

A pass store is a directory of `.gpg` files. The tool reads those files directly and never invokes
`pass`: the manage script follows symlinks on read, writes through them on insert, and treats a
linked directory inconsistently on delete, so a selection built from `pass ls` is not necessarily
a subset of what is on disk.

A directory selected by the whitelist is walked recursively. `.git`, `.gpg-id` and `.DS_Store` are
skipped; any other file that is not `.gpg` is a refusal, not a silent skip — a stray `README` in a
selected area is a selection that needs looking at, not one to guess about.

The source's nested `.gpg-id` files are **not** copied. Every output entry is encrypted to the
travel recipient, so the output keeps one root `.gpg-id`, which is the correct id for the whole
subset. A nested id in the source also means entries beneath it may not be decryptable with the
current keyring; check before a build rather than discovering it halfway through.

### The whitelist

Flat `key: value`, the same shape as every other control file in the fleet:

```text
# commentary
pass: travel, identities/passport, finance/amex
```

- One `pass:` key. A duplicate is an error, not a merge.
- The value is comma-separated. An entry is a directory (walked) or one entry, with or without
  the `.gpg` suffix.
- `#` starts a comment only at the **start of a line**. A trailing `#` is part of the entry.
- A missing `pass:` key, an empty list or an empty entry is a usage error. An absolute path or a
  `..` segment is a refusal: the selection cannot reach outside the store.
- Unknown keys are ignored, so a travel file can carry `host:`, `repos:` and `recipient:` beside
  the whitelist and still be passed to `--whitelist` directly.

### The recipient

A full fingerprint of a public key that is already in the keyring `gpg` will use. It need not be
trusted: encryption is `--trust-model always`, because a travel key imported from a file will never
have an ownertrust setting, and refusing it for that reason would make the tool unusable on the
machine that needs it.

`gpg --with-colons --list-keys` is read to resolve the recipient:

- the fingerprint must resolve to exactly one primary key;
- a revoked, expired, invalid or disabled primary key is refused;
- the encryption key is the primary key when it carries `e`, otherwise the first usable `e`
  subkey — which is the usual case;
- no usable encryption subkey is a refusal, not a fallback to the primary.

The tool records the **primary fingerprint** in `.gpg-id` (what `pass` writes) and verifies the
output against the **encryption subkey id**, because that is the key a packet names.

### The output directory

It must be absent or empty, and not a symlink. The tool stages into `<out>/store.tmp`, checks its
work, then renames it to `<out>/store`, so an interrupted run does not leave a half store that
looks finished. The manifest is written last.

Modes are set to `0700` for directories and `0600` for files. A filesystem that cannot hold Unix
modes (exFAT, for example) fails here rather than silently producing a world-readable payload; use
APFS or ext4 on the travel volume.

## What it writes

```text
out/
├── store/
│   ├── .gpg-id                 # the travel fingerprint
│   └── <entry>.gpg             # re-encrypted, same relative path as the source
└── manifest
```

`manifest` is flat text, in this order:

```text
version: 1
recipient: <PRIMARY_FINGERPRINT>
entries: <N>

entry: travel/airline
bytes: 512
sha256: <hex>

entry: …
```

- `manifest` records what was **carried**, not what was allowed: one block per selected entry, with
  the ciphertext size and digest. It contains no plaintext and no source path.
- The report on stdout repeats the entries, then bounds the payload with the total size, the
  recipient and the exact restore command.
- Exit codes: `0` a payload was written, `1` nothing to show (not produced by this tool), `2` a
  usage error, `3` an environment failure (gpg, filesystem, unreadable source), `4` a refusal
  (outside the store, symlink, non-empty output, unusable recipient).

## The gpg contract

Two processes, one pipe:

```text
gpg --decrypt <source>  ──stdout──>  gpg --encrypt --recipient <fpr> --output <dest>
```

The encryption side is deliberately explicit:

| flag | why |
| --- | --- |
| `--no-encrypt-to` | Do not also encrypt to the keyring's default recipient. The payload must be readable by the travel key and nothing else. |
| `--no-throw-keyids` | The user's `gpg.conf` may hide recipient key ids; the output must name its recipient so it can be verified. |
| `--trust-model always` | The travel key is imported, not certified. Trust is the wrong question here; the fingerprint is the answer. |
| `--batch --yes --quiet` | No prompt for a destination, no overwrite prompt, no progress noise on stderr. |
| `--output <dest>` | Written into the staging store, never over a source file. |

Every output is then read back with `gpg --list-packets` and checked for the expected encryption
key id. That command needs no secret key, but it **exits non-zero when the keyring has no secret
key for the recipient** — it prints the key id and the failure, because it tried to decrypt. The id
is the answer, so verification looks for it in either the packet listing (`keyid <id>`) or gpg's
summary (`ID <id>`), and consults the exit status only when no id was printed. Requiring exit `0`
would reject exactly the normal case: a primary keyring holding only the travel public key.

## Safety invariants

- **No plaintext file.** The two gpg processes are connected by a pipe; the only disk writes are
  ciphertext and the manifest.
- **The source is read-only.** No `pass` verb, no `gpg --output` into the store, no `pass init -p`.
- **A symlink anywhere in a selected area is refused**, at the entry and at every file below a
  selected directory. Following one would make the output a selection of somewhere else.
- **Containment is checked twice**: the whitelist path is resolved inside the canonical store, and
  each selected file is resolved again before it is added.
- **The output is verified, not assumed.** Every file is checked for the expected recipient before
  the store is renamed into place.
- **The output directory is single-use.** A previous payload is never merged with a new one; that
  is how an entry the new selection dropped stays on the satellite without anyone noticing.
- **The terminal check is real.** The tool decrypts the source store, so it refuses a non-terminal
  stdin unless `--allow-non-tty` is explicit. An agent session must leave that flag alone.

## Failure modes

| Message contains | Cause | What to do |
| --- | --- | --- |
| `run it from an interactive terminal` | stdin is not a TTY | run it yourself, or arrange pinentry and pass `--allow-non-tty` deliberately |
| `is not empty` | the output directory already holds a payload | use a fresh directory; never merge |
| `missing pass: key` | the whitelist has no `pass:` key | fix the selection; it is not a default-all |
| `empty entry` | a stray comma in the list | fix the selection |
| `is not a directory or a .gpg entry` | the name selects nothing | check the spelling and the store |
| `is a symlink` | a link in the selected area | resolve it in the store first; do not carry links |
| `is not a .gpg entry` | a non-ciphertext file inside a selected directory | remove it from the area or narrow the selection |
| `resolves to N keys` | the recipient is ambiguous | use one full fingerprint |
| `no usable encryption subkey` | the key cannot encrypt | generate or add an encryption subkey |
| `revoked, expired, invalid or disabled` | the key is not usable | rotate the travel key |
| `gpg could not decrypt` | the current keyring cannot open a source entry | check `GNUPGHOME`, the key, and the entry |
| `gpg could not encrypt` | the recipient failed at write time | check the key and the output filesystem |
| `not encrypted to the expected recipient key` | the packet does not name the travel key | check `--no-throw-keyids` and the recipient |
| `cannot set mode` | the filesystem cannot hold Unix modes | use APFS or ext4 |

## Tests

```sh
cargo test -p travel-secrets --lib          # parsing, selection, manifest, recipient records
cargo test -p travel-secrets --test encrypt # the gpg round trip
```

The gpg test generates throwaway passphrase-less keys in isolated temporary `GNUPGHOMEs` under
`/tmp`, imports only the travel **public** key into the primary home, sets `throw-keyids` to
exercise the override, and asserts that each output decrypts with the travel key and fails with the
primary. It never reads `~/.password-store` or the real keyring.

`/tmp` is chosen over the session's `TMPDIR` on purpose: gpg-agent binds a Unix socket under
`GNUPGHOME`, and macOS's `sun_path` is 104 bytes; a long per-session TMPDIR overflows it before any
key is generated.

**An agent must not run the gpg test, `pass`, or `gpg --decrypt` against a real keyring.** It is
the human's command; the agent prepares and reads the manifest.

## Relationship to the fleet

The tool is machine-independent: it takes a store, a whitelist, a recipient and an output path. It
does not read a fleet profile, does not contact a host, and has no SSH path.

The per-trip selection and the procedure live in the private `fleet` repository:

- `fleet/travel/<host>.md` — the `pass:` whitelist, plus the recipient fingerprint for the trip;
- `fleet/skills/travel-secrets/SKILL.md` — key generation on the satellite, build, restore, teardown;
- `fleet/travel/README.md` — what a selection is for, and its lifetime.

A **public** key or its fingerprint is not a secret and may be committed there. The secret half is
generated on the satellite and never leaves it; do not copy `~/.gnupg`, and never commit an
exported secret key.

This is a deliberate, temporary subset for one device — not a replacement for a finer-grained
password system.
