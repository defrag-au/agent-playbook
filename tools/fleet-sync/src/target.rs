//! `plan` and `apply` — read the payload, decide what the target needs, and (for `apply`) do it.
//!
//! Both run **on the target**, and `apply` refuses unless the hostname matches the profile and
//! the profile is not a writer. There is no remote path: the tool never runs anything over SSH.

use std::fs;
use std::path::{Path, PathBuf};

use at_core::contract::{Exit, Fail, Outcome, Report};
use at_reach::fleet::{Fleet, Profile};
use at_reach::probe::is_local;

use crate::manifest::{self, Entry, Manifest};
use crate::profile::{ensure_sync, is_writer, repo_path};
use crate::{disk, git, TOOL};

/// The space left after the payload is written. A satellite with a small disk should fail the
/// plan rather than fill up in the middle of a reset.
const HEADROOM_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clean {
    Artifacts,
    Reset,
    None,
}

impl Clean {
    pub fn parse(value: &str) -> Result<Clean, Fail> {
        match value.trim() {
            "artifacts" => Ok(Clean::Artifacts),
            "reset" => Ok(Clean::Reset),
            "none" => Ok(Clean::None),
            other => Err(Fail::usage(format!(
                "unknown clean mode `{other}` · expected `artifacts`, `reset` or `none`"
            ))),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Clean::Artifacts => "artifacts",
            Clean::Reset => "reset",
            Clean::None => "none",
        }
    }
}

struct Row {
    entry: Entry,
    path: PathBuf,
    action: &'static str,
    current: Option<String>,
    dirty: usize,
    reclaim: u64,
    free_before: u64,
    free_after: u64,
    problem: Option<String>,
    cleaned: Option<disk::Cleaned>,
    applied: bool,
}

pub fn plan(
    host: &str,
    from: &Path,
    clean: Option<Clean>,
    force: bool,
    allow_writer: bool,
    include_env: bool,
    fleet: &Fleet,
) -> Result<Outcome, Fail> {
    evaluate(
        host,
        from,
        clean,
        force,
        allow_writer,
        include_env,
        fleet,
        false,
    )
}

pub fn apply(
    host: &str,
    from: &Path,
    clean: Option<Clean>,
    force: bool,
    allow_writer: bool,
    include_env: bool,
    fleet: &Fleet,
) -> Result<Outcome, Fail> {
    evaluate(
        host,
        from,
        clean,
        force,
        allow_writer,
        include_env,
        fleet,
        true,
    )
}

