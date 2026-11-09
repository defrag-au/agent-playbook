# dotenvy 0.15.7 (allan2/dotenvy)

## At a glance

- `dotenvy::from_path_iter(path)` reads a file and yields `Result<(String, String)>` pairs without
  touching the process environment.
- `dotenvy::from_path(path)` and `dotenv()` load the file **into** the process environment.
- Comments, blank lines, quotes and an optional `export` prefix are handled.
- This sheet answers reading a model key file and passing it to a child process.

## Cargo.toml

```toml
dotenvy = "0.15"
```

## Read without leaking values

```rust
use std::path::Path;

use anyhow::{Context, Result};

fn read_env(path: &Path) -> Result<Vec<(String, String)>> {
    let entries = dotenvy::from_path_iter(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let mut vars = Vec::new();
    for entry in entries {
        let (key, value) = entry.with_context(|| format!("parsing {}", path.display()))?;
        vars.push((key, value));
    }
    Ok(vars)
}
```

Pass the pairs to a child with `Command::env`, so they never enter the runner's own environment.

## Load into the process

```rust
dotenvy::from_path(".env")?;
dotenvy::dotenv().ok(); // the nearest .env from the current directory upwards
```

## Gotchas

- `from_path_iter` does **not** set variables; `from_path` and `dotenv` do. Choose deliberately.
- `$` is substituted by default: `${VAR}` and `$VAR` are expanded from earlier lines or the
  environment. An API key containing `$` must be written in single quotes, which are literal, or
  with `\$`.
- A missing file is an error; a file with no entries is a successful empty iterator.
- `dotenv()` walks upwards and stops at the first `.env`; it does not merge several.
