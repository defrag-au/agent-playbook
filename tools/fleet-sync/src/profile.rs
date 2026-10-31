//! The parts of a fleet profile this tool needs, and the checks that keep a write local.

use std::path::{Component, Path, PathBuf};

use at_core::contract::Fail;
use at_reach::fleet::Profile;

/// The profile's sync policy. A profile may opt out; an unknown mode is a usage error rather
/// than a silent default, because guessing here writes somewhere it should not.
pub fn ensure_sync(profile: &Profile) -> Result<(), Fail> {
    match profile.sync.as_deref().unwrap_or("bundle") {
        "bundle" => Ok(()),
        "none" => Err(Fail::refused(format!(
            "{}: sync is none · this profile is not a sync target",
            profile.id
        ))),
        other => Err(Fail::usage(format!(
            "{}: unsupported sync mode `{other}` · expected `bundle` or `none`",
            profile.id
        ))),
    }
}

/// A machine that writes a repository is a source of truth, not a satellite. `apply` refuses
/// to reset one unless the caller says so explicitly.
pub fn is_writer(profile: &Profile) -> bool {
    !profile.writes.trim().eq_ignore_ascii_case("none")
}

/// Resolve one repository entry against the profile's `repo_root`, or against an explicit
/// source root for `bundle`. A `..` segment is refused: the inventory is trusted, but a typo
/// must not delete a parent directory.
pub fn repo_path(
    profile: &Profile,
    repo: &str,
    root_override: Option<&Path>,
) -> Result<PathBuf, Fail> {
    if repo.trim().is_empty() {
        return Err(Fail::usage(format!("{}: empty repo entry", profile.id)));
    }

    let candidate = Path::new(repo);
    if candidate
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(Fail::refused(format!(
            "{}: repo `{repo}` contains `..`",
            profile.id
        )));
    }

    if candidate.is_absolute() {
        return Ok(candidate.to_path_buf());
    }

    let root = root_override
        .map(Path::to_path_buf)
        .or_else(|| profile.repo_root.clone())
        .ok_or_else(|| {
            Fail::environment(format!(
                "{}: repo `{repo}` is relative but `repo_root` is missing",
                profile.id
            ))
        })?;
    Ok(root.join(candidate))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(writes: &str, sync: Option<&str>) -> Profile {
        Profile {
            id: "air".to_string(),
            role: "client".to_string(),
            os: "darwin".to_string(),
            arch: "aarch64".to_string(),
            nix_system: None,
            ssh: "local".to_string(),
            tailscale: None,
            tailscale_ip: None,
            tailscale_tag: None,
            shell: None,
            sudo: "none".to_string(),
            repo_root: Some(PathBuf::from("/Users/damo/code")),
            repos: vec!["defrag/shared-crates".to_string()],
            rebuild_repo: None,
            rebuild: "true".to_string(),
            writes: writes.to_string(),
            sync: sync.map(String::from),
            sync_clean: None,
            notes: None,
        }
    }

    #[test]
    fn a_relative_repo_resolves_against_the_root() {
        let path = repo_path(&profile("none", None), "defrag/shared-crates", None).expect("path");
        assert_eq!(path, PathBuf::from("/Users/damo/code/defrag/shared-crates"));
    }

    #[test]
    fn an_absolute_repo_is_left_alone() {
        let path = repo_path(
            &profile("none", None),
            "/Volumes/development/compositor",
            None,
        )
        .expect("path");
        assert_eq!(path, PathBuf::from("/Volumes/development/compositor"));
    }

    #[test]
    fn a_parent_segment_is_refused() {
        let error = repo_path(&profile("none", None), "../secrets", None).expect_err("refused");
        assert!(error.message.contains("contains `..`"));
    }

    #[test]
    fn a_source_root_overrides_the_profile_root() {
        let path = repo_path(
            &profile("none", None),
            "defrag/shared-crates",
            Some(Path::new("/tmp/primary")),
        )
        .expect("path");
        assert_eq!(path, PathBuf::from("/tmp/primary/defrag/shared-crates"));
    }

    #[test]
    fn a_profile_can_opt_out_of_sync() {
        let error = ensure_sync(&profile("none", Some("none"))).expect_err("refused");
        assert_eq!(error.exit, at_core::contract::Exit::Refused);
    }

    #[test]
    fn an_unknown_sync_mode_is_a_usage_error() {
        let error = ensure_sync(&profile("none", Some("rsync"))).expect_err("refused");
        assert_eq!(error.exit, at_core::contract::Exit::Usage);
    }

    #[test]
    fn a_writer_is_recognised() {
        assert!(!is_writer(&profile("none", None)));
        assert!(is_writer(&profile("nix-config", None)));
    }
}
