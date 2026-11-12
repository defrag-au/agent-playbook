# Evaluating rig against real sessions

The one-prompt benchmark was useful once: it proved the plugin loads, the tool runs in process, and
a refusal reaches the model. It cannot tell us whether any of this makes work cheaper, because the
tasks we actually run are not one search.

This is the design for the next measurement: scenarios shaped like real Zed threads, a metric that
tracks what those threads actually cost, and the ways to drive rig through them.

## What the real sessions look like

An audit of the Zed thread store (`~/Library/Application Support/Zed/threads/threads.db`, 143
threads exported 2026-10-20; the live store held 303 on 2026-11-11) gives the shape of the work:

| Measure | Value |
| --- | --- |
| Median tool calls per thread | **74** |
| 90th percentile | **821** |
| Mean | **293** |
| Threads with at least one failed tool result | **92 of 143** |
| Mean tool output per thread | **793 KB** |
| Largest threads | 8–9 MB of tool output, 1,500–3,800 tool calls, 5–16 compactions |

One thread is enough to show where the calls go. In the 97-message "Zed agent thread analyser
recovery" session:

| Tool | Calls |
| --- | --- |
| `terminal` | **59** |
| `write_file` | 19 |
| `edit_file` | 17 |
| `read_file` | 17 |
| `fetch` | 12 |
| `list_directory` | 4 |
| `grep` | 1 |
| `find_path` | 1 |

The catch-all is the terminal, and the read verbs that the toolkit replaces are barely used. That
is the bypass the Gate exists for, and it is why a one-call test tells us nothing: the effect only
shows over tens of reads and hundreds of kilobytes.

## The scenarios

Each is a task shaped like one a real thread attempted, cut down to one answerable question, with
a gold answer computed by the `at-*` CLI before the run. "Gold" is a set of `path:line` citations,
not a prose reference.

| # | Scenario | Shape | Gold | Exercises |
| --- | --- | --- | --- | --- |
| 1 | **Recover an investigation** — "A previous session left the rig integration mid-stream." | Read the notes, follow the trail | The status lines in the README and design record | `at_peek_search`, `at_peek_slice` |
| 2 | **Trace a symbol** — "How does a shell `rg` reach `at_peek_search`?" | Definition, handler replacement, the in-process read | Every definition on the path | `at_peek_search`, `at_peek_slice` |
| 3 | **Draft a PR** — "Summarise this branch for a PR body." | Commits, diffstat, changed areas | The commit hashes and representative files | `at_recall_pr`, `at_recall_diff`, `at_recall_log` |
| 4 | **Run the devshell tests** — `cargo test` for the plugin; report the totals | Run under the devshell, then locate the harness test | The harness test's `path:line` | `shell`, `at_peek_slice` |
| 5 | **Pre-edit state** — "What is dirty, and what changed since main?" | Status plus a diff summary | The changed paths and the branch | `at_recall_state`, `at_recall_diff` |
| 6 | **Rig vs factotum** — "Does rig replace factotum?" | The decision, the ownership boundary, the v2 constraint | Four lines in `rig.md` | `at_peek_search`, `at_peek_slice` |
| 7 | **OTel or DuckDB** — "Is OTel required?" | What is canonical, what a span lacks, what would bring OTel back | Five lines in `rig.md` | `at_peek_search`, `at_peek_slice` |
| 8 | **Eval contamination** — "How is the gold sealed?" | The rule, the tags, history, the sandbox | Seven lines across three files | `at_peek_search`, `at_peek_slice` |
| 9 | **Facts without a subprocess** — "How does `at-peek search` reach rig?" | The facts type, `collect`, publication | Six lines across the verb and the plugin | `at_peek_search`, `at_peek_slice` |
| 10 | **Redirect declines** — "What does the redirect serve?" | Served forms, the compound stop, declined flags | Eight lines in `redirect.rs` | `at_peek_search`, `at_peek_slice` |
| 11 | **Advertising over gating** — "Why redirect instead of refuse?" | Refusal versus redirect cost, the record, the bounded answer | Five lines in the enforcement measurement | `at_peek_search`, `at_peek_slice` |
| 12 | **Zed thread tools beside at-** — "Why not in the namespace?" | Private store, writes, never-print contract | Six lines across the doc and README | `at_peek_search`, `at_peek_slice` |
| 13 | **The skipped rule** — "Audit the secrets rule from `rules/`" | A false zero, the bound that exposes it, two quoted lines | The definition and the report-don't-delete instruction | `at_peek_search`, the CLI's `--include-secret-paths` |

