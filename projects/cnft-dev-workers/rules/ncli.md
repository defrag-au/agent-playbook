---
id: cnft-ncli
title: ncli is built in this workspace — run it through the devshell
layer: project
activation: project:cnft-dev-workers
priority: 55
overrides:
targets:
---

`ncli` is the tool for interacting with our deployed services, and it is built in this
workspace from `tools/notification-config`. Do not look for it on `PATH` and do not reach for
`curl` against the services it fronts.

```sh
nix develop -c ncli <args>
```

Because it is a workspace binary, a change to the crates it depends on changes `ncli` — rebuild
rather than assuming the installed one is current.
