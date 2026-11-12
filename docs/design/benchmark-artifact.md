# The publishable benchmark

What a published result is, what it is not, and what a reader needs in order to check it.

The measurement is a benchmark over **repository work**: read a checkout, answer a question with
citations. It is not a coding benchmark and it is not a leaderboard. Its claims are about cost,
adoption and the effect of a read toolkit, on one corpus, with one model.

## The four artefacts

| Artefact | What it is | Where it lives |
| --- | --- | --- |
| **The corpus** | 13 scenarios, each a prompt, a gold answer and a set of checks | `integrations/rig-eval/scenarios/` |
| **The runner** | `rig-eval`: snapshots, isolation, drivers, scoring, cost accounting | `integrations/rig-eval/` |
| **A run** | the evidence: one directory per run, with the manifest, results, streams and effect logs | `--out <dir>` |
| **The report** | the claims, the tables, the notebook and the limits | `docs/design/` + the run's `observable.md` |

A published result names all four: the corpus fingerprint, the runner commit, the run directory and
the report it supports. A number without them is an anecdote.

## A run directory

```
manifest.json          identity: corpus, checkout, snapshot, arms, model, totals
results.json           one row per run — every field the score and cost are computed from
summary.md             the human tables, including Cost/check, Cost/solved and Cache
traces.json            one row per run, grouped outcome/work/usage/toolkit/status
tools.json             one row per (scenario, target, repeat, tool)
checks.json            one row per check: required, source, found
observable.md          the notebook: loader cells and the run's charts
<scenario>-<target>-r<n>.jsonl        the harness's own stream, verbatim
<scenario>-<target>-r<n>.effects.jsonl  rig's effect log (rig arms only)
<scenario>-<target>-r<n>.err          stderr
```

`manifest.json` is written for a measured run, not for `--reuse`: a re-export is a view of an
existing run, and the identity belongs to the run that produced it.

## The report

Two generated views sit beside the evidence. Neither makes a claim; the authored page does that.

`summary.md` is the human view. Its sections, in order:

| Section | Answers | Columns |
| --- | --- | --- |
| identity | which corpus, checkout, snapshot and runner | corpus fingerprint, checkout head and branch, dirty, snapshot, runner |
| totals | how much was run, and what it cost | runs, arms, pass rate, billed cost, input tokens, cache share |
| By arm | does the arm pass, and what does a passed check cost? | Arm, Runs, Pass rate, Solved, Score (median [min–max]), Cost/check (median [min–max]), Cost/solved, Cache |
| Work | how much the arm did to get there | Arm, Model calls, Tool calls, Input (median tokens), Output, Wall (median), Precision |
| Adoption | did the arm attempt the toolkit, and how heavily? | Arm, Runs, Plugin tools (runs), Plugin calls/run, CLI attempts (runs), CLI attempts/run, Skill runs |
| Score by scenario | where the arm wins and loses | Scenario × arm, median, with min–max when repeats disagree |
| Solved by scenario | how often an arm passed every check in a scenario | Scenario × arm, solved/run — a median score can hide a bimodal arm that this separates |
| Cost per check by scenario | the same in money | Scenario × arm |
| Misses | every failed **scored** check, grouped | scenario, arm, check, source, missed/carried |
| Tool evidence | which `tool_says` checks ran, separately from the score | Arm, Tool checks seen |
| Limits | what the numbers do not license | model, runs per cell, saturated scenarios, dirty checkout, parallel jobs |

Rules the generator follows:

- **Score and Cost/check are the claim.** The score counts `gold`, path and answer checks. Model
  calls, tool calls, output bytes and cache share are diagnostics — a run can do more work and cost
  less because a stable prefix was cached. `tool_says` is **evidence, not score**: it says the
  mechanism ran, and it is reported in its own table so a plugin-only check cannot decide a
  comparison between harnesses.
- **Cost/check and Cost/solved are both reported.** `Cost/check` divides by partial credit, which
  can flatter an arm that passes fewer checks cheaply; `Cost/solved` divides by runs that passed
  every check, which cannot. When they disagree, read the pass rate beside them.
- **`Solved` is all-or-nothing and is reported twice**: per arm, and per scenario. Partial credit is
  the score; a run that passed every check is the unit `Cost/solved` divides by. A scenario whose
  arms all solve it, or none do, is visible in the solved matrix even when the median score moves.