#[allow(clippy::too_many_arguments)]
fn evaluate(
    host: &str,
    from: &Path,
    clean: Option<Clean>,
    force: bool,
    allow_writer: bool,
    include_env: bool,
    fleet: &Fleet,
    write: bool,
) -> Result<Outcome, Fail> {
    let profile = fleet.find(host).ok_or_else(|| {
        Fail::usage(format!(
            "unknown host `{host}` · known: {}",
            if fleet.profiles().is_empty() {
                "none".to_string()
            } else {
                fleet.ids()
            }
        ))
    })?;
    ensure_sync(profile)?;

    if !is_local(profile) {
        return Err(Fail::refused(format!(
            "{} is not this machine · `{}` runs only on the host the profile names",
            profile.id,
            if write { "apply" } else { "plan" }
        )));
    }
    if is_writer(profile) && !allow_writer {
        return Err(Fail::refused(format!(
            "{} writes `{}` · refusing to reset a source of truth without --allow-writer",
            profile.id, profile.writes
        )));
    }

    let clean = resolve_clean(profile, clean)?;
    if include_env && clean != Clean::Reset {
        return Err(Fail::usage(
            "--include-env is only meaningful with --clean reset",
        ));
    }

    let from = fs::canonicalize(from)
        .map_err(|error| Fail::environment(format!("cannot read {}: {error}", from.display())))?;
    if !from.is_dir() {
        return Err(Fail::environment(format!(
            "{} is not a directory",
            from.display()
        )));
    }

    let payload = Manifest::read(&from)?;
    if payload.host != profile.id {
        return Err(Fail::refused(format!(
            "the payload was built for `{}`, not `{}`",
            payload.host, profile.id
        )));
    }

    let mut rows = Vec::new();
    for entry in &payload.entries {
        let bundle_path = from.join(&entry.bundle);
        verify_bundle(&bundle_path, entry)?;

        let path = repo_path(profile, &entry.repo, None)?;
        let exists = path.exists();
        let mut current = None;
        let mut dirty = 0;
        let mut reclaim = 0;
        let mut problem = None;

        if exists {
            if !path.is_dir() {
                problem = Some(format!("{} exists but is not a directory", path.display()));
            } else if !git::is_work_tree(&path) {
                problem = Some(format!(
                    "{} exists but is not a git work tree",
                    path.display()
                ));
            } else {
                current = Some(git::head(&path)?);
                dirty = git::tracked_changes(&path)?;
                if dirty > 0 && !force {
                    problem = Some(format!(
                        "{dirty} tracked path(s) are changed · commit, stash or pass --force"
                    ));
                }
                if clean != Clean::None {
                    reclaim = disk::artifact_bytes(&path)?;
                }
            }
        }

        let free_before = disk::free_kb(&path)? * 1024;
        let required = if exists {
            entry.bytes
        } else {
            entry.worktree_bytes
        }
        .saturating_add(HEADROOM_BYTES);
        let available = free_before.saturating_add(reclaim);
        if problem.is_none() && available < required {
            problem = Some(format!(
                "needs {} after cleaning (payload + {} headroom) but only {} is available · run `nix-collect-garbage --delete-old` or free space first",
                disk::human(required),
                disk::human(HEADROOM_BYTES),
                disk::human(available),
            ));
        }

        rows.push(Row {
            entry: entry.clone(),
            path,
            action: if exists { "update" } else { "clone" },
            current,
            dirty,
            reclaim,
            free_before,
            free_after: available,
            problem,
            cleaned: None,
            applied: false,
        });
    }

    let blocked = rows.iter().any(|row| row.problem.is_some());
    if write && !blocked {
        for row in &mut rows {
            apply_row(row, clean, include_env, &from)?;
            row.applied = true;
        }
    }

    let mut report = Report::new();
    report.header(
        TOOL,
        if write { "apply" } else { "plan" },
        &fleet.root().display().to_string(),
        &format!("{} · local", profile.id),
    );

    for row in &rows {
        let current = row.current.as_deref().map(short).unwrap_or("-");
        let state = if row.applied {
            " · applied"
        } else if row.problem.is_some() {
            " · blocked"
        } else {
            ""
        };
        report.content(format!(
            "{} · {} · {} → {} · dirty={} · reclaim={} · free={} → {}{}",
            row.entry.repo,
            row.action,
            current,
            short(&row.entry.commit),
            row.dirty,
            disk::human(row.reclaim),
            disk::human(row.free_before),
            disk::human(row.free_after),
            state,
        ));
    }

    report.bound(format!(
        "payload: {} repositor{} from {} · headroom {} · clean {}",
        rows.len(),
        if rows.len() == 1 { "y" } else { "ies" },
        from.display(),
        disk::human(HEADROOM_BYTES),
        clean.label(),
    ));
    report.bound(if write {
        "free after is measured".to_string()
    } else {
        "free after is an estimate (free now + reclaimable artifacts)".to_string()
    });

    for row in &rows {
        if let Some(problem) = &row.problem {
            report.bound(format!("refused: {} · {problem}", row.entry.repo));
        }
        if let Some(cleaned) = &row.cleaned {
            if !cleaned.removed.is_empty() {
                report.bound(format!(
                    "removed: {} · {} · {}",
                    row.entry.repo,
                    cleaned.removed.join(", "),
                    disk::human(cleaned.bytes),
                ));
            }
            for skipped in &cleaned.skipped {
                report.bound(format!(
                    "left alone: {} · {skipped} is a symlink or not a directory",
                    row.entry.repo,
                ));
            }
        }
    }

    if !write && !blocked {
        report.next(
            format!(
                "{TOOL} apply {} --from {} --clean {}",
                profile.id,
                from.display(),
                clean.label()
            ),
            "writes the plan above",
        );
    }

    let mut outcome = Outcome::from_report(report);
    if blocked {
        outcome.exit = Exit::Refused;
    }
    Ok(outcome)
}

