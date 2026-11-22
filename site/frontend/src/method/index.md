# How this was measured

This is the contract behind every number on the site. It is deliberately short: the detailed
artefact specification lives in the repository at `docs/design/benchmark-artifact.md`, and the
corpus schema at `integrations/rig-eval/scenarios/README.md`.

## The corpus

A directory of JSON scenarios. Each names a prompt, a working directory, and the checks its answer
must satisfy:

- **`gold`** — `path:line` citations the answer must contain.
- **`extra_paths`** — paths the answer must name; they also enter the precision denominator.
- **`must_say`** — literal strings the answer must contain.
- **`tool_says`** — literal strings a tool's output must contain. This is **evidence the work
  happened**, reported separately; it is not scored.

Every check is validated against the snapshot **before** a run starts. A stale `path:line` is a
runner error, not a model failure. The corpus is versioned by a fingerprint of its file contents;
each run page names the fingerprint it measured.

## Scoring

- **Score** is scored checks passed divided by scored checks attempted. The scored checks are
  `gold`, `extra_paths`, `must_say` and the working-tree expectations; `tool_says` is evidence and
  is not in either sum.
- **Pass rate** is the same sum across an arm's runs.
- **Solved** is a run that passed every **scored** check. It is stricter than score, and it is the
  unit `Cost/solved` divides by.
- **Precision** is the share of cited `path:line` references that match the gold set. It is
  reported as a diagnostic, never as a score.

Tool evidence is reported in its own table. It can show that the mechanism ran, but it cannot make
an arm that answered correctly look worse, or one that answered incorrectly look better.

## Isolation

Every run executes in a snapshot checkout, not in the working tree:

- the corpus is removed from **every commit's history** in the snapshot, so `git show` cannot read
  the expected answer;
- the snapshot sits on an `eval` branch with one file left uncommitted, so a scenario about the
  working tree has a deterministic answer;
- on macOS, `sandbox-exec` denies the corpus directory, the results directory and `.git`.

A run that names the sealed corpus is marked **inspected**: the read was refused. A tool output
that contains a corpus document is marked **contaminated**, and a contaminated run is excluded
from every table and median. It stays in the results file so the exclusion is auditable.

## Cost

**Billed cost per passed check** and **billed cost per solved task** are both reported. The first
is the unit the invoice is made of; the second is what completing a task costs. They diverge when
an arm passes fewer scored checks for less money, which is why the pass rate sits beside them.

All three drivers normalise `input_tokens` to **every input token sent**, including the part the
provider served from its prompt cache. The `Cache` column is the share served from cache. It
explains the token counts; it is not a discount the runner applies.

`Cost/solved` is the arm's total bill divided by the runs that passed every **scored** check. A
dash means it cannot be stated: no solved run, or no price.

## The ladder

The arms vary one thing at a time:

| Arm | `at-*` on `PATH` | Advertised in the repository | Plugin |
| --- | --- | --- | --- |
| `absent` | no — shadowed by a stub that exits 127 | no | — |
| `cli` | yes | no | — |
| `documented` | yes | yes | — |
| `rig:*` | yes | yes | the playbook homes |

The advertisement is repository content, not harness configuration: `AGENTS.md`, `skills/` and the
skill store are part of the snapshot. That is why the ladder is a controlled experiment rather
than a prompt.

A run that loads a toolkit skill is **documented in practice** whatever its arm says, so the
adoption table reports skill loads beside plugin calls and CLI attempts.

## Limits

- **One model per run.** The model is named on the run page; comparing models is a different
  experiment.
- **One corpus.** These are this repository's design notes, rules and integration code. The
  results do not describe general coding ability.
- **macOS.** Isolation uses `sandbox-exec`; a Linux run needs a container or an explicitly weaker
  control.
- **Repeats, not rates, when `n` is small.** A single run per scenario is a smoke test. The limits
  section of each report says how many runs a cell holds.
- **Saturation.** A scenario every arm passes in full separates nothing. The report names those
  scenarios rather than letting a table imply a difference.
