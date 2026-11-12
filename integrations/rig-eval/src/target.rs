//! What a run drives: one of the rig homes, opencode, or Pi — plus the toolkit treatment.

use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::cli::Home;
use crate::driver::{self, RunContext};
use crate::report::RunResult;
use crate::scenario::Scenario;

/// One dimension of the ladder: what the arm is allowed to see and call.
///
/// The first two are the same repository with the toolkit removed from the snapshot; the third is
/// the repository as it ships. `absent` additionally shadows the binaries on `PATH` with stubs that
/// exit 127, so the treatment cannot be undone by a shell that still has them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolkitMode {
    Absent,
    Cli,
    Documented,
}

impl ToolkitMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Cli => "cli",
            Self::Documented => "documented",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "absent" => Some(Self::Absent),
            "cli" => Some(Self::Cli),
            "documented" => Some(Self::Documented),
            _ => None,
        }
    }

    /// Whether the `at-*` binaries are on the agent's `PATH`.
    pub fn on_path(self) -> bool {
        !matches!(self, Self::Absent)
    }

    /// Whether the snapshot's history still advertises the toolkit.
    pub fn advertised(self) -> bool {
        matches!(self, Self::Documented)
    }
}

#[derive(Debug, Clone)]
pub enum Target {
    Rig(Home, ToolkitMode),
    Opencode(ToolkitMode),
    Pi(ToolkitMode),
}

impl Target {
    /// The arm label the report groups by. The mode is explicit wherever the ladder varies it.
    pub fn label(&self) -> String {
        match self {
            Target::Rig(home, ToolkitMode::Documented) => format!("rig:{}", home.label),
            Target::Rig(home, mode) => format!("rig:{}:{}", home.label, mode.as_str()),
            Target::Opencode(mode) => format!("opencode:{}", mode.as_str()),
            Target::Pi(mode) => format!("pi:{}", mode.as_str()),
        }
    }

    pub fn toolkit(&self) -> ToolkitMode {
        match self {
            Target::Rig(_, mode) | Target::Opencode(mode) | Target::Pi(mode) => *mode,
        }
    }

    pub fn is_rig(&self) -> bool {
        matches!(self, Target::Rig(..))
    }
}

pub fn run_one(
    target: &Target,
    scenario: &Scenario,
    workdir: &Path,
    repeat: u32,
    context: &RunContext<'_>,
) -> Result<RunResult> {
    let mut result = match target {
        Target::Rig(home, _) => driver::rig::run(scenario, workdir, home, repeat, context),
        Target::Opencode(_) => driver::opencode::run(scenario, workdir, repeat, context),
        Target::Pi(_) => driver::pi::run(scenario, workdir, repeat, context),
    }?;
    // The driver knows the harness; the arm label and the treatment are the runner's.
    result.target = target.label();
    result.toolkit = Some(target.toolkit());
    Ok(result)
}
