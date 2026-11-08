---
id: defrag-upstream-patch-readiness
title: A fork goes upstream only when every backend is covered and it reads like the project
layer: org
activation: org:defrag
priority: 60
overrides:
targets:
---

## Directive

Do not open — or offer to open — an upstream PR for a fork until two tests pass:

1. **Every backend the upstream builds for is covered.** A flag that is warned-and-ignored on
   one platform is a feature that platform does not have. Enumerate the upstream's backends and
   make each one work, or leave the patch local.
2. **The patch reads as if the project wrote it.** Match the comment density, naming and where a
   config field lives. Explanations written for a diff belong in the fork's commit message, not
   in a file upstream maintains.

Until both are true, the fork is the deliverable: pin it, carry it in `[patch.crates-io]`, and
leave the PR unopened. Do not end the work by offering to send it.

## Rationale

The miniquad fork for `Conf::window_hidden` (2026-11-08) was not ready on either count. macOS
was verified and X11/Windows were guarded, but Wayland was only warned about — and hiding a
Wayland window cannot be a `Conf` flag: `eglCreateWindowSurface` is built on a `wl_egl_window`,
and `eglSwapBuffers` attaches the buffer that maps the xdg surface, so it needs a headless EGL
surface instead. The patch had also not had the style pass miniquad's own code implies. The fork
stayed pinned at `34bf014` and was not sent.

An upstream reviewer's first question is "does this work everywhere", and a PR that has to be
reworked costs the maintainer time and makes the next contribution harder. Answering that after
the PR is open is too late.

## Applies to

Forked upstream dependencies — `macroquad`, `miniquad`, and any future `[patch.crates-io]` entry
pointing at someone else's repository. A fork of a defrag crate is not this rule.
