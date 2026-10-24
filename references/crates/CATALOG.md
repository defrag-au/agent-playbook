# Crate references

Read this table first. It is the index; the individual files are task-shaped reference
material, not something to load whole.

```sh
at-peek stat references/crates/<file>.md
at-peek slice references/crates/<file>.md:1-25
at-peek search '<symbol>' references/crates/<file>.md
```

Read one reference at a time. Every file opens with `## At a glance`; read that first, then
only the section named below.

Budget: ≤ 200 lines and ≤ 8 KB per file. The test suite enforces a 10 KB ceiling, an
`## At a glance` opening and a row in this table for every file.

| Crate | Version | File | Read first |
| --- | --- | --- | --- |
| chrono | 0.4.45 | `chrono-0.4.md` | `## Parse an RFC 3339 timestamp` |
| clap | 4.6.7 | `clap-4.6.md` | `## Derive a parser` |
| headless_chrome | 1.0.22 | `headless_chrome-1.0.22.md` | `## Launch a browser and own it` |
| liquid | 0.26 | `liquid-0.26.md` | `## Building a Parser with Custom Extensions` |
| ratatui | 0.29.0 | `ratatui-0.29.md` | `## Init, draw, restore` |
| rmcp | 1.3 | `rmcp-1.3.md` | `## Simplest Pattern: #[tool_router(server_handler)]` |
| rusqlite | 0.40.2 | `rusqlite-0.40.md` | `## Open a database read-only` |
| serde | 1.0.229 | `serde-1.0.md` | `## Derive a tolerant reader` |
| serde_json | 1.0.154 | `serde_json-1.0.md` | `## Read and write typed values` |
| ureq | 3.4.2 | `ureq-3.4.2.md` | `## Key differences from ureq 2.x` |
| zstd | 0.14.3 | `zstd-0.14.md` | `## Decompress a frame` |

## What each file answers

| Crate | Use it for |
| --- | --- |
| chrono | RFC 3339 parsing and `DateTime<Utc>` without the serde feature |
| clap | Derive parser, subcommands and the feature set that produces them |
| headless_chrome | Headless Chromium over CDP: process lifecycle, true viewport, input, console events, raw commands |
| liquid | Parser builder, custom filters/tags and partials for the Liquid engine |
| ratatui | Terminal init/draw/restore, the re-exported crossterm and widget layout |
| rmcp | MCP server with `#[tool_router(server_handler)]` and transport-io |
| rusqlite | Bundled SQLite, read-only opens and statement/transaction patterns |
| serde | Tolerant derives, `#[serde(default)]` and `IgnoredAny` for large values |
| serde_json | Typed JSON, `raw_value` and streaming/writer patterns |
| ureq | ureq 3.x native HTTP: rustls, the new `http::Response<Body>` API and no default JSON |
| zstd | Frame decompression/compression and the streaming API |

The rationale for the format — version in the filename, task-shaped sections, complete
snippets — is in [`../README.md`](../README.md).