Scenario 4 is the one that tests the devshell rule as well as the read rules: a bare `cargo` in a
flake repository is the failure the `rust-devshell-first` rule exists to prevent. The plan called
for a real failing test. No such failure exists in the repository today, and seeding one would
measure a fixture rather than the repository, so the scenario runs the suite that is there and
scores the totals. The devshell half — the part the rule governs — is real either way.

Scenarios 6–12 were mined on 2026-11-11 from the exported rig thread (`Rig HawDB OTEL agent
observability`, 78 user messages) and the analyser recovery thread. Each prompt is a real question
from one of them, cut to an answer the committed tree can settle. The first runs, all on `rig:gated`
with `deepseek/deepseek-flash`, one run each:

| Scenario | Score | Wall | Model calls | Tool calls | Output | Missed |
| --- | --- | --- | --- | --- | --- | --- |
| `rig-vs-factotum` | 1.00 | 100.3 s | 6 | 8 | 34.2 KB | — |
| `otel-or-duckdb` | 1.00 | 18.8 s | 6 | 9 | 23.5 KB | — |
| `advertising-over-gating` | 1.00 | 28.3 s | 11 | 15 | 53.6 KB | — |
| `eval-contamination` | 0.92 | 29.0 s | 13 | 15 | 58.7 KB | `snapshot.rs:5` |
| `redirect-declines` | 0.93 | 19.2 s | 7 | 10 | 49.0 KB | `redirect.rs:327` |
| `facts-without-subprocess` | 0.89 | 37.7 s | 10 | 15 | 55.4 KB | `tools.rs:292` |
| `zed-thread-tools-not-at` | 0.92 | 26.8 s | 11 | 15 | 81.1 KB | `README.md:44` |

The first `facts-without-subprocess` run scored 0.60 because its gold named commentary lines
rather than the `SearchFacts`, `collect` and publish definitions; the gold was corrected before the
run above. The first `zed-thread-tools-not-at` run missed a `must_say` that required the literal
"never prints" phrased as the README has it; the check now names the concepts (`message text`,
`result body`). One run each is a smoke test, not a comparison.

**The local arm is not ready for this corpus.** On 2026-11-11 `local/qwen3-vl:8b` ran
`rig-vs-factotum` on `rig:slim-local` and `rig:advertised-local` (the same scenario, separate local
homes): both scored 0.12. The slim arm made 0 tool calls in 182 s; the advertised arm made 2
`at_peek_search` calls, 482 B of output, in 284 s. Both answers cited `AGENTS.md` rather than
`docs/design/rig.md` — the model used the system-prompt copy and stopped there. Advertising changed
the model's tool habit, not its answer. Do not run the local matrix until a stronger local model or
an explicit "read the repository files, not AGENTS.md" instruction is chosen.

### The long-scenario matrix (2026-11-11)

Seven long scenarios × three frontier arms, one run each, `deepseek/deepseek-flash`, two workers:

| Target | Runs | Median score | Median tool calls | Median output | Median model calls |
| --- | --- | --- | --- | --- | --- |
| `rig:advertised` | 7 | 1.00 | 10 | 47.3 KB | 8 |
| `rig:baseline` | 7 | 1.00 | 11 | 41.3 KB | 8 |
| `rig:slim` | 7 | 1.00 | 13 | 51.0 KB | 9 |

Every run scored at least 0.83 and no run was refused. The slim arm had the only sub-1.00
(`eval-contamination`, 0.83) and the highest median tool calls; the advertised arm had the lowest.
Those differences are one run each, and precision was 0.10–0.33 across all arms because the models
over-cite — read recall and output beside the score. The 21 runs cost $0.12 in total (4.1M input and
82k output tokens). This is a smoke test, not a ranking; the next step is paired repeats (two or
three, still under $1) or a harder scenario the baseline fails.

