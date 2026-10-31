---
name: at-cardano
description: Use when asking a Cardano chain question — a transaction, address, account, asset, policy, block, epoch, pool, DRep or governance action — with at-cardano. Covers the noun/aspect grammar, the key file and rate tiers, target and paging rules, the scenario file for questions that take several answers, and the Koios traps that read like a bad identifier.
---

# Asking the Cardano chain with at-cardano

The situation that should have brought you here: you are about to `curl https://api.koios.rest/…`,
retype a JSON request body, or put an API key on a command line. Don't. `at-cardano` is the
read-only Cardano client in the `at-*` toolkit: one fixed Koios host per network, GET and
query-POST only, `/submittx` refused by name, and a key read from a file that is never printed.

## Discover it

Every command here is local — no request, no rate limit:

```sh
at-cardano                                  # 29 nouns, one line each, plus exit codes
at-cardano help pool                        # that noun's aspects, with usage
at-cardano help pool delegators             # that aspect's flags and notes
at-cardano pool --help                      # the same as `help pool`
at-cardano tip --help                       # a one-aspect noun's flags
at-cardano endpoints                        # every Koios path → the verb that answers it
at-cardano endpoints tx                     # filter by path, verb or target kind
at-cardano auth                             # endpoint and key source, without a request
at-cardano run --help                       # the scenario actions
```

The grammar is **noun + aspect**: the noun is what you are asking about, the aspect is what you
want from it. `at-cardano pool delegators <pool>` is one question with one bounded answer. The bare
noun asks its principal question (`at-cardano pool` is `pool info`).

## The mapping

```sh
# the chain head, and the network's constants
at-cardano tip
at-cardano network params
at-cardano network genesis
at-cardano network eras
at-cardano epoch 500
at-cardano epoch blocks 500

# a block — by height or by hash
at-cardano block 14100188
at-cardano block <hash>
at-cardano block txs <hash>
at-cardano block tx-info <hash>
at-cardano block cbor <hash>

# a transaction
at-cardano tx <hash>
at-cardano tx utxos <hash>
at-cardano tx cbor <hash>
at-cardano tx metadata <hash>
at-cardano tx status <hash>
at-cardano tx metalabels
at-cardano tx by-metalabel <label>

# an address, or a stake account
at-cardano address <addr>
at-cardano address utxos <addr>
at-cardano address txs <addr>
at-cardano address assets <addr>
at-cardano account <stake>
at-cardano account rewards <stake>
at-cardano account stake-history <stake>
at-cardano account txs <stake>

# an asset, its policy, or a pool
at-cardano asset <policy.name>
at-cardano asset addresses <policy.name>
at-cardano asset history <policy.name>
at-cardano policy <policy>
at-cardano policy mints <policy>
at-cardano pool <pool>
at-cardano pool delegators <pool>
at-cardano pool history <pool>
at-cardano pool relays <pool>
at-cardano pool metadata <pool>

# governance
at-cardano dreps
at-cardano drep <drep>
at-cardano drep delegators <drep>
at-cardano committee
at-cardano proposal summary <gov_action>
at-cardano proposal votes <gov_action>
at-cardano votes
at-cardano proposals

# a script or a datum
at-cardano script <hash>
at-cardano script native
at-cardano script redeemers <hash>
at-cardano datum <hash>
```

If you know the Koios path but not the noun, `at-cardano endpoints <word>` answers with the verb
that covers it. `at-cardano query <endpoint>` is the long tail for anything else; it cannot choose
a host, add a header or follow a redirect.

## Targets and paging

- Targets are validated by kind: addresses begin `addr`, stake addresses `stake`, pools `pool1`,
  a 28-byte hash is 56 hex characters, a 32-byte hash is 64, an asset is `policy.name` (or
  `policy.hexname`, or a bare 56-character policy), and a UTxO is `tx_hash#index`.
- Where an endpoint batches, pass several targets in one invocation: `tx <hash>…`,
  `address <addr>…`, `pool <pool>…`, `utxo <ref>…`. A block batch must be all hashes or all
  heights, and a height is one target.
- `--limit` is rows to ask for (default 20, ceiling 1000), `--offset` skips, `--all` follows pages
  at the 1000-row ceiling up to ten pages, and both `--order col.desc` and `--select col,…` are
  passed to Koios. A full page ends with a `# next:` command; run it as printed.
- `--summary` prints the frame without the rows. `--json` prints the response body, bounded to
  64 KiB. `--network mainnet|preprod|preview|guild` chooses the host; `--keyless` ignores a key.
- A nested list inside one record — an account's reward history, a script's redeemers, a pool's
  relays — is **not** bounded by `--limit`: the limit bounds the records. Up to 100 lines are
  printed and the cut is stated; narrow with `--epoch` or one target, or take `--json`.

## The key

