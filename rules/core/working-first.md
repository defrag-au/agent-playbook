---
id: working-first
title: Working first — fix problems, don't hide them
layer: core
activation: always
priority: 90
overrides:
targets:
---

## Directive

- Deliver working functionality before optimising architecture. Fix the cause; never comment
  out, disable, or route around a problem to get a build green.
- Facing a compile error, a lifetime fight or a design question → ask **"what is the simplest
  thing that makes this actually work?"**, not "what is the correct architecture?".
- Stop and reconsider if you are about to:
  - add a `// TODO:` to disable functionality that was supposed to work
  - comment out code to silence a compilation error
  - fight type/lifetime/async issues before the plain logic is proven
  - rewrite a signature to make an error go away rather than understanding it
  - say "we'll implement that later" about a core feature rather than an edge
  - spend longer on the shape of the code than on whether the feature works
- Deviate only when continuing would break something that works, introduce a security hole,
  or risk data corruption — and then fix it properly, do not disable it.

**Make it work, make it right, make it fast — in that order.**

## Rationale

There are two paths when something does not compile, and they diverge immediately.

**Fix and prove.** Identify the root cause. Implement the minimal working solution. Verify it
end-to-end. *Then* improve it.

**Hide and perfect.** Comment out the code that will not compile. Fight an async or lifetime
problem before the basic logic is proven. Optimise something that has never run. Prioritise
"it builds" over "it works".

The second path is faster for about ten minutes and then costs a session. It is also
self-concealing: a disabled feature looks like progress in a diff, and the discovery of the
failure moves to whoever runs the code next.

Green lights, for contrast: the user can exercise the feature right now · core functionality
works even if the implementation is plain · each change leaves the working state working ·
problems are being solved rather than hidden · value is demonstrable this session rather than
next.
