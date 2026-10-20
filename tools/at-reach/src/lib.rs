//! `at-reach` — tier 3 of the agent toolkit: read-only fleet inspection.
//!
//! The machines in a fleet do not need to know about each other. The agent does need a map:
//! which host exists, what it is for, how to reach it, and what its working tree looks like.
//! That map lives in a private fleet tree, not in this public repository.
//!
//! `at-reach` reads that tree and runs one fixed, read-only probe either locally or over SSH.
//! It has no write verb, no sudo path and no secret access. The probe itself is one `sh` or
//! `ssh` invocation; local detection reads the hostname, and that distinction is why this is a
//! separate binary from `at-peek` and `at-recall`.

pub mod catalogue;
pub mod cli;
pub mod fleet;
pub mod probe;

/// The name a transcript and an allowlist entry use.
pub const TOOL: &str = "at-reach";

/// The crate version, so a caller can tell two installs apart.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Run with the arguments after the binary name. Returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    cli::run(args)
}
