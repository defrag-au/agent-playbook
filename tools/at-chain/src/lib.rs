//! `at-chain` — tier 6 of the agent toolkit: read-only Cardano chain enquiry.
//!
//! The first and only adaptor is Koios. The grammar is closed in the same sense the other
//! `at-*` binaries are closed: no write path, no arbitrary host, no redirect, no subprocess,
//! and `/submittx` is refused by name. The one capability this binary adds over its siblings
//! is a network read, so the name of the binary is what a caller allowlists for "ask the
//! chain a question", not "run an HTTP client".

pub mod catalogue;
pub mod cli;
pub mod key;
pub mod koios;
pub mod model;
pub mod verbs;

pub use at_core::contract;

/// The name a transcript and an allowlist entry use.
pub const TOOL: &str = "at-chain";

/// The crate version, so a caller can tell two installs apart.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Run with the arguments after the binary name. Returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    cli::run(args)
}
