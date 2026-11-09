//! One module per harness: the command it runs and the stream it writes.

pub mod opencode;
pub mod pi;
pub mod rig;

use crate::cli::Args;

pub struct RunContext<'a> {
    pub args: &'a Args,
    pub env: &'a [(String, String)],
    pub sandbox: Option<String>,
}
