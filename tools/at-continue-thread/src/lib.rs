//! `at-continue-thread` — tier 2 of the agent toolkit: continue a thread in a new one.
//!
//! Zed stores every thread as a zstd-compressed JSON document in `threads.db`. A thread that has
//! read many screenshots grows to tens of megabytes of base64, and a provider can refuse the next
//! request on body size — a 413 that no amount of context window helps with, because the failure
//! is bytes rather than tokens. `/compact` cannot rescue it either: compaction is a request over
//! the same body.
//!
//! This tool reads one thread, drops the image payloads and the private reasoning, keeps the
//! user's direction, the files touched and a head-and-tail window of the agent's activity, and
//! renders that as a bounded digest. A new thread reads the digest and carries on. The old thread
//! is untouched.
//!
//! It is read-only and has no write path, in any verb or flag: the store is opened with SQLite's
//! read-only flag and `query_only` pragma, and nothing is spawned.

pub mod catalogue;
pub mod cli;
pub mod verbs;

/// The name a transcript and an allowlist entry use.
pub const TOOL: &str = "at-continue-thread";

/// The crate version, so a caller can tell two installs apart.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Run with the arguments after the binary name. Returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    cli::run(args)
}
