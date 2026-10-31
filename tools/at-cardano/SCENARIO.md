# Scenario DSL

`at-cardano run --scenario <file.dsl>` runs a sequence of chain questions, carries a value out of
one answer into the next, and checks what came back. It is the same shape as `browser-shot`'s
and `tui-shot`'s scenario files: one action per line, positional names first, then `key=value`
fields. The whole file is parsed before anything runs, so a misspelled noun, a field the verb
does not take, or a reference to a value no `save` binds fails while the line is being read —
with the file, line and column.

The reason it exists is the second half of that sentence. A shell script can loop over `curl`,
but carrying a hash from one answer into the next means parsing output with `jq` or retyping
it, and neither is reviewable. A scenario says `save txs.last.tx_hash as prev` and then uses
`${prev}` as a target.

## Grammar

```ebnf
file       = { line } ;
line       = blank | comment | run | target | save | assert | echo ;
run        = "run" { field } ;
target     = "target" name noun [ aspect ] { positional } { field } ;
save       = "save" path "as" name ;
assert     = "assert" name { check } ;
echo       = value { value } ;
path       = name "." row "." column ;
row        = "first" | "last" | digit { digit } ;
check      = "count" operator number | "contains=" value | "absent=" value
           | "message=" value ;
operator   = "=" | ">" | "<" | ">=" | "<=" ;
field      = key "=" value | switch ;
```

A line starts with the action. Every remaining token is either positional (a name, a noun, an
aspect, a target value) or a field. A token containing `=` is a field; quote a positional that
contains one. Values may be single- or double-quoted, and a quoted value may contain spaces and
`=`. Quote every value that contains whitespace or `=`. A `#` at the start of a token begins a
comment; a `#` inside an unquoted token is kept.

Escapes inside a quoted value: `\n`, `\t`, `\\`, `\"`, `\'`. An unknown escape is an error.

Durations are milliseconds by default; `ms` and `s` suffixes are accepted (`timeout=90s`,
`timeout=500ms`). Bare integers only.

## Actions

`run` — at most once, before any step. Everything it does not set is defaulted.

| Field | Meaning |
| --- | --- |
| `network=mainnet` | `mainnet` (default), `preprod`, `preview` or `guild` |
| `keyless` / `keyless=true` | ignore a configured key and use the free tier |
| `timeout=120s` | the whole run's bound, checked before each step (default `120s`, cap `600s`) |

`--network` on the command line overrides the file's `network=`, and the run says so.

`target` — ask a question through a catalogue verb. The name is what later steps refer to.

```text
target <name> <noun> [<aspect>] [positionals…] [fields…]
```

The noun and aspect are the command line's: `target txs block txs ${hash}` is
`at-cardano block txs <hash>`. The bare noun means its principal question, so `target tip tip` is
`at-cardano tip`. A noun without a principal question needs its aspect (`target p proposal summary`).
The verb's own flags are the fields; the `--` is dropped and the value is written after `=`:

| Field | Verbs that take it | Meaning |
| --- | --- | --- |
| `limit=100` | paged lists | rows to ask for (default 20, ceiling 1000) |
| `offset=100` | paged lists | rows to skip |
| `order=block_time.desc` | paged lists | PostgREST ordering |
| `select=tx_hash,block_height` | paged lists | columns to ask Koios for |
| `all` / `all=true` | paged lists | follow pages to the ten-page cap |
| `summary` / `summary=true` | paged lists | the frame without the rows |
| `epoch=500` | history verbs | the endpoint's `_epoch_no` filter |
| `history` / `history=true` | `asset txs` | include every historical transaction |
| `no-extended` | paged lists | omit inline datums and script references |
| `no-inputs`, `no-assets`, `scripts` | `tx` | what `/tx_info` includes |
| `post`, `body="…"`, `query=k=v` | `query` | the long tail's request |