### The skipped rule: a scenario built around a false zero (2026-11-12)

The thirteenth scenario was written to the one failure the earlier corpus could not produce: a
question whose naive answer is **confidently wrong**. `rules/core/no-committed-secrets.md` is
the source of truth for the secrets policy, and `at-peek`'s secret-shaped guard matches its
filename. A default search of `rules/` therefore prints

```
# walked 40 files
# skipped: 1 secret-shaped
# 0 matches in 0 files
```

The prompt asks for the rule, both quoted instructions, and what each search did not read. If a
harness believes the zero, or lists `rules/` and never searches, the answer is "no rule governs
secrets".

| Target | Score | Tool calls | Output | Missed |
| --- | --- | --- | --- | --- |
| `rig:slim` | 1.00 | 16 | 12.3 KB | — |
| `rig:advertised` | 1.00 | 12 | 7.9 KB | — |
| `rig:baseline` | 1.00 | 14 | 6.7 KB | — |
| `opencode` | 0.86 | 16 | 25.3 KB | `at_peek_search` |
| `pi` | 0.86 | 22 | 29.5 KB | `at_peek_search` |

All five found the trap, quoted `# skipped: 1 secret-shaped`, re-ran with
`--include-secret-paths`, and cited `:13` and `:30`. The honest bound did the work the scenario
was built to test: no arm answered from the false zero. So this is a ceiling case for every
harness, and the only separating check is the rig plugin's tool name, which opencode and Pi cannot
have by construction. The first smoke (before that check was corrected) scored all three rig arms
0.86 for the same reason.

Three things it found that are worth acting on:

- **A named path still returns a false zero.** `at-peek search secret
  rules/core/no-committed-secrets.md` skips the file it was pointed at and reports `0 matches`,
  and `slice` refuses it too. opencode and Pi both called this out as a false fact rather than a
  refusal. Whether a named path should override the guard, or the guard should refuse loudly, is an
  open decision.
- **`tool_says` naming a `ContextValue` key measures the wrong thing.** `agent_playbook.search` is
  never printed by the search tool; it appears in the stream only because
  `facts-without-subprocess` reads `tools.rs`, which declares the key, and
  `agent_playbook.redirection` passes only because `advertising-over-gating` reads the design doc
  that names it. The check in this scenario now names `at_peek_search`, which the stream records as
  the tool result's name. A cross-harness comparison still needs per-check applicability, or
  opencode and Pi are capped below 1.00 on every scenario that requires a rig tool.
- **Snapshot file counts are 478, not 483.** HEAD tracks five of the corpus files; the snapshot
  rewrites them out of history and the working tree. Counting gold against `git archive HEAD`
  includes those five, so a true total measured that way is five files high.

Two gold boundaries were also corrected from the first matrix: `redirect-declines` no longer asks
for `redirect.rs:327` — no arm cited the boolean wrapper, and `:346` is the dispatch they cite —
and `zed-thread-tools-not-at` asks for `README.md:43`, the sentence the `:41-43` citation covers,
rather than `:44`, which is about naming a thread.

Arm variance is large at this sample size: two smokes of this one scenario gave `rig:advertised`
23 tool calls and 20.4 KB in the first, 12 and 7.9 KB in the second, at the same score. One run
each is a smoke test, not a rate.

### Where rig loses the call count (2026-11-12)

The two exploratory scenarios are where rig spends several times opencode's calls. One run each at
HEAD, `deepseek/deepseek-flash`, two workers:

| Scenario | Target | Model calls | Tool calls | Output | Score |
| --- | --- | --- | --- | --- | --- |
| `recover-investigation` | opencode | 4 | 6 | 65.7 KB | 0.75 |
| | `rig:baseline` | 14 | 20 | 84.3 KB | 0.50 |
| | `rig:advertised` | 19 | 28 | 83.2 KB | 0.75 |
| | `rig:slim` | 17 | 29 | 106.5 KB | 1.00 |
| `trace-redirect` | opencode | 3 | 6 | 112.6 KB | 0.75 |
| | `rig:baseline` | 10 | 14 | 71.5 KB | 1.00 |
| | `rig:advertised` | 9 | 12 | 67.3 KB | 1.00 |
| | `rig:slim` | 10 | 14 | 72.3 KB | 0.50 |

