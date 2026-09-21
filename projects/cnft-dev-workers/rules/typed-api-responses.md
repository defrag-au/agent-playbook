---
id: cnft-typed-api-responses
title: Concrete response types, never json!()
layer: project
activation: project:cnft-dev-workers
priority: 52
overrides:
targets:
---

## Directive

Every API response is a concrete typed struct defined in `api-types`.

- Define response structs in `api-types/admin` (or the matching module) for **all** response
  types.
- SSE messages are typed structs, not `json!()` — each event looks small enough not to deserve
  a type, which is where this slips.
- `Response::from_json(&MyResponse { … })`, never `Response::from_json(&json!({ … }))`.

```rust
// no
json!({ "type": "progress", "message": "…" })

// yes
CatchupProgressEvent { event_type: "progress", message: "…" }
```

## Rationale

Repo-specific application of [`core/typed-json-only`](../../../rules/core/typed-json-only.md).
The core rule tells you not to reach for `Value`; this one tells you *where the struct goes*,
which is the question that actually stalls the work — and a stalled rule gets worked around.
