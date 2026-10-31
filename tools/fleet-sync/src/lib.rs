//! `fleet-sync` — seed and resync a read-only fleet satellite.
//!
//! Unlike the `at-*` toolkit this tool writes: it removes build artifacts from a satellite's
//! checkouts and resets them to commits carried in a git bundle. That is why it is its own
//! package, and why `apply` refuses to run anywhere but the host the profile names. There is no
//! SSH executor: the tool never reaches another machine.

pub mod bundle;
pub mod cli;
pub mod disk;
pub mod git;
pub mod manifest;
pub mod profile;
pub mod target;

/// The name a transcript and an allowlist entry use.
pub const TOOL: &str = "fleet-sync";

/// The crate version, so a caller can tell two installs apart.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Run with the arguments after the binary name. Returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    cli::run(args)
}
