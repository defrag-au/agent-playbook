---
id: cnft-ecosystem-docs
title: Check the ecosystem docs before changing a cross-service contract
layer: project
activation: project:cnft-dev-workers
priority: 45
overrides:
targets:
---

## Directive

Cross-service contracts are documented outside this repo, in `~/code/defrag/ecosystem-docs/`:
`SYSTEM-OVERVIEW.md`, `SERVICE-MAP.md`, `DATA-FLOWS.md`, `API-CONTRACTS.md`, `EVENT-SCHEMAS.md`.

- Crossing a service boundary (an API response shape, an event schema, a queue message) → read
  the relevant doc first.
- The change invalidates the doc → update the doc in the same change.

Source of truth for what other services expect — this repo's code implements them, it does not
define them.

## Rationale

This service is part of the larger defrag ecosystem, and the contracts between services are
documented outside this repo. A contract documented in one place and implemented in another is
a contract that is already drifting.
