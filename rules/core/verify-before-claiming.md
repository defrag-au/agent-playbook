---
id: verify-before-claiming
title: Do not report success you have not observed
layer: core
activation: always
priority: 94
overrides:
targets:
---

Never claim something works, passes, builds or renders unless you ran it and saw the result
in this session.

- "Tests pass" means the test command ran and its output showed them passing. Not "the change
  looks correct".
- "It builds" means a build ran. A successful edit is not a successful build.
- "The widget renders correctly" means it was rendered and looked at — see
  [`org/defrag/look-at-what-you-built`](../org/defrag/look-at-what-you-built.md).
- "Fixed" means the reported symptom was reproduced and then observed to be gone.

If validation was not run, say so plainly and say why — no toolchain reachable, needs a
device, needs credentials. That is a useful answer. An unearned "done" is worse than a
failure report, because it moves the discovery of the failure to me.

## Related failure modes

- **Reporting a partial result as complete.** If three of four things were done, say which
  three.
- **Silently skipping a step.** A step that could not be run must be named in the final
  message, not dropped.
- **Treating a green exit code as evidence.** A command that exits zero while doing nothing —
  a `str.replace` that found no anchor, a test filter that matched no tests — is not
  validation. Check that the output says what you think it says.
- **Extrapolating from a narrower run.** A single-crate build is not a workspace build.
