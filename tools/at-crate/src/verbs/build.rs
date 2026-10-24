//! Choosing a build-script output directory, and refusing to choose when there is more than one.
//!
//! Cargo names each build directory `<crate>-<hash>`, and a hash changes when features, the
//! profile, the target or a dependency changes. The result is that a project can hold several
//! `out` directories for one crate, only one of which the last build wrote. This module treats
//! that as an ambiguity to report, not as a newest-wins guess: `--build-dir` is the visible token
//! that chooses.

use std::fs;
use std::path::{Path, PathBuf};

use at_core::contract::Fail;

use crate::project::Project;
use crate::store::normalised;
use crate::target::{self, CrateTarget};
use crate::verbs::Options;
use crate::walk::{self, BuildOut};

/// The one build output a `slice --build` target names.
pub fn locate(
    target: &CrateTarget,
    project: &Project,
    target_dir: Option<&Path>,
    opts: &Options,
) -> Result<(BuildOut, PathBuf), Fail> {
    let target_dir = target_dir.ok_or_else(|| {
        Fail::environment(
            "no target directory found · --project <path> or --target-dir <path> names one",
        )
    })?;

    if let Some(explicit) = &opts.build_dir {
        let out = out_dir(explicit)?;
        let found = checked_out(&out, target)?;
        let path = target::resolve_in(&out, &target.inner, &target.crate_name)?;
        if !path.is_file() {
            return Err(Fail::environment(format!(
                "{} is not a file in {}",
                target.inner,
                out.display()
            )));
        }
        return Ok((found, path));
    }

    let matching: Vec<BuildOut> = walk::build_outs(target_dir)
        .into_iter()
        .filter(|out| normalised(&out.crate_name) == normalised(&target.crate_name))
        .collect();

    if matching.is_empty() {
        return Err(Fail::environment(format!(
            "no build-script output for {} under {} · build first, or point --target-dir at the \
             directory that holds it",
            target.crate_name,
            target_dir.display()
        )));
    }

    let mut hits: Vec<(BuildOut, PathBuf)> = Vec::new();
    for out in matching {
        if let Ok(path) = target::resolve_in(&out.out, &target.inner, &target.crate_name) {
            if path.is_file() {
                hits.push((out, path));
            }
        }
    }

    match hits.len() {
        1 => Ok(hits.remove(0)),
        0 => Err(missing(target, project, target_dir, opts)),
        _ => {
            let mut lines = Vec::new();
            for (out, path) in &hits {
                lines.push(format!(
                    "{} · {}",
                    out.label(project.root.as_deref()),
                    path.display()
                ));
            }
            Err(Fail::usage(format!(
                "{} matches {} build-script output directories · --build-dir <path> picks one\n  {}",
                target.describe(),
                hits.len(),
                lines.join("\n  ")
            )))
        }
    }
}

/// Every build output a `search --build` target names. The caller searches all of them because
/// rows carry their own directory; `--build-dir` narrows it to one.
pub fn search_roots(
    target: &CrateTarget,
    target_dir: Option<&Path>,
    opts: &Options,
) -> Result<Vec<BuildOut>, Fail> {
    let target_dir = target_dir.ok_or_else(|| {
        Fail::environment(
            "no target directory found · --project <path> or --target-dir <path> names one",
        )
    })?;

    if let Some(explicit) = &opts.build_dir {
        let out = out_dir(explicit)?;
        return Ok(vec![checked_out(&out, target)?]);
    }

    let matching: Vec<BuildOut> = walk::build_outs(target_dir)
        .into_iter()
        .filter(|out| normalised(&out.crate_name) == normalised(&target.crate_name))
        .collect();
    if matching.is_empty() {
        return Err(Fail::environment(format!(
            "no build-script output for {} under {} · build first, or point --target-dir at the \
             directory that holds it",
            target.crate_name,
            target_dir.display()
        )));
    }
    Ok(matching)
}

/// An explicit `--build-dir` can be either the build directory or its `out`.
fn out_dir(given: &Path) -> Result<PathBuf, Fail> {
    let resolved = fs::canonicalize(given).map_err(|error| {
        Fail::environment(format!(
            "cannot read --build-dir {}: {error}",
            given.display()
        ))
    })?;
    if !resolved.is_dir() {
        return Err(Fail::usage(format!(
            "--build-dir {} is not a directory",
            given.display()
        )));
    }
    if resolved.file_name().is_some_and(|name| name == "out") {
        return Ok(resolved);
    }
    let out = resolved.join("out");
    if out.is_dir() {
        Ok(out)
    } else {
        Ok(resolved)
    }
}

fn parse_out(out: &Path, expected_crate: &str) -> BuildOut {
    let parent = out.parent().unwrap_or(out);
    let parsed = parent
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .and_then(|name| walk::split_build_dir(&name));
    let (crate_name, hash) =
        parsed.unwrap_or_else(|| (expected_crate.to_string(), "explicit".into()));
    BuildOut {
        crate_name,
        hash,
        dir: parent.to_path_buf(),
        out: out.to_path_buf(),
    }
}

fn checked_out(out: &Path, target: &CrateTarget) -> Result<BuildOut, Fail> {
    let found = parse_out(out, &target.crate_name);
    if found.hash != "explicit" && normalised(&found.crate_name) != normalised(&target.crate_name) {
        return Err(Fail::usage(format!(
            "--build-dir {} belongs to {}, not {} · name the build directory for the crate you \
             asked about",
            out.display(),
            found.crate_name,
            target.crate_name
        )));
    }
    Ok(found)
}

pub fn missing(target: &CrateTarget, project: &Project, target_dir: &Path, opts: &Options) -> Fail {
    let mut lines = Vec::new();
    for out in walk::build_outs(target_dir)
        .into_iter()
        .filter(|out| normalised(&out.crate_name) == normalised(&target.crate_name))
    {
        lines.push(format!("  {}", out.label(project.root.as_deref())));
        if let Ok(files) = walk::files(&out.out, 12, opts.include_secret_paths) {
            for file in files.files.iter().take(12) {
                lines.push(format!("    {}", file.rel));
            }
        }
    }
    Fail::environment(format!(
        "no {} in the build-script output for {} under {}\n{}",
        target.describe(),
        target.crate_name,
        target_dir.display(),
        lines.join("\n")
    ))
}