`KOIOS_API_KEY` is read first, then `$XDG_CONFIG_HOME/koios/api_key` (usually
`~/.config/koios/api_key`). The header on every answer names the source — `key from
~/.config/koios/api_key`, `keyless (no …)`, `keyless (--keyless)` — and never the value. A key
file readable by other accounts is reported with a `chmod 600` caveat.

`at-cardano auth` prints the endpoint and the key source without making a request. It is the
first command after an unexpectedly rate-limited run: HTTP 429 means the key was missing or over
its tier, and a 401/403 means the key was rejected (retry with `--keyless` to separate the two).

## Questions that take several answers

One verb answers one question. When the next question depends on a value inside the previous
answer — the tip's height into a block lookup, that block's first transaction into `tx` — write a
scenario file and run it:

```text
# the chain head, the block at that height, and a transaction in it
run network=mainnet timeout=90s

target tip tip
save tip.0.block_height as height
target block block ${height}
save block.0.hash as block_hash
target txs block txs ${block_hash} limit=3
save txs.first.tx_hash as first_tx
target one tx ${first_tx}

assert block count=1
assert txs count>0 message="the head block carries transactions"
assert one contains="fee"
echo "block ${block_hash} · first tx ${first_tx}"
```

```sh
at-cardano run --check --scenario .tmp/block.dsl   # parse and resolve; no request, no key
at-cardano run --scenario .tmp/block.dsl
```

The five actions are `run` (network, keyless, timeout), `target <name> <noun> [aspect]
[positionals…] [fields…]`, `save <target>.<row>.<column> as <name>`, `assert <name> count=…
| count>0 | contains="…" | absent="…"`, and `echo`. A target's fields are that verb's own flags
without the leading `--` (`limit=`, `offset=`, `order=`, `select=`, `epoch=`, `all`, `summary`,
`history`, `post`, `body=`, `query=`).

`--check` is the one to run first and in CI: it validates the whole file — nouns, aspects, every
literal target, every field, every `save` target and every `${name}` — then stops. A parse error is
exit 2 with the file, line and column, before any request.

**Obtaining an answer is fatal; checking one is not.** A target that cannot be answered stops the
run with that failure. A failed `save` or `assert` is recorded and the run continues, so one bad
expectation does not hide the four after it. Exit 0 means every check passed, 2 means the file is
wrong or a check failed, 3 means the chain could not answer or the run timed out, and 4 means the
tool refused.

## Output and exits

A `# ` line is metadata: the header names the request, and the bounds name what was not shown. All
other lines are the answer. `# next:` is a command that continues the question just asked; run it
rather than composing a narrower pipeline. Exit 1 means the read succeeded and found nothing — an
unknown identifier or an indexer that has not caught up — and is not an error.

## Traps

- **A block height is resolved to a hash first.** `block` and its aspects take hashes; the tool
  turns a height into one with a `/blocks?block_height=eq.…` request. A height ahead of the
  indexer is an error naming the height, not an empty answer.
- **`--limit` on a non-paged verb is refused.** If the verb has no `--limit` in its flags, the
  answer is already one row; do not reach for `--all` to make it a list.
- **`account txs` is a recent window, not a page.** The endpoint takes one stake address and has no
  offset, so `--offset` cannot walk it; use the account's other aspects or `--json`.
- **`tx status` carries only the hash and the confirmation count.** It is not a receipt; the fee,
  outputs and metadata are `tx`.
- **Pool `margin` is printed as a percentage** (the wire value is a fraction), and
  `live_saturation` is already a percentage. Do not multiply either again.
- **`block cbor` names the byte lengths; `tx cbor` prints the hex.** The latter is capped at
  256 KiB with the cut stated.
- **Four spec paths are left to `query` because Koios does not answer them**: the Ogmios proxy,
  `/tx_outs_epoch` (a 504 after two minutes), and `/reserve_withdrawals` /
  `/treasury_withdrawals` (404 on mainnet). A verb would only wrap the failure; do not retry them.
- **A keyless run is rate-limited.** A burst of scenarios without a key is the usual cause of a
  429. `at-cardano auth` says which tier you are on before you blame the endpoint.
- **`at-cardano` may not be on PATH in a spawned shell.** An interactive shell loads the devshell;
  an agent's does not. Use `direnv exec . at-cardano …`, and never fall back to `curl`.
- **The binary was called `at-chain` before the rename.** A devshell that still exposes only
  `at-chain` is behind its `agent-playbook` re-lock; use the `at-cardano` the repository builds
  rather than the stale name.

## What it will not do

It writes nothing, spawns nothing, follows no redirect, and never submits a transaction. A
`query` names one Koios endpoint, not a URL. So a grant over `at-cardano` is a grant over "ask
Koios a read question", and a scenario cannot widen it: every target is a catalogue verb, and
`/submittx` is refused while the file is parsed.
