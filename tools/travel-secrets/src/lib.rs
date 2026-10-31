//! `travel-secrets` — a whitelisted, recipient-encrypted pass-store subset.
//!
//! Machine-independent by design: the inputs are a store, a whitelist, a recipient public key
//! and an output directory. It never reads a fleet profile and never contacts a host. It
//! decrypts with the source key and re-encrypts to the recipient in a pipe; plaintext is never
//! written to disk, and the source store is never modified.

pub mod cli;
pub mod fsutil;
pub mod gpg;
pub mod manifest;
pub mod payload;
pub mod selection;
pub mod whitelist;

/// The name a transcript and an allowlist entry use.
pub const TOOL: &str = "travel-secrets";

/// The crate version, so a caller can tell two installs apart.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Run with the arguments after the binary name. Returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    cli::run(args)
}
