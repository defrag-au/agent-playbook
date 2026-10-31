//! `at-cardano` — tier 6 of the agent toolkit: read-only Cardano chain enquiry.
//!
//! The first adaptor is Koios. The grammar is closed in the same sense the other `at-*`
//! binaries are closed: no write path, no arbitrary host, no redirect, no subprocess, and
//! `/submittx` is refused by name. The one capability this binary adds over its siblings is
//! a network read, so the name of the binary is what a caller allowlists for "ask the chain a
//! question", not "run an HTTP client".
//!
//! The grammar is **noun + aspect**: `at-cardano pool delegators <pool>`. The noun is what you
//! are asking about; the aspect is what you want from it. All 100 read endpoints in the
//! published Koios spec are reachable through a verb, and `at-cardano endpoints` proves it
//! rather than claiming it.
//!
//! One verb answers one question. `at-cardano run --scenario <file.dsl>` asks several and carries
//! a value out of one answer into the next; the grammar is in `tools/at-cardano/SCENARIO.md`,
//! and it is the same catalogue underneath, so a scenario can ask nothing the command line
//! could not be asked.

pub mod catalogue;
pub mod cli;
pub mod endpoints;
pub mod key;
pub mod koios;
pub mod model;
pub mod scenario;
pub mod verbs;

pub use at_core::contract;

/// The name a transcript and an allowlist entry use.
pub const TOOL: &str = "at-cardano";

/// The crate version, so a caller can tell two installs apart.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Run with the arguments after the binary name. Returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    cli::run(args)
}
