//! `bundle` — build the payload on the machine that has the work.
//!
//! It reads the local checkouts, writes one git bundle per repository plus a manifest, and
//! touches nothing else. The target is not contacted; the payload is carried to it.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use at_core::contract::{plural, Fail, Outcome, Report};
use at_reach::fleet::Fleet;

use crate::profile::{ensure_sync, repo_path};
use crate::{disk, git, manifest, TOOL};

pub fn run(
    host: &str,
    out: &Path,
    source_root: Option<&Path>,
    reference: Option<&str>,
    fleet: &Fleet,
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

    if profile.repos.is_empty() {
        return Err(Fail::environment(format!(
            "{}: the profile names no repositories",
            profile.id
        )));
    }

    fs::create_dir_all(out).map_err(|error| {
        Fail::environment(format!(
            "cannot create the payload directory {}: {error}",
            out.display()
        ))
    })?;
    let out = fs::canonicalize(out)
        .map_err(|error| Fail::environment(format!("cannot resolve {}: {error}", out.display())))?;
    if !out.is_dir() {
        return Err(Fail::environment(format!(
            "{} is not a directory",
            out.display()
        )));
    }

    let mut used = BTreeSet::new();
    let mut entries = Vec::new();
    let mut warnings = Vec::new();

    for repo in &profile.repos {
        let path = repo_path(profile, repo, source_root)?;
        if !path.is_dir() {
            return Err(Fail::environment(format!(
                "{}: {} is not a directory",
                profile.id,
                path.display()
            )));
        }
        if !git::is_work_tree(&path) {
            return Err(Fail::environment(format!(
                "{}: {} is not a git work tree",
                profile.id,
                path.display()
            )));
        }

        let reference = resolve_ref(&path, reference)?;
        let commit = git::capture_trimmed(&path, &["rev-parse", "HEAD"])?;
        let changes = git::tracked_changes(&path)?;
        if changes > 0 {
            warnings.push(format!(
                "{}: {} has {} tracked path(s) changed · the bundle carries the commit, not the working tree",
                profile.id,
                repo,
                changes
            ));
        }

        let bundle = bundle_name(repo, &mut used);
        let bundle_path = out.join(&bundle);
        let bundle_arg = bundle_path.to_string_lossy().into_owned();

        let mut args = vec!["bundle", "create", bundle_arg.as_str(), "HEAD"];
        if reference != "HEAD" {
            args.push(reference.as_str());
        }
        git::run(&path, &args)?;
        git::run(&path, &["bundle", "verify", bundle_arg.as_str()])?;

        let bytes = fs::metadata(&bundle_path)
            .map_err(|error| {
                Fail::environment(format!("cannot read {}: {error}", bundle_path.display()))
            })?
            .len();
        let sha256 = manifest::sha256_file(&bundle_path)?;
        let worktree_bytes = disk::worktree_bytes(&path)?;

        entries.push(manifest::Entry {
            repo: repo.clone(),
            reference,
            commit,
            bundle,
            bytes,
            sha256,
            worktree_bytes,
        });
    }

    let manifest = manifest::Manifest {
        host: profile.id.clone(),
        entries,
    };
    manifest.write(&out)?;

    let total = manifest
        .entries
        .iter()
        .map(|entry| entry.bytes)
        .sum::<u64>();

    let mut report = Report::new();
    report.header(
        TOOL,
        "bundle",
        &fleet.root().display().to_string(),
        &format!("{} · local", profile.id),
    );
    for entry in &manifest.entries {
        report.content(format!(
            "{} · {} · {} · {} · {}",
            entry.repo,
            entry.reference,
            short(&entry.commit),
            entry.bundle,
            disk::human(entry.bytes)
        ));
    }
    report.bound(format!(
        "payload: {} in {} · {}",
        disk::human(total),
        plural(manifest.entries.len(), "bundle"),
        out.display()
    ));
    for warning in &warnings {
        report.bound(warning.clone());
    }
    report.next(
        format!("{TOOL} plan {} --from {}", profile.id, out.display()),
        "reads the payload on the target before writing",
    );

    Ok(Outcome::from_report(report))
}

fn resolve_ref(repo: &Path, requested: Option<&str>) -> Result<String, Fail> {
    if let Some(name) = requested {
        let name = name.trim();
        if name.is_empty() {
            return Err(Fail::usage("--ref is empty"));
        }
        if name == "HEAD" || name.starts_with("refs/") {
            return Ok(name.to_string());
        }
        return Ok(format!("refs/heads/{name}"));
    }

    Ok(match git::branch(repo)? {
        Some(branch) => format!("refs/heads/{branch}"),
        None => "HEAD".to_string(),
    })
}

fn bundle_name(repo: &str, used: &mut BTreeSet<String>) -> String {
    let base = Path::new(repo)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".to_string());
    let base = base
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();

    let mut candidate = format!("{base}.bundle");
    let mut number = 2;
    while !used.insert(candidate.clone()) {
        candidate = format!("{base}-{number}.bundle");
        number += 1;
    }
    candidate
}

fn short(commit: &str) -> &str {
    &commit[..commit.len().min(7)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bundle_name_is_unique_when_two_repos_share_a_basename() {
        let mut used = BTreeSet::new();
        assert_eq!(bundle_name("a/shared", &mut used), "shared.bundle");
        assert_eq!(bundle_name("b/shared", &mut used), "shared-2.bundle");
    }

    #[test]
    fn a_bundle_name_is_safe_for_a_path() {
        let mut used = BTreeSet::new();
        assert_eq!(
            bundle_name("defrag/shared crates", &mut used),
            "shared-crates.bundle"
        );
    }

    #[test]
    fn an_explicit_ref_is_canonicalised() {
        let root = std::env::temp_dir();
        assert_eq!(
            resolve_ref(&root, Some("main")).expect("ref"),
            "refs/heads/main"
        );
        assert_eq!(
            resolve_ref(&root, Some("refs/tags/v1")).expect("ref"),
            "refs/tags/v1"
        );
    }
}
