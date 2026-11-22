//! One module per harness: the command it runs and the stream it writes.

pub mod opencode;
pub mod pi;
pub mod rig;

use std::path::PathBuf;

use crate::cli::{Args, Home};
use crate::process::Isolation;

pub struct RunContext<'a> {
    pub args: &'a Args,
    pub env: &'a [(String, String)],
    pub isolation: &'a Isolation,
    /// The arm label, filesystem-safe, so three opencode treatments do not share a stream file.
    pub arm: String,
}

impl RunContext<'_> {
    /// Where the arm's rig home is, as the harness sees it: the host path, or the path the trial's
    /// container mounts it at.
    pub fn rig_home(&self, home: &Home) -> PathBuf {
        match self.isolation {
            Isolation::Host { .. } => home.path.clone(),
            Isolation::Container(trial) => trial.rig_home(&home.label),
        }
    }

    /// The opencode provider config, as the harness sees it.
    pub fn opencode_config(&self) -> PathBuf {
        match self.isolation {
            Isolation::Host { .. } => self.args.opencode_config.clone(),
            Isolation::Container(_) => PathBuf::from(crate::container::OPENCODE_CONFIG),
        }
    }

    /// The host directory whose `sessions` child holds this run's effect log. Host mode reads the
    /// shared home; container mode reads the directory mounted into the image's home.
    pub fn effects_root(&self, home: &Home) -> PathBuf {
        match self.isolation {
            Isolation::Host { .. } => home.path.clone(),
            Isolation::Container(trial) => trial
                .effects_root()
                .map(PathBuf::from)
                .unwrap_or_else(|| home.path.clone()),
        }
    }
}