- **A range is min–max across the arm's runs**, so it mixes scenarios with repeats. The scenario
  matrix separates the two.
- **Adoption is counted from the tool-call table**, not from a driver flag: an arm that called
  `at_peek_search` used the toolkit even when it was run before the flag existed. CLI attempts are
  counted by the drivers, which see the shell command's arguments; a run is CLI-attempting if it
  named an `at-*` binary at least once. An attempt that exits 127 is still an attempt — the
  `absent` arm is expected to make them. A result that predates the count shows a dash rather than
  a zero it cannot support.
- **A skill load is a covariate, not a pass.** A non-documented arm that loads a toolkit skill is
  documented in practice: the repository advertisement is removed, not the harness's own skill
  store.
- **A scenario every arm passed in full is named under Limits** as separating nothing. That is the
  saturation warning, generated from the data rather than remembered.
- **The report never states a conclusion.** It says "opencode 91% at 1641 µ$/check; rig:baseline
  82% at 2875"; whether that is good is the authored page's job.

`observable.md` is the interactive view, and it is authored markdown kept in
`integrations/rig-eval/observable/notebook.md` and compiled into the runner. `traces.json` is
mapped once into a flat `runs` table and every chart uses it. It carries `cost_per_check` and
`cache_pct` with nulls preserved, so a run the provider did not price is not plotted as a free
one. `tools.json` drives the tool-call split, and `checks.json` drives the misses table.

The notebook is the run page, not an appendix: it leads with the cost of a solved run, then shows
the per-run spread under that aggregate, a solved-by-scenario matrix sorted by the spread between
arms, how each arm reached the toolkit, what the tools were, what the model read, and what was
missed. The generated tables are a second page beside it. A published analysis forks the cells,
not the data.

The console is a progress log, not a table: one line per finished run with wall time, model and
tool calls, cost per check, cache share, output bytes, score and the checks it missed. It is the only
view available while a run is in flight.

## The published page

`docs/design/rig-eval-results-<date>.md` is authored, not generated. It quotes the run's tables and
adds the argument. Its shape:

1. **The claim**, one paragraph.
2. **The headline table**, copied from `summary.md` (`By arm`), not recomputed.
3. **The ladder reading**, one paragraph: what changed between `absent`, `cli`, `documented` and
   the integrated arm, and whether the change is larger than the spread.
4. **Adoption**, one paragraph plus the table: did the harness actually use what was advertised?
5. **Where it loses**, from `Misses`, grouped by scenario rather than by run.
6. **What would change the conclusion**, from `Limits`, copied.
7. **Links**: the run directory, the corpus fingerprint, the runner commit and the notebook.

A published page never restates a number differently from `summary.md`. If the two disagree, one
of them is a bug.

The notebook ships as-is: `observable.md` plus the three arrays is a complete Observable Framework
page. A public analysis forks the cells, not the data.

## Identity, and how a reader checks it

Every identity field is content-addressed with `git hash-object`, so it is verifiable with git and
needs no trust in the runner:

| Field | Meaning |
| --- | --- |
| `corpus.fingerprint` | hash of the `name\0blob\n` listing of the scenario files |
| `corpus.files[].blob` | `git hash-object --no-filters` of each scenario |
| `checkout.head` / `checkout.branch` | the commit whose tree the scenarios are about |
| `checkout.dirty` | whether the working tree differed; a published run must be `false` |
| `snapshot.commit` | the advertised tip: history with the corpus rewritten out |
| `snapshot.unadvertised` | the same history with the toolkit advertisement rewritten out |
| `targets[].config` | the blob of the settings or config that defines the arm |
| `runner.version` | the binary that wrote the results |

A dirty run is a development run. The snapshot is built with `checkout -f`, so uncommitted changes
do not reach it — but the corpus is read from the working tree and the rig homes compile the plugin
from it, so a dirty tree can change both the questions and the agent. Publish from a clean checkout,
or publish the dirty commit as a different experiment.

`env_names` records the variables the run was given, never their values. A manifest that carries a
credential is a bug.

## The ladder

The question the benchmark exists to answer is whether the toolkit helps, and whether integration
helps on top of the CLI. That is three controlled dimensions:

| Arm | `at-*` on `PATH` | Advertised in the snapshot | Plugin |
| --- | --- | --- | --- |
| `absent` | no — shadowed by a stub that exits 127 | no | — |
| `cli` | yes | no | — |
| `documented` | yes | yes | — |
| `rig:*` | yes | yes | the playbook homes |

