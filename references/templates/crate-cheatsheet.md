# <crate> <version> (<owner>/<repo>)

<!--
Copy this file to references/crates/<crate>-<version>.md and fill it in.
Written for an agent that was NOT trained on this version — see references/README.md.
Delete any section that does not apply; do not leave placeholders behind.
-->

## Cargo.toml

```toml
# The exact version researched. A cheat sheet without a pin is a claim about `latest`.
<crate> = { version = "<version>", features = ["<feature>"] }
```

## Key Differences from <version you probably know>

<!--
The highest-value section. The model's recollection is some older release, so lead with
what changed: renames, signature changes, removals. Be concrete — "X was renamed to Y"
not "the API was reworked".
-->

- `<old>` was renamed to `<new>`
- `<fn(a, b)>` now takes `<fn(a, b, c)>` — the third argument is `<what>`

## <Task: how to do the thing you actually came here for>

```rust
// Complete and compilable. A snippet that omits the imports is a snippet that has to be
// reconstructed from memory, which is what this file exists to avoid.
```

## <Second task>

```rust
```

## Gotchas

<!--
The traps: a default that is not what you would assume, an ordering requirement, a
feature flag that silently changes behaviour, an error that means something else.
-->
