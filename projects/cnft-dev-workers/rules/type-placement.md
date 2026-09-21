---
id: cnft-type-placement
title: Where a type lives is decided by who consumes it
layer: project
activation: project:cnft-dev-workers
priority: 50
overrides:
targets:
---

**If a type is consumed by more than one domain — frontend, workers, data layer — it belongs
in `types/shared`.**

| Location | Contents | May depend on |
| --- | --- | --- |
| `types/shared/` | Primitive types used across domains. No business logic, pure data definitions | Nothing local — this is the foundation |
| `api-types/` | Request/response types for frontend↔backend | `types/shared`, `data`, `cardano-assets` |
| `orchestrator/` | Worker queue message types for internal worker communication | `types/shared`, `cardano-assets`. **Not** `api-types` |
| `data/` | Database layer types | `types/shared`, `collection-config`. **Not** `orchestrator` or `api-types` |

```
types/shared (foundation — no local dependencies)
    ↑
    ├── api-types → data
    ├── orchestrator
    └── data
```

The forbidden edges exist to break cycles. `orchestrator → api-types` and `data → api-types`
both create circular dependencies, which is why a type that both need must be pushed down to
`types/shared` rather than shared by reference.

## Worked examples

- `shared_types::CollectionTag` — used by api-types, orchestrator and data → `types/shared`
- `shared_types::CatchupStyle` — used by api-types and orchestrator → `types/shared`
- `orchestrator::AssetRefresh` — only used in worker queues → stays in `orchestrator`
- `api_types::admin::CatchupEvent` — SSE events, only the frontend needs them → stays in
  `api-types`

## The test

Ask which crates import it. One → keep it local. More than one → it belongs in
`types/shared`, and any cycle that appears on the way is the compiler telling you the type is
at the wrong layer.