**The advertisement is repository content, not harness configuration.** On 2026-11-12 opencode's
`the-skipped-rule` run began `ls -la && ls rules/`, then a `skill` call, and only then
`at-peek search`. The skill it loaded is `skills/inspect-code/SKILL.md` — the repository telling the
harness about its own tools. AGENTS.md also names the toolkit at line 56.

So an "unaware" arm cannot be made by editing a prompt: it removes the advertisement from the
snapshot that arm runs against — `AGENTS.md`, `skills/` and `.skill-store/` removed or stripped of
`at-*` mentions, with the files the scenarios cite left untouched. Every arm in a ladder starts
from the same checkout, so the only difference between `absent`, `cli` and `documented` is the
treatment itself; that is what keeps the comparison fair, and why the manipulation is named in the
manifest rather than assumed.

The `skill` call is itself a measurable covariate: a run that loads a toolkit skill is `documented`
in practice whatever the arm says. The run table should report it.

The manipulation is the snapshot and the agent environment, not a prompt: an arm is a target with a
mode — `--target opencode:absent` — and the runner applies it to that arm's snapshot and `PATH`.
The manifest records `toolkit: { mode, on_path, advertised }` per arm, so a row cannot be read
without knowing what it was shown.

The report groups by the arm's label, so the label carries both dimensions —
`opencode:absent`, `opencode:cli`, `opencode:documented`, `rig:baseline` — and one run holds the
whole ladder. A merged report over four run directories would have four identities to reconcile and
no way for a row to name the snapshot it ran against; one run, one manifest, one table is the
target. The snapshot template therefore holds a commit per treatment, and a job resets to its arm's
commit rather than the template's single HEAD.

## What ships, and what does not

| Ships | Does not ship |
| --- | --- |
| the 13 scenarios and their schema | environment values, keys, tokens |
| `manifest.json`, `results.json`, `summary.md` | absolute paths outside the checkout |
| `traces.json`, `tools.json`, `checks.json`, `observable.md` | raw streams by default |
| the arm configs (`settings.toml`, `opencode.json`) after secret review | any run from a dirty checkout, labelled clean |

Raw streams and effect logs contain repository content and the model's full reasoning. Publish them
only as an archive after review, or publish a redacted sample. The scores never depend on them: every
number in `summary.md` is re-derivable from `results.json`, and every row of `traces.json` from the
run's own stream *as recorded by the runner*, not by the harness.

## Claims the artefact supports

- **Cost per passed check and cost per solved task, per arm**, with the pass rate beside them. The
  first is the unit the invoice is made of; the second is what a completed task costs. They
  diverge when an arm passes fewer checks for less money. Model calls and token counts are
  reported as diagnostics, because prompt caching makes them disagree with cost by design.
- **Adoption and evidence**: whether a harness attempted the CLI, called a plugin tool, or loaded
  a skill that advertises it; and which `tool_says` evidence appeared. Evidence is reported, not
  scored.
- **The ladder**: absent → CLI → documented → integrated, on the same corpus and model.
- **Null results**, including the ones already found: the advertised rig arm does *more* work than
  the plain one; the frontier corpus saturates at 1.00 for every arm; the outline verb displaced
  heading searches (3 → 0) without reducing total calls in one run.

## Claims it does not support

- General coding ability. The corpus is one repository's docs and code.
- Cross-model behaviour. Every run names one model; a different model is a different experiment.
- Cross-platform reproduction. The isolation uses macOS `sandbox-exec`; a Linux rerun needs a
  container or an explicitly weaker control.
- A ranking between rig plugin variants at n < 3. The between-run spread currently exceeds the
  between-arm difference.

## Reproduction

```sh
cargo run --manifest-path integrations/rig-eval/Cargo.toml -- \
  --out <dir> \
  --target opencode:absent --target opencode:cli --target opencode:documented \
  --target rig:baseline \
  --repeat 3 --jobs 2 --timeout 300 --publish site/artifacts
```

The model key is read from the env file named by `--env-file` (default
`.tmp/rig-home/model.env`); only its variable name reaches the manifest. The corpus is a directory
of JSON files whose schema is `integrations/rig-eval/scenarios/README.md`; adding a scenario is a
pull request against the corpus, and changes the fingerprint.

One repeat is a smoke test. A published comparison is the median of at least three, with the spread
shown, on a named model, from a clean checkout.
