//! `at-cost` — tier 1 of the agent toolkit: what the machine is costing, by repository.
//!
//! Every other member of the namespace answers a question about files. This one answers a
//! question about *processes*: which repository is burning CPU, what an agent left running, and
//! which thread inside one process is hot. That is a new capability rather than a subset of
//! `cat`, so the grant is its own — but the property that makes the namespace safe to allowlist
//! still holds: no subprocess, no socket, no write path in any flag or option. It reads the
//! process table through `libproc`, the same syscalls `ps` uses, and prints what it found.
//!
//! `argv` is the one surface the file tools do not have, so it is the one that carries a guard
//! rail: a flag whose name says it holds a credential has its value replaced before the command
//! is printed, and the caveat says how many were replaced. Nothing else is read — not the
//! environment, not another user's memory.

pub mod catalogue;
pub mod cli;
pub mod sample;
pub mod verbs;

pub use at_core::contract;

/// The name a transcript and an allowlist entry use.
pub const TOOL: &str = "at-cost";

/// The crate version, so a caller can tell two installs apart.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Run with the arguments after the binary name. Returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    cli::run(args)
}
