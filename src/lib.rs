//! agent-playbook — resolve and render agent rules for a project + target.
//!
//! The resolution model is documented in `docs/precedence.md`; the rule format in
//! `docs/rule-format.md`. This crate is the single implementation of both.

pub mod frontmatter;
pub mod install;
pub mod load;
pub mod model;
pub mod render;
pub mod resolve;
