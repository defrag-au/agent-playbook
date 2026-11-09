//! What a run drives: one of the rig homes, opencode, or Pi.

use std::path::Path;

use anyhow::Result;

use crate::cli::Home;
use crate::driver::{self, RunContext};
use crate::report::RunResult;
use crate::scenario::Scenario;

#[derive(Debug, Clone)]
pub enum Target {
    Rig(Home),
    Opencode,
    Pi,
}

impl Target {
    pub fn label(&self) -> String {
        match self {
            Target::Rig(home) => format!("rig:{}", home.label),
            Target::Opencode => "opencode".to_owned(),
            Target::Pi => "pi".to_owned(),
        }
    }

    pub fn is_rig(&self) -> bool {
        matches!(self, Target::Rig(_))
    }
}

pub fn run_one(
    target: &Target,
    scenario: &Scenario,
    workdir: &Path,
    repeat: u32,
    context: &RunContext<'_>,
) -> Result<RunResult> {
    match target {
        Target::Rig(home) => driver::rig::run(scenario, workdir, home, repeat, context),
        Target::Opencode => driver::opencode::run(scenario, workdir, repeat, context),
        Target::Pi => driver::pi::run(scenario, workdir, repeat, context),
    }
}