The cause is in the shell calls, not the model. Of the 16 shell calls in
`recover-investigation`/`rig:advertised`, **14 begin `cd <snapshot> &&`** — the model makes the
snapshot absolute because the shell tool does not keep its `cwd`. The redirect declines any `&&`
before it looks at the read, so those 14 are never served; the remaining reads use
`grep -n -E` (an extended-regex flag it declines) or a pipeline (`| sed -n`, `| head`). Only 6
redirections fired for 16 shell calls. In `trace-redirect` the same shape gives 3 redirections for
12 shell calls.

Advertising does not close this: it raises the call count (19 against 14 on
`recover-investigation`) because a model told about seven tools tries more of them. The toolkit is
not losing on description, it is losing because the forms the model actually writes are declined.

Three levers, in order of the calls they recover:

1. **`cd <literal> && <served read>`** — strip a leading `cd` to one literal path and use it as the
   base; every remaining segment must still be a served read, so `cd x && rm -rf` declines as it
   does today. Fourteen of the 16 shell calls here begin that way, but most continue into a pipeline
   or a second `&&`, so only the pure ones are servable.
2. **`-E` on `grep`/`rg`** — `grep -n -E '^## '` is an extended regex, which is the engine the
   search already uses, so accepting it interprets the pattern correctly rather than approximately.
   `-i`, `-l` and `-c` stay declined: they change the answer, and the existing tests say so.
3. **Pipelines** — `rg | head` is the habit the toolkit exists to replace, and serving it as a
   bounded search with the true total is arguably the better answer. But `grep | grep -v` is a
   different query, and the module's rule is that a wrong substitution is worse than none. This one
   is a decision, not a fix.

**Levers 1 and 2 are in (2026-11-12).** `cd <literal> &&` is stripped to a base, `grep -E` is
accepted, and an empty quoted word is kept rather than dropped — which had turned `grep ''` into a
search for the next argument. 42 unit tests, clippy and fmt clean. The redirect fires on the form:
`cd docs/design && grep -E '...'` comes back as `# at-peek search · design · /…/`. The call count
did not move: redirections went 6 → 7 (`recover-investigation`, advertised) and 6 → 9 (slim),
model calls 19 → 18 and 17 → 18, tool calls 28 → 25 and 29 → 27. Both rig arms' scores moved
*down* in the same run, and `rig:baseline` — which registers no redirect at all — moved from 0.50
to 0.00 on the same scenario, so that swing is run variance, not the change.

The transcript says why redirect coverage cannot close a 3–5× gap. opencode's
`recover-investigation` run is **five** calls: `ls && git log`, one `grep`, then three whole-file
reads. `rig:baseline`'s is **seventeen**: one `at_recall_state`, eight `at_peek_search` calls with
heading and keyword patterns, seven `read` calls, one diff. The model is not being refused; it is
repeatedly asking for a file's shape and then for ranges of it. A served read is still one tool
call and one model turn, so redirecting more shell forms cannot reduce the count — only asking for
fewer, larger things can.

The missing affordance was an **outline**: one call returning each file's headings with line
numbers and its true line count, which is what `grep -n -E '^## '` and the eight searches were
hand-rolling.

**Built 2026-11-12.** `at-peek outline <path>…` is a catalogued verb, an `at_peek_outline` tool in
all four tool sets (including slim, because no shell form maps to it), and an advertised entry with
a map line. It reports ATX headings outside fenced code blocks, removes a closing hash run only
where CommonMark allows one, and states the file's true line count. In the snapshot it answers
`docs/design/rig.md` in one call: 18 headings, 420 lines, and the shell snippets inside fences
ignored.

