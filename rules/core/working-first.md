---
id: working-first
title: Working first — fix problems, don't hide them
layer: core
activation: always
priority: 90
overrides:
targets:
---

Deliver working functionality before optimising architecture. Fix the cause of a problem;
never comment it out, disable it, or route around it to get a build green.

When facing a compilation error, a lifetime fight, or a design question, the first question
is **"what is the simplest thing that makes this actually work?"** — not "what is the correct
architecture for this?". Prove the concept end-to-end, then iterate.

## The two paths

**Fix and prove.** Identify the root cause. Implement the minimal working solution. Verify it
end-to-end. *Then* improve it.

**Hide and perfect.** Comment out the code that will not compile. Fight an async or lifetime
problem before the basic logic is proven. Optimise something that has never run. Prioritise
"it builds" over "it works".

The second path is faster for about ten minutes and then costs a session.

## Red flags — stop and reconsider

- Adding a `// TODO:` to disable functionality that was supposed to work
- Commenting out code to silence a compilation error
- "We'll implement that later" applied to a core feature rather than an edge
- Fighting type/lifetime/async issues before the plain logic is proven
- Spending longer on the shape of the code than on whether the feature works
- Rewriting a signature to make an error go away rather than understanding it

## Green lights — keep going

- The user can exercise the feature end-to-end right now
- Core functionality works, even if the implementation is plain
- Each change leaves the working state working
- Problems are being solved rather than hidden
- Value is demonstrable this session, not next session

## Exceptions

Deviate only when continuing would break something that currently works, would introduce a
security hole, or would risk data corruption. Even then: fix it properly. Do not comment it
out and do not disable it.

**Make it work, make it right, make it fast — in that order.**
