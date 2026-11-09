# Evaluating rig against real sessions

The one-prompt benchmark was useful once: it proved the plugin loads, the tool runs in process, and
a refusal reaches the model. It cannot tell us whether any of this makes work cheaper, because the
tasks we actually run are not one search.

This is the design for the next measurement: scenarios shaped like real Zed threads, a metric that
tracks what those threads actually cost, and the ways to drive rig through them.

## What the real sessions look like

An audit of the Zed thread store (`~/Library/Application Support/Zed/threads/threads.db`, 143
threads exported 2026-10-20; the live store now holds 302) gives the shape of the work:

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

Scenario 4 is the one that tests the devshell rule as well as the read rules: a bare `cargo` in a
flake repository is the failure the `rust-devshell-first` rule exists to prevent. The plan called
for a real failing test. No such failure exists in the repository today, and seeding one would
measure a fixture rather than the repository, so the scenario runs the suite that is there and
scores the totals. The devshell half — the part the rule governs — is real either way.

## What to measure

Per run, from the `--json` stream (the effect log is kept beside it for the questions the stream
cannot answer — see the `rig-session-query` skill):

- **Correctness** — cited `path:line` set against gold, as precision and recall.
- **Tool calls by name** — `terminal`/`shell` calls are the bypass; `at_*` calls are compliance.
- **Total tool output bytes** — the audit's 8 MB threads are the target. A bounded answer is the
  point of the toolkit, and this is the number that shows it.
- **Model calls** — the refusal round-trip is visible here; a redirect would remove it.
- **Wall time** and **compactions** — the latter is a proxy for context bloat.

Turns are not a metric: rig counts one turn however many calls it contains. Model calls, tool calls
and output bytes are the quantities.

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
copy, and `--out` is required. A corpus inside the checkout is only run where the macOS sandbox is
available: `sandbox-exec` denies the agent every read of the corpus, its parent (the eval crate),
the output directory and `.tmp/rig-eval`. Without it a repo-wide search would find the content,
and a model that reads its own gold scores itself, so the runner refuses an in-checkout corpus
rather than pretend the run is evidence.

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
than a model failure. The score is checks passed. Precision is `path:line` hits over every
citation the answer makes; recall is hits over gold. Precision is a weak signal when gold is a
small set — an answer that cites ten correct lines and three gold ones scores 0.3 — so read it
beside the score, not instead of it. Turns are not a metric.

Each run keeps its raw stream, stderr, and a copy of `effects.jsonl`, and the directory gets
`results.json` and `summary.md`. A run whose tool arguments name the corpus, or whose tool output
contains a scenario or a results document, is marked contaminated and excluded.

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
