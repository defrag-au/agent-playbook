# chrono 0.4.45 (chronotope/chrono)

## At a glance

- The 0.4 API is stable since the version you know; the traps are the feature set and the types.
- `clock` is a superset of `now`; `default-features = false` plus `clock,std` is the lean build.
- `parse_from_rfc3339` returns `DateTime<FixedOffset>`, not `DateTime<Utc>` — convert before comparing.
- `signed_duration_since` can be negative; a future timestamp breaks a `> 0` assumption.
- This sheet answers RFC 3339 parse, age and storage formatting.

## Cargo.toml

```toml
chrono = { version = "0.4", default-features = false, features = ["clock", "std"] }
```

`clock` is a superset of `now`; it provides `Utc::now()`. The `serde` feature is only needed if a
`DateTime` is itself a serialized field. Keeping timestamps as strings avoids it.

## Parse an RFC 3339 timestamp

```rust
use chrono::{DateTime, Utc};

let updated: DateTime<Utc> = DateTime::parse_from_rfc3339(value)?.with_timezone(&Utc);
```

Both `Z` and `+00:00` suffixes parse. The result is `DateTime<FixedOffset>`; convert to `Utc`
before comparing or subtracting.

## Ages

```rust
let age_seconds = Utc::now().signed_duration_since(updated).num_seconds();
```

`num_seconds()` can be negative if the stored timestamp is in the future. Do not assume it is
positive; a fixture with a future timestamp will make a `> 0` assertion fail.

## Format for storage

```rust
let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
```

The second argument uses `Z` instead of `+00:00`. Zed stores timestamps in RFC 3339 and parses
both forms.

## Parse a date-only argument

```rust
let date = chrono::NaiveDate::parse_from_str("2026-10-17", "%Y-%m-%d")?;
```

`NaiveDate` has no timezone. A filter like "older than this date" should compare
`updated.date_naive() < cutoff`.

## Gotchas

- Do not compare RFC 3339 strings to decide chronology: `Z` and `+00:00` sort differently at the
  offset character even when they denote the same instant.
- `Local` needs the `clock` feature and reads the OS timezone; use `Utc` for stored data.
- `parse_from_rfc3339` accepts an offset but not a bare date or a missing offset.
