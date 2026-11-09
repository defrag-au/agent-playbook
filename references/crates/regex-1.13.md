# regex 1.13.1 (rust-lang/regex)

## At a glance

- `Regex::new(pattern)` returns `Result<Regex, regex::Error>`; compile once, not per call.
- A `static` is a `std::sync::LazyLock<Regex>`; matching is linear-time and never backtracks
  catastrophically.
- Named captures are `(?P<name>...)`; `captures_iter` yields `Captures` with `name("...")`.
- There is no lookahead or lookbehind: express a boundary in the pattern, or filter the match.
- This sheet answers the `path:line` scan in `integrations/rig-eval`.

## Cargo.toml

```toml
regex = "1.13"
```

## Compile once in a static

```rust
use std::sync::LazyLock;

use regex::Regex;

static CITATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?P<path>[A-Za-z0-9_./~-]+\.(?:rs|md|toml))(?::(?P<line>\d+))?")
        .expect("the pattern is a constant")
});

for captures in CITATION.captures_iter(text) {
    let path = captures.name("path").map(|found| found.as_str());
    let line = captures
        .name("line")
        .and_then(|found| found.as_str().parse::<usize>().ok());
    // ...
}
```

## Match one occurrence

```rust
if let Some(found) = CITATION.find(text) {
    println!("{}", found.as_str());
}
```

`find` is the cheap answer when the groups are not needed; `is_match` is cheaper still.

## Gotchas

- `.` matches any character except `\n` by default. Use `(?s)` only when a match must cross lines.
- The pattern is a raw string, so `\.` is a literal dot; without `r"..."` the backslash needs
  doubling.
- A group that did not participate is `None`, not an empty string.
- A greedy class such as `[A-Za-z0-9_./~-]+` includes a URL's `//`; filter the match when the
  surrounding text can be a URL.
- `Regex` is `Send + Sync`; a `LazyLock` is the whole of the guard. Do not add a mutex.
