# The toolkit's typed core

How the `at-*` tools become usable in two modes — as a CLI, and as a Rust library inside a
harness — without a second grammar, a second renderer or a subprocess. Written 2026-11-08 against
the tree at `agent-playbook`.

The short version: **the verbs already compute structured facts and then render them immediately.
Keep the facts, render on demand, and both modes share one implementation.**

**Status 2026-11-08.** Step 1 is done: `Opts::resolve`, `search::collect`, `SearchFacts`, and the
optional `serde` feature. Six goldens were captured from the **pre-refactor** binary and still
match byte for byte; a second test proves the library renders the CLI's bytes, including the exit a
limit adds. `cargo test -p at-peek` passes 39 tests, `cargo clippy --all-targets -D warnings` is
clean, and `cargo check --workspace --all-targets` is clean. The remaining verbs are steps 3 and 4.

## What this decides

1. Every verb returns an `XxxFacts` value. The CLI renders it; a harness reads it and renders the
   same `Report`.
2. The rendered text is byte-identical in both modes. The CLI's grant surface does not change.
3. `at-core` stays dependency-free. Serde on the facts is an opt-in feature of the verb crates.
4. The rig plugin stops spawning `at-peek`, `at-recall` and `playbook`. `playbook` already has a
   library target (`Cargo.toml:55`); `at-peek` and `at-recall` already have lib crates and gain an
   entry point that does not print.

This supersedes the "seven tools and a gate" framing in [`rig.md`](rig.md): the tools are the
replacement vocabulary, and the rule engine is the product. The rule engine is planned next; this
document is the seam it stands on.

## What exists today

The CLI is not a thin wrapper over a library — the library is the CLI's implementation, and the
render happens inside it.

| Evidence | What it shows |
| --- | --- |
| `tools/at-core/src/contract.rs:85-162` | `Report` is a **renderer**: `header`, `content`, `note`, `bound`, `next`, then `render() -> String` |
| `tools/at-peek/src/cli.rs:71-89` | dispatch calls a verb, receives `Outcome { report, exit }`, prints `report.render()` |
| `tools/at-peek/src/verbs/search.rs:124-177` | the walk accumulates `shown`, `counted`, counters — the structured answer |
| `tools/at-peek/src/verbs/search.rs:179-239` | it is rendered immediately, and the structure is then dropped |
| `tools/at-recall/src/facts.rs:1-10` | the precedent: git-shaped reads as structs, with "no rendering decision that belongs to one verb" |
| `tools/at-recall/src/facts.rs:217-242` | `Commits::lines()` — a typed value that renders itself |
| `tools/at-peek/src/lib.rs:26-28` | the only library entry point is `run(args) -> i32`, which prints |

So a caller that is not a process has exactly one option today: capture stdout. That is why the
rig plugin spawns. It is not a preference; it is the shape the tools have.

## The shape

```mermaid
flowchart TD
    subgraph cli [CLI mode]
        argv[argv] --> parse[parse + Opts::resolve]
        parse --> collect[collect -> XxxFacts]
        collect --> render1[facts.report() -> Report]
        render1 --> stdout[stdout]
    end
    subgraph lib [Library mode]
        args[typed tool args] --> opts[Opts::resolve]
        opts --> collect2[collect -> XxxFacts]
        collect2 --> render2[facts.report() -> Report]
        render2 --> model[tool result text]
        collect2 --> publish[publish -> effect log]
    end
```

`collect` and `render` are the two halves. The CLI keeps both; a harness uses both, and gains the
middle value the CLI discards.

### Per verb

```rust
pub struct SearchFacts { /* pattern, scope, rows, counted, walk, counters */ }

impl SearchFacts {
    /// The CLI's answer, as the contract renders it.
    pub fn report(&self) -> Report;
}

/// The typed answer. No printing: a caller that runs in-process owns its own stdout.
pub fn collect(paths: &[String], opts: &Opts, mode: Mode) -> Result<SearchFacts, Fail>;

/// The CLI's entry point, unchanged in signature and output.
pub fn run(paths: &[String], opts: &Opts, mode: Mode) -> Result<Outcome, Fail> {
    collect(paths, opts, mode).map(|facts| Outcome::from_report(facts.report()))
}
```

`run` stays, so `cli.rs` does not change and every contract test keeps exercising the same path.
The new surface is additive.

### Argument handling is shared too

`Opts` is where limit clamping, `--max-files`, `--include-secret-paths` and root discovery live
today, but the constructor is private to `cli.rs:188-246`. A library caller that re-implemented
it would drift on the first clamp.

Move the constructor onto `Opts` in `verbs/mod.rs`, and make `cli::options` a thin adapter:

```rust
impl Opts {
    pub fn resolve(
        root: Root,
        root_was_explicit: bool,
        limit: Option<usize>,
        max_files: Option<usize>,
        include_secret_paths: bool,
        summary: bool,
    ) -> Result<Opts, Fail>;
}
```

`Root` is already public (`tools/at-core/src/paths.rs:25,46`) and re-exported by both tools.

### Serde is opt-in

`at-core` says "No dependencies, deliberately" (`tools/at-core/Cargo.toml:8-9`), and that stays
true. The facts live in the verb crates and carry:

```toml
[features]
serde = ["dep:serde"]

[dependencies]
serde = { workspace = true, optional = true }
```

```rust
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SearchFacts { /* … */ }
```

The CLI builds without it. The plugin enables it and wraps a facts value in a newtype that
implements rig's `ContextValue`, so rig never appears in a tool crate:

