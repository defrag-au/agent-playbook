# serde 1.0.229 (serde-rs/serde)

## At a glance

- The derive API is unchanged; the leverage is in the attributes.
- Unknown fields are ignored by default; `#[serde(default)]` lets an older document omit a field.
- `IgnoredAny` consumes a value without allocating it — the way to skip base64 payloads and thinking blocks.
- `#[serde(rename = "Text")]` is required when a JSON key is not a valid Rust identifier.
- This sheet answers tolerant readers, skipping large values and enum representations.

## Cargo.toml

```toml
serde = { version = "1.0", features = ["derive"] }
```

## Derive a tolerant reader

```rust
use serde::Deserialize;

#[derive(Deserialize)]
struct ThreadData {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    messages: Vec<Message>,
    #[serde(default)]
    updated_at: Option<String>,
}
```

Unknown fields are ignored by default. `#[serde(default)]` lets an old document omit a field.
Use `#[serde(rename = "Text")]` when a JSON key is not a valid Rust identifier.

## Skip a large value without allocating it

`serde::de::IgnoredAny` consumes the value and throws it away. This is how an image's base64
payload or a long thinking block can be skipped:

```rust
use serde::de::IgnoredAny;

#[derive(Deserialize)]
struct ContentPart {
    #[serde(rename = "Text")]
    _text: Option<IgnoredAny>,
    #[serde(rename = "ToolUse")]
    tool_use: Option<ToolUse>,
}
```

`IgnoredAny` implements `Deserialize` but not `Serialize`; it is for read-only models.

## Externally tagged enums

These JSON shapes are the same enum:

```json
{"User": {"id": "u1"}}
"Resume"
{"Compaction": {"Summary": "..."}}
```

```rust
#[derive(Deserialize)]
enum Message {
    User(UserMessage),
    Agent(AgentMessage),
    Resume,
    Compaction(IgnoredAny),
}
```

A newtype variant reads the object form; a unit variant reads the bare string. There is no need
to wrap the unit variant in an object.

## Two real shapes for one field

`#[serde(untagged)]` is appropriate when stored data genuinely has two shapes across versions,
not as a way to avoid deciding on a model:

```rust
#[derive(Deserialize)]
#[serde(untagged)]
enum ToolResultContent {
    One(ContentItem),
    Many(Vec<ContentItem>),
    Text(String),
}
```

Document why the union exists. If there is only one current shape, model that shape.

## Gotchas

- `#[serde(default)]` on a `Vec` gives an empty vector; on an `Option` it gives `None`; on a
  newtype it requires `Default`.
- `deny_unknown_fields` is the safe choice for a writer that must preserve a format it does not
  fully understand — fail loudly rather than silently dropping fields.
- `#[serde(flatten)]` with `serde_json::value::RawValue` is not reliable; use a map of
  `Box<RawValue>` when every top-level field must survive.
