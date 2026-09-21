---
id: typed-json-only
title: Typed structs only — no serde_json::Value, no json! macro
layer: core
activation: always
priority: 78
overrides:
targets:
---

## Directive

- Never `serde_json::Value` or `json!()` for data structures, API responses, configuration,
  queue messages, SSE payloads, or anything persisted. Define concrete types with
  `#[derive(Serialize, Deserialize)]`.
- Banned: `Value` · `json!()` · `Map<String, Value>` · `Vec<Value>` · any composition of
  `Value` · `#[serde(untagged)]` used to avoid deciding on a shape.
- Required: structs and enums with serde derives · `#[serde(tag = "type")]` for tagged unions
  rather than a stringly-typed discriminant field · `Option<T>` for nullable fields and
  newtypes for ids and timestamps · `#[serde(flatten)]` where a shape genuinely composes.

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

Exceptions, allowed only with the justification written in the code: consuming a genuinely
dynamic external API whose schema cannot be known · temporary debugging code that will be
removed · a low-level JSON utility whose whole purpose is handling arbitrary JSON.

## Rationale

Type safety catches the error at compile time instead of in production, and the shape is
documented by the type rather than by a comment. The frontend can deserialise into a matching
type instead of indexing into a map, and schema generation works.

The payoff that matters most is at the point of change: when the shape moves, the compiler
finds every call site rather than the runtime finding one.
