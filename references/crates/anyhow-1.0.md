# anyhow 1.0.104 (dtolnay/anyhow)

## At a glance

- `anyhow::Result<T>` is `Result<T, anyhow::Error>`; the error carries a chain of context strings.
- `.context("...")` and `.with_context(|| ...)` attach a message to any `std::error::Error`, and to
  `Option<T>`.
- `bail!("...")` returns early; `anyhow!("...")` constructs an error value.
- `{error:#}` prints the chain on one line; `{error:?}` prints it with each cause on its own line.
- This sheet answers binary-shaped error handling: `main` returning an exit code, and every
  fallible step keeping the path that failed.

## Cargo.toml

```toml
anyhow = "1.0"
```

## Attach context while preserving the cause

```rust
use std::path::Path;

use anyhow::{Context, Result};

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}
```

`with_context` takes a closure, so the message is built only when the result is an error. Use
`.context("...")` when the message is a constant.

## Return an error from a function that has none

```rust
use anyhow::{bail, Result};

fn check(repeat: u32) -> Result<()> {
    if repeat == 0 {
        bail!("`repeat` must be at least 1");
    }
    Ok(())
}
```

## Print the chain from `main`

```rust
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rig-eval: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
```

## Gotchas

- Do not use `anyhow::Error` in a library's public API: callers cannot match on it. It is for a
  binary or an application boundary.
- `?` converts any `E: std::error::Error + Send + Sync + 'static`. A type that is not
  `Send + Sync` needs `.map_err(...)` first.
- `.context()` on an `Option<T>` turns `None` into an error; import the same `Context` trait.
- `RUST_BACKTRACE=1` is read by `{error:?}`, not by `{error:#}`.
