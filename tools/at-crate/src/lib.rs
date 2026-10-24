//! `at-crate` — tier 1 of the agent toolkit: read-only inspection of installed crate source.
//!
//! What makes `at-crate *` safe to allowlist is the same negative property `at-peek` has: it
//! never spawns a process, never opens a socket, and has no write path in any form. It reads the
//! cargo home (`--cargo-home`, `CARGO_HOME`, or `~/.cargo`), the project's `Cargo.lock`, and the
//! project's `target` directory, and it says which of those it used. It never runs `cargo`, so it
//! cannot fetch, build or extract anything — an uninstalled crate is an answer, not an action.
//!
//! The tools are separate binaries because they are separate trust tiers. This one reads a store
//! outside the worktree, which is why it is not a verb on `at-peek` — but it is still read-only,
//! still bounded, and still states what it did not show.

pub mod catalogue;
pub mod cli;
pub mod project;
pub mod store;
pub mod target;
pub mod verbs;
pub mod walk;

pub use at_core::contract;

/// The name a transcript and an allowlist entry use.
pub const TOOL: &str = "at-crate";

/// The crate version, so a caller can tell two installs apart.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Run with the arguments after the binary name. Returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    cli::run(args)
}
