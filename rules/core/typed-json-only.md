---
id: typed-json-only
title: Typed structs only — no serde_json::Value, no json! macro
layer: core
activation: always
priority: 78
overrides:
targets:
---

Never use `serde_json::Value` or the `serde_json::json!()` macro for data structures, API
responses, configuration, queue messages, SSE payloads, or anything persisted.

Define concrete types with `#[derive(Serialize, Deserialize)]`.

## Banned

- `serde_json::Value`, `json!()`
- `Map<String, Value>`, `Vec<Value>`, any composition of `Value`
- `#[serde(untagged)]` as a way to avoid deciding on a shape

## Required

- Concrete structs and enums with serde derives
- `#[serde(tag = "type")]` for tagged unions rather than a stringly-typed discriminant field
- `Option<T>` for nullable fields, newtypes for ids and timestamps
- `#[serde(flatten)]` where a shape genuinely composes

```rust
// no
let event = json!({ "type": "progress", "message": msg, "pct": pct });

// yes
#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
enum CatchupEvent {
    Progress { message: String, pct: u8 },
    Complete { assets: u32 },
}
```

## Limited exceptions

Allowed only with a justification written in the code:

1. Consuming a genuinely dynamic external API whose schema cannot be known
2. Temporary debugging code that will be removed
3. A low-level JSON utility whose whole purpose is handling arbitrary JSON

## Why

Type safety catches the error at compile time instead of in production. Structs document the
shape. The frontend can deserialise into a matching type instead of indexing into a map.
Schema generation works. And when the shape changes, the compiler finds every call site
rather than the runtime finding one.
