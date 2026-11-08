# Findings: capture, report, decide

A **finding** is something an agent noticed in a session and could not afford to turn into a rule
at that moment. This is the loop that keeps it from being lost, and keeps it from becoming a rule
nobody can retire.

The predecessor loop fused noticing with production. `skills/capture-rule` asked an agent, at the
moment of noticing, to choose a layer, write a rule, verify it activates and install it — a
production pipeline triggered by an observation. Two failures followed: the finding was never
captured because producing a rule cost more than the incident, or it became a plausible-looking
rule from one incident that could never be retired because nothing recorded what it was for.

`docs/inventory.md` is the ledger this loop feeds, not the loop itself. It was written
retrospectively, at consolidation time.

## The record

One file per finding, in the repository the agent was working in:

```
<repo>/.playbook/findings/<YYYY-MM-DD>-<slug>.md
```

```markdown
---
captured: 2026-11-05
project: mitos
thread: 5f7ff843-cb59-43ed-b7eb-b3bd0207ee25
message: 40
class: unknown
source: instrument
---
`at-peek slice ...:263-647` was printed as the next read. The next call was the same verb on
the same file, narrowed to `320-400`.

The exit's bound is "the rest of the file", which is not the bound the reader wanted.
```

| Field | Meaning |
| --- | --- |
| `captured` | The date. Findings are dateable so a stale backlog is visible as stale. |
| `project` | The `project:` from the repository's `.playbook/project.conf`. |
| `thread` | The Zed thread id the evidence is in. Empty when the finding came from a conversation. |
| `message` | The message index in that thread. Together with `thread`, it reopens the evidence. |
| `class` | One of the classes below, or `unknown`. |
| `source` | `agent` (noticed while working), `instrument` (surfaced by a trace read), `user` (a correction). |

The body is two sentences: what happened, and what would have been better. **Not the fix, not the
rule, not the layer.** Those are the decision's job, and writing them at capture is how a finding
becomes an unretirable rule.

Open findings are **not committed**. `.playbook/findings/` belongs in the repository's
`.gitignore`: the directory is a working set on the machine that captured it, and a tracked
observation is product debt that every reviewer then feels responsible for.

The **decision** is committed, in `.playbook/FINDINGS.md` — one heading per decided finding,
including the ones that produced nothing. That file is the durable half: the rule is the
conclusion, the `docs/inventory.md` entry is the global provenance, and the log is the
repository's own memory of what was considered.

## The classes

The class decides which layer can fix it. Getting it wrong is why interventions do not stick: a
rule cannot fix a missing capability, and a tool cannot impose "always".

| Class | Symptom | Fix lives in | Will not work |
| --- | --- | --- | --- |
| `missing-capability` | A four-command pipeline for one question | a new verb or flag | a rule naming something that does not exist |
| `missing-constraint` | Right by luck; wrong next time | a rule | a tool — it is only one option among many |
| `misleading-surface` | A failure that reads as something else; a bound silently discarded | the wording, or the error path | a doc — nobody reads it at the moment of failure |
| `undiscoverable` | The capability exists and never loads | the declaration or default | more prose beside it |
| `split-truth` | Two files disagree | one source, one render | an override |
| `platform-limit` | A hard ceiling, not a bug | architecture, or a documented bound | a retry |
| `human-cost` | Approvals, interruptions | the trust boundary | a faster tool inside the same boundary |

`class: unknown` is preferred to a guess. The report stage is allowed to reclassify.

## Report

**A finding is an observation. An issue is a group of findings sharing a class and a cause.**
Do not file an issue; let the third occurrence create one. A single finding is an anecdote.

Trigger: the third finding in a class, or a request.

```sh
playbook findings                          # the repository you are in
playbook findings --dir ~/code/defrag      # every repository below a directory
playbook findings --class missing-capability
playbook findings --stale 30               # open, and captured over a month ago
playbook findings --json
```

It needs no playbook root and no manifest — it reads `.playbook/findings/` in any repository, so
it works for one that has never adopted the composer. `gitignore` does not affect it: the scan
reads the filesystem. A finding that does not parse is a warning, not a silent drop.

The pilot on 2026-11-05 used `at-peek search` for this and could not: `at-peek` has no verb that
lists a directory, and its `tree` verb is documented but not in the binary. Both are findings.

The report is one screen: findings grouped by class, each group naming the `thread @ message`
that reopens it.

```sh
zed-thread-audit --behaviour <thread-id-or-title> --limit 0
```

That is the enrichment step. It is what turns "a file read felt expensive" into a count.

## Decide

A decision is a section appended to the repository's `.playbook/FINDINGS.md` — the committed half
of the record — and the finding's file is deleted.

```markdown
## 2026-11-05 · slice exits name the rest of the file
`fix-now` · rule `rules/org/defrag/slice-exits-window` · measure: exits-followed on a depth-first task
```

The heading is the date and what the finding was. The first backticked outcome under it is the
decision. The rest is detail, and what it has to contain depends on the outcome: a `fix-now` names
the rule and the measure that would retire it, a `fix-later` names the trigger, and
`accept-as-is` and `reject` say why.

The log is append-only. A heading with no outcome is listed rather than dropped, and a fenced
block in the file is documentation, not an entry.

Four outcomes, and all four are legitimate:

| Outcome | Meaning |
| --- | --- |
| `fix-now` | An intervention, with the measure named **before** it is built. |
| `fix-later` | Deferred, with the condition that would trigger it. |
| `accept-as-is` | The behaviour is correct; the finding was wrong. |
| `reject` | Not a real problem. |

Two disciplines make the loop work:

- **Only a decision creates a rule.** A finding is captured cheaply and might never become one.
- **A `fix-now` with no measure is not a decision, it is a wish.** The exits experiment is the
  model: one measure, a stated reading, and deletion as a legitimate result. If the number does
  not move, the intervention is removed rather than extended.

`accept-as-is` is not a cop-out. A low exits-followed ratio is partly "the exit was wrong" and
partly "the agent asked a better question". Both are true, and only one is a defect. Without
`accept-as-is`, the backlog fills with correct behaviour.

## Lifecycle

A finding is a working-set entry, not an archive. Every outcome appends to the log and deletes the
file; what differs is what the entry has to contain.

| Outcome | The log entry |
| --- | --- |
| `fix-now` | The rule or tool change, and the measure that would retire it. The evidence is copied into the `docs/inventory.md` entry beside the rule. |
| `fix-later` | The condition that would trigger the work. |
| `accept-as-is` | Why the behaviour is correct. |
| `reject` | Why it is not a problem. |

Deleting is the point rather than a side effect: the loop should produce rules and measurements,
not a transcript of everything considered. Re-capturing a rejected finding costs one file, which
is what makes losing it acceptable.

`accept-as-is` and `reject` are the entries that stop the same observation being raised again,
which is why they are written down rather than dropped.

A finding that turns out to be about exactly one repository becomes a rule in that repository's
`.playbook/rules/` at `fix-now`, exactly as the table says.

## Pilot

Seeded 2026-11-05 in `mitos` and `cnft.dev-workers`, from traces read with
`zed-thread-audit --behaviour`. `.playbook/findings/` is ignored in `mitos`,
`cnft.dev-workers` and `augminted-bots`; the third has no findings yet. Both seeded findings came
from the instrument rather than from a correction, which is the case this loop most needs to
handle: nothing was wrong enough to interrupt a session, and nothing would have been written down.
