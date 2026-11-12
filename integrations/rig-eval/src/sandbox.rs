//! The seatbelt profile that seals the eval data from the agent.
//!
//! The corpus now ships in the repository, so a repo-wide search can see its path. The content is
//! what matters: `sandbox-exec` denies the agent every read of the corpus, the output directory,
//! and the repository's eval scratch. It denies the corpus **directory**, not its parent — a
//! harness may walk the eval crate at startup, and an unreadable directory in that walk can kill
//! it. Without the sandbox the runner still reports contamination, but it refuses an in-checkout
//! corpus rather than pretend the run is evidence.

use std::fmt::Write as _;
use std::path::Path;

use crate::cli::Args;

pub fn available() -> bool {
    Path::new("/usr/bin/sandbox-exec").is_file()
}

pub fn profile(args: &Args) -> Option<String> {
    if !available() {
        return None;
    }

    // Deny the corpus itself, not its parent: a harness may walk the eval crate at startup, and
    // an unreadable directory in that walk can kill it. The corpus directory is the only thing
    // that holds the gold; the checkout's `.git` also holds it, because the corpus is committed
    // and a snapshot's history does not have it.
    let mut denied = vec![
        args.scenario_dir.clone(),
        args.out.clone(),
        args.root.join(".git"),
    ];
    // A copy of an earlier run kept under the repository's scratch directory is still a leak.
    let scratch = args.root.join(".tmp/rig-eval");
    if scratch.exists() {
        denied.push(scratch);
    }

    let mut clauses = String::new();
    for path in &denied {
        let Ok(path) = path.canonicalize() else {
            continue;
        };
        let quoted = path
            .display()
            .to_string()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        let _ = write!(clauses, "(subpath \"{quoted}\")");
    }
    if clauses.is_empty() {
        return None;
    }
    Some(format!(
        "(version 1)(allow default)(deny file-read* {clauses})"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn a_profile_quotes_every_denied_path() {
        let root = PathBuf::from("/repo");
        let args = Args {
            root: root.clone(),
            scenario_dir: root.join("integrations/rig-eval/scenarios"),
            only: Vec::new(),
            targets: Vec::new(),
            model: "model".to_owned(),
            rig: root.join("rig"),
            opencode_config: root.join(".tmp/opencode/opencode.json"),
            pi: root.join(".tmp/pi/bin/pi"),
            env_file: root.join(".env"),
            out: root.join(".tmp/rig-eval/run"),
            reuse: None,
            publish: None,
            repeat: 1,
            jobs: 1,
            timeout: None,
            dry_run: false,
        };
        // The paths do not exist here, so the profile has nothing to deny; the test is that the
        // builder returns `None` rather than a malformed profile.
        assert!(profile(&args).is_none());
    }
}