The first measurement: the mechanism works, the aggregate is noise. `rig:baseline` stopped using
`at_peek_search` as an outline — three heading-pattern searches before the verb
(`^## |the first result|…`, `^## |^### |…`, `^## |Status|…`) became two `at_peek_outline` calls
covering four files, three of them in one call, and no heading search remained. Its total still
rose, 17 → 24 tool calls, because that run explored differently: seven shell calls and seven
slices, including a `cargo test` to check a claim. `rig:advertised` moved the other way on model
calls (18 → 15) and used the outline once, and the unchanged opencode arm moved 5 → 9 on the same
scenario. The between-run strategy variance is larger than the displacement, so one run each
cannot attribute the total. The next measurement is a paired A/B — the same arm with and without
the verb, three repeats, in one batch.

`trace-redirect`'s gold was also stale: the stored lines pointed into the layout before the
redirect was refactored, so on 2026-11-12 every arm scored 0.00, opencode included. The gold is now
`:95` (the interception system), `:119` (the handler replaced), `:261` (`search::collect` in
process) and `:348` (the `rg`/`grep` arm), which is what every arm cites unprompted. The 2026-11-09
`trace-redirect` row in `harness-comparison.md` was measured against a different source layout and
is not comparable with these runs.

## What to measure

Per run, from the `--json` stream (the effect log is kept beside it for the questions the stream
cannot answer — see the `rig-session-query` skill):

- **Correctness** — cited `path:line` set against gold, as precision and recall. It is a gate, not
  a ranking: the corpus saturates at 1.00 for every frontier arm.
- **Billed cost per passed check** (`Cost/check`, µ$) — the number the prompt is actually judged
  on. It prices turns, tokens and caching together, so it cannot be improved by moving work between
  turns.
- **Cache share** (`Cache`, %) — the leading indicator behind that cost. A stable prompt prefix is
  charged at a fraction of the uncached rate, and a description or ordering change that breaks the
  prefix drops the share before the bill moves.
- **Model calls, tool calls by name, output bytes** — diagnostics, not the sentinel. On the
  2026-11-09 corpus opencode wins model calls per check (1.0 against 2.0) while rig wins cost per
  check ($0.00072 against $0.00104), because 85–96% of rig's input is served from cache against
  opencode's ~62%. The three proxies disagree, and only cost prices the trade.
- **Wall time** and **compactions** — wall time is contended under `--jobs > 1`; compactions are a
  proxy for context bloat.

`Cost/check` and `Cache` are `—` when they cannot be stated: no check passed, no price reported, or
no input tokens. An unpriced run is not a free one. Each driver normalises `input_tokens` to every
input token sent — opencode and Pi report cache reads beside the uncached count, rig includes them —
so the share means the same thing across arms.

## What the corpus already has

`corpus/base` is 50+ plugins written only against the public API, one per capability, each with
tests. Six matter to this evaluation, and one is a sibling of our redirect:

| Module | What it is | What we take |
| --- | --- | --- |
| `shell_rewrite` | rewrites a shell call's command in `Prepare`, annotates the result `(ran as: …)` | the same seam as our redirect, with the alternative attribution: edit the request rather than replace the handler |
| `effect_query` | `effects`/`effects_detail` tools and `/effects`, reading the live log through `EffectLog` | the agent can query its own effects; DuckDB is for cross-session questions, not in-session ones |
| `output_truncate` | a Review system that cuts long tool results and spills the whole text to a file | bounding the 8 MB threads may matter more than any read redirect |
| `session_replay` | `rig --replay=DIR`: replays a recorded session, comparing every request and refusing what the record lacks | deterministic regression over a **real** session |
| `http_api` | `--serve`, POST/GET/SSE over the agent, approvals over HTTP | a custom front or dashboard without forking the launcher |
| `window_gui` | `add_front(Front::new(..))` and four resources driven by requests | the front contract, if we ever want a rig-native view |
| `code_search_index` | a background semantic index with a `code_search` tool | not a replacement for exact search; do not rebuild it |

## How to drive rig

