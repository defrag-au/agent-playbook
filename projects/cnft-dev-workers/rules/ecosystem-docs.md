---
id: cnft-ecosystem-docs
title: Check the ecosystem docs before changing a cross-service contract
layer: project
activation: project:cnft-dev-workers
priority: 45
overrides:
targets:
---

This service is part of the larger defrag ecosystem, and the contracts between services are
documented outside this repo:

- `~/code/defrag/ecosystem-docs/SYSTEM-OVERVIEW.md`
- `~/code/defrag/ecosystem-docs/SERVICE-MAP.md`
- `~/code/defrag/ecosystem-docs/DATA-FLOWS.md`
- `~/code/defrag/ecosystem-docs/API-CONTRACTS.md`
- `~/code/defrag/ecosystem-docs/EVENT-SCHEMAS.md`

Before changing anything that crosses a service boundary — an API response shape, an event
schema, a queue message — read the relevant doc. If the change invalidates it, **update the
doc in the same change.** A contract documented in one place and implemented in another is a
contract that is already drifting.

These are the source of truth for what other services expect. This repo's code is the
implementation of them, not the definition.
