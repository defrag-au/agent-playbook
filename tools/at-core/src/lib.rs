//! `at-core` — the output contract and path containment shared by the agent toolkit.
//!
//! Every `at-*` binary links this. The contract is what makes the tools cheap to read and safe to
//! allowlist, and it has to be identical in all of them or it is not a contract — a bound stated
//! one way in `at-peek` and another in `at-recall` is worse than no bound at all.
//!
//! It carries no dependencies, so the binary that spawns `git` links this and std and nothing
//! else.

pub mod catalogue;
pub mod contract;
pub mod paths;
