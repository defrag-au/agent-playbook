---
captured: <YYYY-MM-DD>
project: <the project name from .playbook/project.conf>
thread: <Zed thread id, or leave empty>
message: <message index in that thread, or leave empty>
class: <missing-capability | missing-constraint | misleading-surface | undiscoverable | split-truth | platform-limit | human-cost | unknown>
source: <agent | instrument | user>
---

<!--
Copy to `.playbook/findings/<YYYY-MM-DD>-<slug>.md` in the repository you are working in. That
directory is gitignored: a finding is a working set on this machine, not product. When it is
decided, append a section to `.playbook/FINDINGS.md` and delete this file.

Two sentences: what happened, and what would have been better. Do not write the fix, the rule,
or the layer. Those are the decision's job, and writing them here is how a finding becomes a rule
nobody can retire.

Delete this comment and every placeholder.

Checklist:
  - `thread` and `message` point at the evidence. `zed-thread-audit --behaviour <thread>`
    reopens it.
  - `class: unknown` is preferred to a guess. The report stage may reclassify.
  - If it does not fit in two sentences, it is probably two findings.
-->

<What happened.>

<What would have been better.>
