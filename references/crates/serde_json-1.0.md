# serde_json 1.0.154 (serde-rs/serde)

## At a glance

- Typed values are `from_slice`/`from_str` and `to_vec`/`to_writer`/`to_writer_pretty`.
- `RawValue` captures exact JSON text and is behind the non-default `raw_value` feature.
- A `BTreeMap<String, Box<RawValue>>` round-trips fields you do not model, byte for byte.
- This sheet answers typed read/write, preserving unmodelled fields, and streaming/writer patterns.

## Cargo.toml

```toml
serde_json = { version = "1.0", features = ["raw_value"] }
```

`raw_value` is not on by default. It is required for `RawValue`.

## Read and write typed values

```rust
let thread: ThreadData = serde_json::from_slice(&decoded)?;
let bytes = serde_json::to_vec(&thread)?;
serde_json::to_writer_pretty(std::io::stdout(), &report)?;
```

## Preserve fields you do not model

`RawValue` captures the exact JSON text of a value. A map of raw values round-trips every
top-level field, including fields added by a newer Zed:

```rust
use std::collections::BTreeMap;
use serde_json::value::RawValue;

let mut fields: BTreeMap<String, Box<RawValue>> = serde_json::from_str(text)?;
let messages: Vec<Box<RawValue>> = serde_json::from_str(fields["messages"].get())?;

// Replace only the array that changed; every other value is written back verbatim.
fields.insert(
    "messages".to_owned(),
    RawValue::from_string(serde_json::to_string(&messages)?)?,
);
let rewritten = serde_json::to_vec(&fields)?;
```

`RawValue::from_string` validates that the string is a JSON value. `RawValue` implements
`Serialize`, so a map or vector of them embeds the raw text rather than re-encoding it.

This is the right shape for a versioned private format: it keeps unknown fields and preserves
existing message bytes exactly. It is not a reason to use `Value` for the fields you do model.

## Gotchas

- `RawValue` needs the `raw_value` feature, and a missing feature looks like a missing type:
  `no RawValue in value`.
- `RawValue` does not work reliably through `#[serde(flatten)]`; flatten buffers into an
  intermediate representation and loses the raw text. Use a map of `Box<RawValue>` instead.
- A `BTreeMap` writes keys in sorted order. That is harmless for a JSON object, but do not rely
  on the original key order.
- `serde_json::Value` is a dynamic tree, not a data model. It is appropriate at a test or a
  debugging boundary, not for a stored schema.
