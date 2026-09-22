//! `at-recall` — tier 2 of the agent toolkit: read-only inspection of history and the working
//! tree.
//!
//! It differs from `at-peek` in exactly one way, and the difference is the reason it is a separate
//! binary: it reads `.git`. A history tool can reach content that no longer exists — a secret
//! committed and then removed — which is a capability no `cat` approval ever granted, so it gets
//! its own grant rather than riding on the one for the working tree.
//!
//! It is still read-only, and that is structural: every git command it can run comes from a closed
//! enum of read verbs ([`git::Noun`]), the invocation neutralises the configuration that could
//! make git execute something else, and it never reads the reflog.

pub mod catalogue;
pub mod cli;
pub mod git;
pub mod status;
pub mod verbs;

/// The name a transcript and an allowlist entry use.
pub const TOOL: &str = "at-recall";

/// The crate version, so a caller can tell two installs apart.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Run with the arguments after the binary name. Returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    cli::run(args)
}