| Way | What it gives | What it costs |
| --- | --- | --- |
| `rig -p --json` | headless, one prompt per process, the full JSON event stream and effect log | a runner that spawns N processes and scores the stream; no mid-run steering |
| **A scripted model** (`MockCompletionModel` + `rig-harness-test-support`) | an app with a scripted reply per turn, collectors for the effect log and tool outputs, `run_until`; no key, no network, milliseconds | none now: the plugin's `[patch.crates-io]` and a dev-dependency point at the fork checkout |
| **`session_replay`** | a recorded real session replayed exactly, request by request, failing on the first divergence | record once; the replay is only as broad as the recording |
| A `BenchPlugin` in the generated agent | scenario delivery, assertions on each call as it happens, early stop, one score row per run — all through the plugin API, no fork | a plugin to write and keep in step with the harness |
| `rig acp` + Zed | the real editor, a human watching | not repeatable, not a measurement |
| `rig-core` with our own loop | maximum control of the model loop | loses the gates and the effect log — the things being evaluated |

**Started with a scripted model for the mechanics.** `integrations/rig/tests/redirect.rs` scripts a
`shell rg` turn, runs the app, and asserts what a front and the log see: the conversation keeps the
model's own call, the result is the bounded search, `agent_playbook.redirection` names the tool that
answered it, and no `decided_by` pretends a gate refused it. It is the corpus idiom
(`shell_rewrite/tests.rs` is the template), costs milliseconds, and is the home for every rule we
add.

The test-support crates are unpublished, so the plugin's workspace patches `rig-harness` to the
local fork (`~/code/defrag/rig`) and takes `rig-harness-test-support` from the same checkout. The
agent build ignores that patch and gets the same checkout from the launcher's own
`[patch.crates-io]`, because `rig` is installed from the fork.

**Then `-p --json` for the behaviour.** A scripted model cannot tell us whether a real model
chooses `rg`, or how it reacts to a redirect. That is the run that answers the question, and it
needs a key and tolerates variance.

**Replay once a scenario is stable.** `session_replay` turns a recorded run into a regression that
fails on the first divergent request — the strongest cheap test of "the harness still behaves as it
did".

A `BenchPlugin` is for scenarios that need to react mid-run (stop when the model has cited the
definition, score each call before the next). `rig acp` is for verifying the interactive experience
once the numbers are in. `rig-core` with our own loop is rejected: it loses the gates and the log.

## The runner

`integrations/rig-eval` is a separate package with no rig dependency: it reads the `--json` stream
and scores it.

```sh
cargo run --manifest-path integrations/rig-eval/Cargo.toml -- --out /tmp/rig-eval
```

The corpus ships at `integrations/rig-eval/scenarios`; `--scenarios` points elsewhere for a private
copy, and `--out` is required. The corpus is committed, so a filesystem deny on `scenarios/` alone
does not hide it: `git show` and `git diff` read the committed content from `.git`. Each run
therefore executes in a snapshot of the worktree under `$TMPDIR`:

- the snapshot copies the real `.git` and rewrites the corpus out of the commits that hold it, then
  prunes the old objects; the reachable history has no corpus, while the branch commits remain for
  `draft-pr`;
- the working tree is a clean checkout of that history; the corpus directory is absent;
- `integrations/rig` resolves its dev-dependencies at `../../../rig`; the sibling fork is symlinked
  beside the snapshot, so the plugin's tests run as they do in the checkout;
- the seatbelt profile denies the real checkout's `.git`, the corpus directory, the output
  directory and `.tmp/rig-eval`; the snapshot's own `.git` is outside those paths;
- the snapshot's `eval` branch carries one uncommitted file (`docs/design/rig-eval.md`), so a
  `git: status` scenario has a deterministic answer;
- build directories in the snapshot are symlinks to the real warm `target/` directories, because a
  cold build per snapshot would dominate the run; cargo's lock serialises workers that share one.
  One consequence: a test binary compiled inside a snapshot has that snapshot's path baked into
  `CARGO_MANIFEST_DIR` and `CARGO_BIN_EXE_*`, so after the snapshot is deleted a later `cargo test`
  at the checkout can run the stale binary and fail with `NotFound`. `cargo clean --workspace`
  (which keeps dependencies) forces the checkout's own build; the eval run itself is unaffected.
