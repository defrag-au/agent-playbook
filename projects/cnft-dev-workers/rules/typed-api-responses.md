---
id: cnft-typed-api-responses
title: Concrete response types, never json!()
layer: project
activation: project:cnft-dev-workers
priority: 52
overrides:
targets:
---

Every API response is a concrete typed struct defined in `api-types`. This is the repo-specific
application of [`core/typed-json-only`](../../../rules/core/typed-json-only.md) — that rule says
what is banned, this one says where the types go.

- Define response structs in `api-types/admin` (or the matching module) for **all** response
  types.
- **SSE messages must be typed structs**, not `json!()`. They are the easiest place to slip,
  because each event looks small enough not to deserve a type.
- `Response::from_json(&MyResponse { … })`, never `Response::from_json(&json!({ … }))`.

```rust
// no
json!({ "type": "progress", "message": "…" })

// yes
CatchupProgressEvent { event_type: "progress", message: "…" }
```

## Why the extra rule

The core rule tells you not to reach for `Value`. This one tells you *where the struct goes*,
which is the question that actually stalls the work — and a stalled rule gets worked around.
