//! `at-peek` — tier 1 of the agent toolkit: read-only inspection of the working tree.
//!
//! What makes `at-peek *` safe to allowlist is negative: it never opens `.git`, never spawns
//! a process, never opens a socket, and has no write path in any form. There is no flag, no
//! cache and no future option that could make an `at-peek` invocation change a file, so an
//! approval over the whole grammar is a complete description of what can happen. That is the
//! property `tools/at-describe` states as the namespace's contract, and the one
//! `tests/contract.rs` asserts from outside the binary.

pub mod catalogue;
pub mod cli;
pub mod contract;
pub mod paths;
pub mod verbs;

/// The name a transcript and an allowlist entry use.
pub const TOOL: &str = "at-peek";

/// The crate version, so a caller can tell two installs apart.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Run with the arguments after the binary name. Returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    cli::run(args)
}