- a `direnv` shim at the front of `PATH` delegates `direnv exec` to the real devshell and then runs
  the command in the snapshot. No per-snapshot allow entry or copied cache is needed.

`--jobs N` runs up to N snapshots concurrently. Each worker resets to the template commit and
reseeds the dirty file between jobs, so every run starts from the same content. Wall time includes
contention and is not comparable across arms; model calls, tool calls, output bytes and score are.
This is measurement integrity, not a safety sandbox; see
`harness-comparison.md#isolation-is-not-an-arm`.

A scenario is JSON:

```json
{
  "name": "trace-redirect",
  "why": "the thread the scenario is cut from",
  "prompt": "…",
  "workdir": ".",
  "gold": ["path/to/file.rs:123"],
  "extra_paths": ["path/to/file"],
  "must_say": ["literal in the answer"],
  "tool_says": ["literal in the tool output"]
}
```

Every check is validated before a run: a gold line past the end of its file is an error rather
than a model failure. A scenario whose expectation is the working tree itself uses
`"git": "status"` instead of the static lists: the runner reads the branch and every changed path
from the snapshot's `git status --branch -uall`. The snapshot always has one uncommitted file, so
the expectation is deterministic. `pre-edit-state` is that scenario.

The score is checks passed. Precision is `path:line` hits over every
citation the answer makes; recall is hits over gold. Precision is a weak signal when gold is a
small set — an answer that cites ten correct lines and three gold ones scores 0.3 — so read it
beside the score, not instead of it. Turns are not a metric.

A path check is satisfied by the path itself or by a directory above it: `git status` collapses
an untracked tree to its top directory, and that is as correct an answer as listing every file
under it.

Each run keeps its raw stream, stderr, and a copy of `effects.jsonl`, and the directory gets
`results.json` and `summary.md`. A run whose tool **output** contains a scenario or a results
document is marked contaminated and excluded from the tables and medians. A tool call that merely
names the sealed corpus is recorded as `inspected-corpus` instead: the sandbox refuses the read,
so a blocked probe says the agent noticed the seal, not that it saw the gold. Both appear in the
run's `tags` in `results.json`, and the summary names the excluded and inspected runs.

## The first result

2026-11-09, five scenarios × two homes × two repeats, `deepseek/deepseek-flash`. The paired table is
the median of the two runs; two is not a sample, and the spread is wide.

| Scenario | Score (gated / baseline) | Tool calls | Output bytes | Model calls |
| --- | --- | --- | --- | --- |
| `devshell-tests` | 1.00 / 1.00 | 9 / 8 | 14.3 KB / 13.9 KB | 7 / 6 |
| `draft-pr` | 1.00 / 0.80 | 13 / 11 | 25.4 KB / 28.6 KB | 8 / 7 |
| `pre-edit-state` | 1.00 / 1.00 | 5 / 3 | 51.5 KB / 1.7 KB | 6 / 4 |
| `recover-investigation` | 1.00 / 1.00 | 27 / 21 | 93.0 KB / 99.3 KB | 17 / 15 |
| `trace-redirect` | 0.80 / 1.00 | 13 / 13 | 44.2 KB / 52.3 KB | 8 / 10 |
| **median** | **1.00 / 1.00** | **13 / 11** | **44.2 KB / 28.6 KB** | **8 / 7** |

Four readings, and the first one is the result:

- **The gate did not make the work cheaper.** Correctness is equal at the median, and the gated
  home spent about 18% more tool calls, 55% more output bytes and 14% more model calls. The one
  apparent win in the earlier, contaminated runs disappeared when the corpus was sealed.
- **The Gate barely engages.** Three refusals in ten gated runs. Almost every shell command begins
  `cd <repo> && …`, and the Gate judges only the first program of the first segment, so the read
  behind the `cd` is never seen. The command table is narrow too: the largest single result in the
  run was `ls -R` over a build tree, **51 KB in one call**, and no `at-*` verb replaces a recursive
  listing. That is the `output_truncate` shape from `corpus/base`, not a refusal.
- **`read` is the second bypass.** The gated home called `read` 31 times against the baseline's 15
  across the same ten runs. A refusal pushes the model to a tool that reads whole files, not to the
  bounded slice the refusal names. The toolkit's advantage — the true total and what the walk
  skipped — is not what the model reaches for.