fn resolve_clean(profile: &Profile, requested: Option<Clean>) -> Result<Clean, Fail> {
    if let Some(clean) = requested {
        return Ok(clean);
    }
    match profile.sync_clean.as_deref() {
        Some(value) => Clean::parse(value),
        None => Ok(Clean::Artifacts),
    }
}

fn verify_bundle(bundle: &Path, entry: &Entry) -> Result<(), Fail> {
    let metadata = fs::metadata(bundle)
        .map_err(|error| Fail::environment(format!("cannot read {}: {error}", bundle.display())))?;
    if metadata.len() != entry.bytes {
        return Err(Fail::environment(format!(
            "{}: {} bytes, the manifest says {}",
            bundle.display(),
            metadata.len(),
            entry.bytes
        )));
    }
    let digest = manifest::sha256_file(bundle)?;
    if digest != entry.sha256 {
        return Err(Fail::environment(format!(
            "{}: checksum differs from the manifest",
            bundle.display()
        )));
    }
    Ok(())
}

fn apply_row(row: &mut Row, clean: Clean, include_env: bool, from: &Path) -> Result<(), Fail> {
    let bundle_path = from.join(&row.entry.bundle);
    let bundle_arg = bundle_path.to_string_lossy().into_owned();
    let exists = row.path.exists();

    if clean != Clean::None && exists {
        row.cleaned = Some(disk::clean_artifacts(&row.path)?);
    }

    if exists {
        git::run(
            &row.path,
            &["fetch", "--quiet", &bundle_arg, &row.entry.reference],
        )?;
        git::run(
            &row.path,
            &["reset", "--hard", "--quiet", &row.entry.commit],
        )?;
    } else {
        if let Some(parent) = row.path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                Fail::environment(format!(
                    "cannot create the parent of {}: {error}",
                    row.path.display()
                ))
            })?;
        }
        let target = row.path.to_string_lossy().into_owned();
        git::command(&["clone", "--quiet", &bundle_arg, &target])?;
        git::run(
            &row.path,
            &["reset", "--hard", "--quiet", &row.entry.commit],
        )?;
    }

    if clean == Clean::Reset {
        let mut args = vec!["clean", "-xfd"];
        if !include_env {
            args.push("-e");
            args.push(".env");
        }
        git::run(&row.path, &args)?;
    }

    let head = git::head(&row.path)?;
    if head != row.entry.commit {
        return Err(Fail::environment(format!(
            "{}: HEAD is {head}, expected {}",
            row.path.display(),
            row.entry.commit
        )));
    }
    let dirty = git::tracked_changes(&row.path)?;
    if dirty > 0 {
        return Err(Fail::environment(format!(
            "{}: {dirty} tracked path(s) are still changed after the reset",
            row.path.display()
        )));
    }

    row.current = Some(head);
    row.free_after = disk::free_kb(&row.path)? * 1024;
    Ok(())
}

fn short(commit: &str) -> &str {
    &commit[..commit.len().min(7)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_mode_is_parsed_strictly() {
        assert_eq!(
            Clean::parse("artifacts").expect("artifacts"),
            Clean::Artifacts
        );
        assert_eq!(Clean::parse("reset").expect("reset"), Clean::Reset);
        assert_eq!(Clean::parse("none").expect("none"), Clean::None);
        assert_eq!(
            Clean::parse("everything").expect_err("unknown").exit,
            Exit::Usage
        );
    }

    #[test]
    fn a_clean_mode_from_a_profile_is_used_when_the_flag_is_absent() {
        let profile = at_reach::fleet::Profile {
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
            writes: "none".to_string(),
            sync: Some("bundle".to_string()),
            sync_clean: Some("reset".to_string()),
            notes: None,
        };
        assert_eq!(resolve_clean(&profile, None).expect("clean"), Clean::Reset);
        assert_eq!(
            resolve_clean(&profile, Some(Clean::None)).expect("clean"),
            Clean::None
        );
    }
}