A field the verb does not take is an error naming the fields it does; a literal value is judged
there and then (`limit=zero` fails while the line is read). `network`, `keyless`, `json` and
`body-file` are not target fields: the run sets the first two, a target is always printed the
way the tool prints it, and a scenario reads only its own file. `auth`, `endpoints` and `run`
cannot be targets — they answer nothing the chain was asked.

`save` — bind one value out of an earlier answer.

```text
save <target>.<row>.<column> as <name>
```

`row` is an index, `first` or `last`; `column` is a field of the answer's records (`tx_hash`,
`block_height`, `stake_address`). The value is bound as a string: a JSON string as itself, a
number as its digits, a boolean as `true`/`false`. A `null`, a list or a record is refused —
name a scalar column. The target must already be declared, and the name must not already be
bound. Use it as `${name}` in a later positional, field value, check or echo.

`assert` — check an earlier answer. One line may carry several checks; all must hold.

| Check | Meaning |
| --- | --- |
| `count=3`, `count>0`, `count<10`, `count>=1`, `count<=5` | rows Koios answered |
| `contains="fee"` | the printed answer contains this text |
| `absent="error"` | the printed answer does not contain it |
| `message="…"` | printed with each failure of this line |

`contains` and `absent` read the answer's content lines — exactly what the tool would print,
without the `# ` metadata — so they test the answer a reviewer sees. They are matched as plain
text, not as JSON.

`echo` — print a line, with `${name}` substituted. One line of the scenario's own words.

## Example

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

Each target prints its answer exactly as `at-cardano <verb>` would print it — its own header, its
own row count, its own bounds — so a line in the transcript can be cited without saying it came
from a scenario. Save, pass and failure lines are `# ` metadata.

## Checking a file

```sh
at-cardano run --check --scenario .tmp/block.dsl
```

`--check` parses the file, resolves every noun and aspect, validates every literal target, and
checks that every `${name}` was bound by an earlier `save` — then stops. It opens no socket and
does not read the key, so a scenario can be validated in CI, or before a run that would spend
requests. It prints `# parses · 11 steps · 4 targets · 3 saves · 3 asserts · 1 echo` and
`# no request was made`. A parse error is the same exit 2, with the same file, line and column,
that a run would have reported.

## Exit codes and failure

| Code | Meaning |
| --- | --- |
| `0` | every step ran and every check passed |
| `2` | the file is wrong, or a check failed (the run finished) |
| `3` | the chain could not answer, or the run timed out |
| `4` | the scenario asked for something the tool refuses |

**Obtaining an answer is fatal; checking one is not.** A target that cannot be answered — HTTP
error, rate limit, a value that is not a hash — stops the run with that failure, because every
step after it would be built on an answer that does not exist. A `save` or an `assert` over an
answer that did arrive is recorded and the run continues, so one bad expectation does not hide
the four after it. A failed `save` leaves its name unbound; every later step that uses `${name}`
fails too, so the first failure is the one to fix. `save` and `assert` lines report as
`# FAIL line N · what · why`; a run with any of them exits 2.

## Bounds

- 64 steps, 256 KiB, and a whole-run timeout (`120s` default, `600s` cap). One request is still
  bounded by the client's own 60-second timeout.
- 200 content lines across every answer. Further answers print their `# ` metadata but not their
  rows, and the run says how many lines it withheld. Checks always run on the whole answer, not
  on what was printed.
- The engine's own bounds are unchanged: 4 MiB per response, 1000 rows per page, ten pages for
  `all`, 64 KiB of raw JSON, 256 KiB of transaction hex.

## What a scenario cannot do

The grammar is closed in the same sense the command line is. A scenario cannot name a host,
port or URL; cannot add a header; cannot read a file other than itself; cannot run a command;
cannot follow a redirect; and cannot reach `/submittx` — a literal `target x query submittx` is
refused while the line is read (exit 4), before a socket opens. Every step is a catalogue verb
with the targets its own entry validates, so allowlisting `at-cardano run` grants exactly what
allowlisting `at-cardano` grants.