- **The redirect is exact when it is reached.** The forced `rg` test was parity, and a redirected
  call costs nothing extra. The problem is reach: on a natural prompt the model rarely issues a
  plain `rg`/`grep` as the first program of the command.

The scenarios are also too easy to separate correctness: both homes score 0.8–1.0, and the misses
are citations one line off rather than wrong conclusions. That confirms the warning that opened
this design: a task that needs one search cannot show the difference. The next corpus has to come
from the long threads — median 74 tool calls and 793 KB — where the shape of the read and the size
of the result compound.

The five-target comparison in `harness-comparison.md#the-first-result` supersedes this two-home
result; the snapshot runner above produced it.

## What the next run changes

Four changes, in order, each measured by the same runner:

1. **Judge every segment of a chain.** Split `&&`, `||` and `;` as well as `|`; skip `cd` and
   environment assignments to find the program; refuse or redirect a read in any segment. The
   current first-program rule is why three refusals were all the Gate produced.
2. **Bound what has no replacement.** A `Review` system that cuts a long tool result and spills
   the whole text to a file, the `output_truncate` pattern. `ls -R`, `find` and a `sed` range the
   parser refuses all reach the model unbounded today.
3. **Bring `read` into the policy.** Replace its handler in `Prepare` with `at_peek_slice`, or bound
   its output through `at_peek_stat` and a `Review`. Until then the gated home has an unbounded way
   to read any file, and it used it twice as often as the baseline.
4. **Add one long-thread scenario.** A recovery that needs a dozen reads across several files, sized
   so the baseline's raw output is hundreds of kilobytes. The current five are useful for
   correctness; they cannot show a byte difference that is not noise.

## The engineering the forced-command run asked for

Before the scenarios, one forced `rg` measured the Gate at **+1 model call** over the ungated
baseline: the model has to receive the refusal and issue a second call. Tightening the message
removed the second shell retry (3 tool calls became 2) but not the extra model call.

**The redirect is built**, for `rg`/`grep`, in `PrepareSystems::Edit` — option 1 below. The same
forced prompt now costs **2 model calls and 1 tool call, exactly the ungated baseline**, with the
bounded search as the shell's result. rig offers two seams for it:

1. `PrepareSystems::Edit` — replace the `shell` call's `CallHandler` with the `at_*` tool when the
   command is a read. The model never sees a refusal, and the call runs on the executor rather than
   blocking a gate. **Taken.**
2. `Gate` — run the replacement and answer the call with `Outcome::from(ToolResult::success(text))`.
   Simpler, but the work is synchronous and the effect log attributes the answer to `shell`, so the
   substitution should be published as a fact.

The remaining arms are the same shape, one parser each: `cat`/`head`/`tail`/`sed -n` → `at-peek
slice`, `wc` → `at-peek stat`, and `git status`/`diff`/`log` → `at-recall`. Until then those are
still refused, which is correct but costs the round-trip.

The redirect is not free either: the model asked to run one command and got another, so the result
text has to say what it is (`at-peek search · …`, which it already does) and the effect log names
the substitution (`agent_playbook.redirection`). That is a smaller price than a round-trip on every
read in a 74-call session.

Doing this well is what turns the plugin from a referee into a **policy layer**: the rule decides
what the call becomes, not only whether it may run.

## What would make this not worth it

- Correctness on scenarios 1–5 no better than the ungated harness, with output bytes no lower.
  **Observed in the first run.** That is not the end of the experiment: the run also names the two
  reasons — a Gate that cannot see a `cd &&` chain, and a `read` tool outside its policy — and
  fixing those is the next measurement, not a reason to keep the current shape.
- Redirect proving impossible to attribute honestly in the effect log. Not observed: the
  substitution publishes `agent_playbook.redirection` beside `agent_playbook.search`, with no
  `decided_by`.
- The harness staying a separate `RIG_HOME` that needs its own build for every experiment. The
  homes are cached; a five-scenario, two-home, two-repeat run is about eight minutes.