```rust
impl ContextValue for PublishedSearch {
    const KEY: &'static str = "agent_playbook.search";
}
```

`ContextValue` requires `Serialize + DeserializeOwned` (`crates/rig-core/src/tool/context.rs:183`),
which is why both derives, not only `Serialize`.

The alternative — a serialisable mirror type in the plugin — was rejected: two definitions of
"what a search found" are two things to keep in step, and the first field renamed in one would
leave the other silently stale.

## What each verb already has

None of these needs a new data model; each needs the model it already builds to survive.

| Verb | Computed today | Becomes |
| --- | --- | --- |
| `at-peek search` | `Walk`, `shown`, `counted`, counters (`search.rs:124-177`) | `SearchFacts`; truncation moves into `render` |
| `at-peek slice` | per-target range, shown lines, continuation (`slice.rs:27-128`) | `SliceFacts`; the bounds and notes are render-time |
| `at-peek stat` | one row per target (`stat.rs:24-44`) | `StatFacts`; bytes and lines become numbers, the mtime stays an ISO 8601 string (below) |
| `at-recall state` | `Status`, `Head`, `pending` (`state.rs:36-59`) | `StateFacts`; `Status` becomes public |
| `at-recall log` | `facts::Commits` already (`facts.rs:155-215`) | `LogFacts` wrapping it |
| `at-recall diff` | `facts::FileStat` and totals (`facts.rs:35-149`) | `DiffFacts`; the patch text stays a field |
| `at-recall pr` | sections composed of the above | `PrFacts`, section by section |

## The rig side

`integrations/rig` changes in four ways:

1. `at_peek_search` calls `collect` with a `Root::at(&base)`, then returns
   `facts.report().render()` — the same bytes the CLI prints.
2. It publishes the facts, so `effects.jsonl` records *which files matched and how many*, not a
   rendered string. That is the difference between a transcript and a queryable log.
3. `playbook_check` calls the composer's library (`playbook = { package = "agent-playbook",
   path = "../.." }`), not the binary. The composer's zero-dependency property is untouched; the
   plugin is the only crate that links both.
4. `spawn.rs` is deleted. The plugin makes no subprocess calls, so it no longer depends on the
   agent's `PATH`, and `AT_PEEK_BIN`/`AT_RECALL_BIN`/`PLAYBOOK_BIN` disappear.

What the plugin does **not** do: duplicate the grammar. The `Opts` constructor and the catalogue
are the same code the CLI uses, so a flag that changes changes both.

## Tests

The tools have the right tests already: `tools/at-peek/tests/contract.rs` (844 lines) and
`tools/at-recall/tests/contract.rs` (1510 lines) drive the real binary and assert properties of
stdout. They keep passing because `run` keeps rendering.

Add two things, in this order:

1. **Goldens before the refactor.** Capture the current output of every verb on the fixtures the
   contract tests already build — with fixed dates and content for the git verbs, so a hash is
   stable — into `tests/golden/<verb>.txt`, and assert `facts.report().render()` equals it.
   Captured from the code that is about to change, so a rendering move that alters a byte fails by
   name.
2. **Facts assertions after it.** Properties that only stdout could prove become direct:
   `SearchFacts.total` is the true total when `rows.len() == limit`; `DiffFacts` carries the same
   `FileStat` rows its table renders; `StateFacts.pending` names a known list.

No existing assertion is changed, and the contract tests still call the binary.

## Order of work

| Step | Change | Proves |
| --- | --- | --- |
| 1 | `Opts::resolve`, `search::collect`, `SearchFacts`, serde feature | the pattern, end to end |
| 2 | plugin uses `search::collect` and publishes facts | no subprocess, identical text |
| 3 | `slice`, `stat` | the simple verbs follow without new decisions |
| 4 | `state` (expose `Status`), `log`, `diff`, `pr` | the git verbs, where the facts mostly exist |
| 5 | delete `spawn.rs`; `playbook` via lib | the plugin has no process dependency |
| 6 | the rule engine (`rules.rs`, footprint gates) | the product, on a typed seam |

Step 2 is the checkpoint: if the plugin's text differs from the CLI's on the same fixture, stop
and fix the render, not the test.

## Costs, risks and what stays out

- **Build time.** The plugin links `regex` (through `at-peek`). That is a compile-time cost once,
  not a runtime one.
- **Public surface.** `XxxFacts` and `collect` become public API of pre-1.0 tool crates. They are
  additive; `run` remains the stable entry point.
- **Byte-identical output** is the invariant most at risk. Goldens, not review, are the guard.
- **`SystemTime` has no serde impl.** `StatFacts` keeps the mtime as the ISO 8601 string
  `contract::iso8601_utc` already produces, rather than the timestamp, so the facts serialise with
  no adapter and the render is the field. This is the one value formatted at collect time; it is
  also the only one where a second form would be a guess.
- **No new capability.** The facts expose nothing the rendered text did not already print: same
  reads, same deny-list, same exit codes. `at-peek`'s no-write property is structural and is not
  touched.
- **Out of scope:** a `--json` CLI mode (a separate surface with its own contract), moving facts
  into `at-core`, changes to exit codes, and any change to what the verbs may read.

## What would make us stop

- A verb whose facts cannot be separated from its render without a semantic change — then that
  verb keeps `run` and is not offered to the plugin.
- Goldens that differ in more than whitespace after a mechanical move — that is a rendering
  change, and it needs its own decision.
- `at-recall`'s facts proving too large to publish within rig's kept-facts budget
  (`MAX_KEPT_FACTS_BYTES`, 256 KiB): publish the summary and the bound, not every hunk.
