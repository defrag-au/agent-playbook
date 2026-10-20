# clap 4.6.7 (clap-rs/clap)

## At a glance

- The derive API is the 4.x shape you know: `#[derive(Parser)]`, `#[command]`, `#[arg]`.
- `#[arg(long)]` on a `bool` is a flag; on an `Option<T>` it is an optional value.
- Mutually exclusive arguments are an `ArgGroup`, not hand-written validation.
- This sheet answers the derive parser, subcommands and the feature set that produces them.

## Cargo.toml

```toml
clap = { version = "4.6", features = ["derive"] }
```

## Derive a parser

```rust
use clap::Parser;

#[derive(Parser)]
#[command(name = "my-tool", version, about = "One-line summary")]
struct Cli {
    /// Path to the database
    #[arg(long, value_name = "PATH")]
    db: Option<std::path::PathBuf>,

    /// Show at most this many rows
    #[arg(long, default_value_t = 20)]
    top: usize,

    /// Emit JSON
    #[arg(long)]
    json: bool,
}
```

`Parser::parse()` handles `--help`, `--version` and usage errors, exiting with code 2 for a
usage error. `#[arg(long)]` on a `bool` makes a flag; on an `Option<T>` it makes an optional
value.

## Mutually exclusive alternatives

```rust
#[command(group(
    clap::ArgGroup::new("summary_source")
        .required(true)
        .args(["summary_file", "summary"])
))]
struct Cli {
    #[arg(long, value_name = "PATH")]
    summary_file: Option<std::path::PathBuf>,

    #[arg(long, value_name = "TEXT")]
    summary: Option<String>,
}
```

The group name must not be the same as any argument name. Clap 4.6 panics at startup with
`Argument group name 'summary' must not conflict with argument name` if it is.

## Dates and other `FromStr` types

`NaiveDate` implements `FromStr`, but its error type is the reason a plain `Option<NaiveDate>`
field can be awkward in a derive. Parsing a `String` in `run` gives a clearer error:

```rust
#[arg(long, value_name = "YYYY-MM-DD")]
older_than: Option<String>,
```

```rust
let cutoff = cli
    .older_than
    .as_deref()
    .map(|value| chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d"))
    .transpose()?;
```

## Gotchas

- `#[arg(default_value_t = 20)]` requires the field type to implement `Display`.
- A positional argument is `#[arg(index = 1)]`; a long flag is `#[arg(long)]`.
- `long_about` is the paragraph shown under the summary; `about` is the one-line summary.
- `value_parser` is for custom parsing. A `FromStr` error must implement `std::error::Error` and
  be `Send + Sync` to use the default parser.
