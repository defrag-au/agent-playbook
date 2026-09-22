---
id: cnft-ncli
title: ncli is built in this workspace — run it through the devshell
layer: project
activation: project:cnft-dev-workers
priority: 55
overrides:
targets:
---

## Directive

`ncli` fronts our deployed services. Built in this workspace from `tools/notification-config`.

```sh
nix develop -c ncli <args>
```

- Never look for it on `PATH` · never `curl` the services it fronts.
- Rebuild before relying on it — the installed one may be stale.

## Rationale

`ncli` is a workspace binary, so a change to any crate it depends on changes `ncli`.
