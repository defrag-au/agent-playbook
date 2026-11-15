//! A snapshot checkout for every run.
//!
//! The corpus is committed, so a seatbelt deny on `scenarios/` only hides the working-tree files:
//! `git show` and `git diff` can still read the expected answers from the real checkout's history.
//! Each run therefore executes in a copy of the checkout whose history has the corpus removed from
//! every commit. The branch history itself survives — `draft-pr` names real commits, and its
//! scenarios must still be able to read them.
//!
//! The copy is the source tree (about 5 MB) plus the real `.git` (about 5 MB). Build directories
//! are symlinked to the real checkout's warm `target` directories: a cold `cargo test` per
//! snapshot would dominate the run, and cargo's own lock serialises the workers that share a
//! target. Wall time is therefore under contention and is not comparable across arms; the other
//! measures are unaffected.
//!
//! A snapshot sits on an `eval` branch at the rewritten tip, with one file left modified. That
//! gives `pre-edit-state` a deterministic answer (`eval`, plus the dirty file) instead of
//! depending on whatever happens to be uncommitted in the real checkout.
//!
//! A `direnv` shim at the front of `PATH` delegates `direnv exec` to the real devshell and then
//! runs the command in the snapshot. The copied `.envrc` therefore needs no allow entry.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::cli::Args;
use crate::target::ToolkitMode;

/// Where the corpus lives in this repository, whatever `--scenarios` points at.
const REPO_CORPUS: &str = "integrations/rig-eval/scenarios";
/// The one file a snapshot leaves uncommitted, so `pre-edit-state` has a stable expectation.
pub const DIRTY_FILE: &str = "docs/design/rig-eval.md";
const MARKER: &str = "\n<!-- rig-eval snapshot: uncommitted -->\n";
const BRANCH: &str = "eval";

pub struct Template {
    pub path: PathBuf,
    /// The corpus-rewritten tip: the repository as it ships.
    pub advertised: String,
    /// The same history with the toolkit's advertisement rewritten out.
    pub unadvertised: String,
    /// The first `PATH` entry for a run: a `direnv` shim that uses the real devshell.
    pub bin: PathBuf,
    /// The `at-*` stubs the `absent` arm puts before everything else on `PATH`.
    pub stubs: PathBuf,
    /// A `direnv` shim that injects the stubs into the devshell too, so `direnv exec . at-peek`
    /// cannot bypass the treatment.
    pub bin_absent: PathBuf,
}

impl Template {
    pub fn build(args: &Args) -> Result<Self> {
        let base = base_dir();
        if base.exists() {
            fs::remove_dir_all(&base).with_context(|| format!("clearing {}", base.display()))?;
        }
        let path = base.join("template");
        fs::create_dir_all(&path).with_context(|| format!("creating {}", path.display()))?;
        let bin = base.join("bin");
        let bin_absent = base.join("bin-absent");
        let stubs = base.join("absent-bin");
        fs::create_dir_all(&bin).with_context(|| format!("creating {}", bin.display()))?;
        fs::create_dir_all(&bin_absent)
            .with_context(|| format!("creating {}", bin_absent.display()))?;
        write_stubs(&stubs)?;
        // The wrappers delegate to the host's devshell through direnv. A container has no direnv
        // and needs none: the image's `PATH` is the toolchain.
        if args.container.is_none() {
            write_wrapper(&bin, args, None)?;
            write_wrapper(&bin_absent, args, Some(&stubs))?;
        }
        link_fork(&base, args)?;

        // Start from the real history, then take the corpus out of every commit. The older
        // commits keep their hashes, so a scenario that names them still works.
        copy_dir(&args.root.join(".git"), &path.join(".git"))?;
        git(&path, &["config", "gc.auto", "0"])?;
        git(&path, &["checkout", "-f"])?;
        let corpus = corpus_paths(args);
        rewrite(&path, &corpus)?;
        git(&path, &["reset", "--hard", "HEAD"])?;
        git(&path, &["checkout", "-f", "-B", BRANCH, "HEAD"])?;

        let advertised = output(&path, &["rev-parse", "HEAD"])?;
        let unadvertised = rewrite_advertisement(&path, &advertised, &base)?;
        git(&path, &["checkout", "-f", "-B", BRANCH, &advertised])?;

        link_targets(args, &path)?;
        exclude_targets(&path)?;
        verify_clean(&path, &corpus)?;
        verify_advertisement(&path, &unadvertised)?;

        seed(&path)?;
        Ok(Self {
            path,
            advertised,
            unadvertised,
            bin,
            stubs,
            bin_absent,
        })
    }

    /// The commit an arm executes at. The mode is the treatment; the commit is the repository it
    /// sees, and `cli`/`absent` share one because only `PATH` differs between them.
    pub fn commit_for(&self, toolkit: ToolkitMode) -> &str {
        if toolkit.advertised() {
            &self.advertised
        } else {
            &self.unadvertised
        }
    }
}

/// Copy the template for one worker. The worker keeps its build directories between jobs.
pub fn worker(template: &Template, index: usize) -> Result<PathBuf> {
    let base = template
        .path
        .parent()
        .context("the template has no parent")?;
    let path = base.join(format!("worker-{index}"));
    discard(&path);
    copy_dir(&template.path, &path)?;
    Ok(path)
}

/// Restore a worker to the template commit, keeping the shared build directories.
pub fn reset(worker: &Path, commit: &str) -> Result<()> {
    git(worker, &["checkout", "-f", "-B", BRANCH, commit])?;
    git(worker, &["clean", "-fd"])?;
    let scratch = worker.join(".tmp");
    if scratch.exists() {
        fs::remove_dir_all(&scratch).with_context(|| format!("clearing {}", scratch.display()))?;
    }
    seed(worker)
}

pub fn discard(path: &Path) {
    let _ = fs::remove_dir_all(path);
}

fn base_dir() -> PathBuf {
    std::env::temp_dir().join(format!("rig-eval-snapshots-{}", std::process::id()))
}

/// The corpus directories to remove from the snapshot: the repo's own, plus an external path.
fn corpus_paths(args: &Args) -> Vec<PathBuf> {
    let mut paths = vec![PathBuf::from(REPO_CORPUS)];
    if let Ok(relative) = args.scenario_dir.strip_prefix(&args.root) {
        paths.push(relative.to_path_buf());
    }
    paths.sort();
    paths.dedup();
    paths
}

/// Rewrite the commits that contain the corpus, then prune the old objects.
///
/// Only the range from the oldest corpus commit's parent is rewritten. The commits before it keep
/// their hashes, so a scenario that names them still works.
fn rewrite(path: &Path, corpus: &[PathBuf]) -> Result<()> {
    let mut log = vec![
        "log".to_owned(),
        "--reverse".to_owned(),
        "--format=%H".to_owned(),
        "--".to_owned(),
    ];
    log.extend(corpus.iter().map(|path| path.display().to_string()));
    let log: Vec<&str> = log.iter().map(String::as_str).collect();
    let commits = output(path, &log)?;
    let Some(oldest) = commits.lines().next() else {
        return Ok(());
    };
    let range = match output(path, &["rev-parse", &format!("{oldest}^")]) {
        Ok(_) => format!("{oldest}^..HEAD"),
        // The corpus was in the root commit; every commit after it changes hash anyway.
        Err(_) => "--all".to_owned(),
    };

    let paths = corpus
        .iter()
        .map(|path| shell_quote(path))
        .collect::<Vec<_>>()
        .join(" ");
    let filter = format!("git rm -r --cached --ignore-unmatch -- {paths}");
    let result = Command::new("git")
        .arg("-C")
        .arg(path)
        .env("FILTER_BRANCH_SQUELCH_WARNING", "1")
        .args([
            "-c",
            "commit.gpgsign=false",
            "filter-branch",
            "--force",
            "--index-filter",
            &filter,
            "--prune-empty",
            "--",
            &range,
        ])
        .output()
        .context("running git filter-branch")?;
    if !result.status.success() {
        bail!(
            "git filter-branch in {}: {}",
            path.display(),
            String::from_utf8_lossy(&result.stderr).trim()
        );
    }

    let refs = output(
        path,
        &["for-each-ref", "--format=%(refname)", "refs/original"],
    )?;
    for reference in refs.lines() {
        git(path, &["update-ref", "-d", reference])?;
    }
    git(path, &["reflog", "expire", "--expire=now", "--all"])?;
    git(path, &["gc", "--prune=now"])?;
    Ok(())
}

/// Rewrite the advertisement out of every commit on a second branch.
///
/// The corpus rewrite has already run, so this branch starts at its tip. The filter removes
/// `skills/` and `.skill-store/` and the "Read code and history with the agent tools" section of
/// `AGENTS.md` from every commit, so `git show` cannot undo the treatment. The advertised branch is
/// restored afterwards; both tips stay reachable.
fn rewrite_advertisement(path: &Path, advertised: &str, base: &Path) -> Result<String> {
    let branch = "eval-unadvertised";
    git(path, &["checkout", "-f", "-B", branch, advertised])?;
    let log = output(
        path,
        &[
            "log",
            "--reverse",
            "--format=%H",
            "--",
            "AGENTS.md",
            "skills",
            ".skill-store",
        ],
    )?;
    let Some(oldest) = log.lines().next() else {
        return Ok(advertised.to_owned());
    };
    let range = match output(path, &["rev-parse", &format!("{oldest}^")]) {
        Ok(_) => format!("{oldest}^..{branch}"),
        Err(_) => branch.to_owned(),
    };

    let script = base.join("filter-advertisement.sh");
    write_filter_script(&script)?;
    let filter = format!("sh {}", shell_quote(&script));
    let result = Command::new("git")
        .arg("-C")
        .arg(path)
        .env("FILTER_BRANCH_SQUELCH_WARNING", "1")
        .args([
            "-c",
            "commit.gpgsign=false",
            "filter-branch",
            "--force",
            "--tree-filter",
            &filter,
            "--prune-empty",
            "--",
            &range,
        ])
        .output()
        .context("running git filter-branch for the advertisement")?;
    if !result.status.success() {
        bail!(
            "git filter-branch (advertisement) in {}: {}",
            path.display(),
            String::from_utf8_lossy(&result.stderr).trim()
        );
    }
    let unadvertised = output(path, &["rev-parse", "HEAD"])?;

    // The filter may have updated `eval` because it pointed at the same tip. Restore it before
    // dropping the backups, so the advertised commit stays reachable.
    git(
        path,
        &["update-ref", &format!("refs/heads/{BRANCH}"), advertised],
    )?;
    git(path, &["checkout", "-f", BRANCH])?;
    let refs = output(
        path,
        &["for-each-ref", "--format=%(refname)", "refs/original"],
    )?;
    for reference in refs.lines() {
        git(path, &["update-ref", "-d", reference])?;
    }
    git(path, &["reflog", "expire", "--expire=now", "--all"])?;
    git(path, &["gc", "--prune=now"])?;
    Ok(unadvertised)
}

fn write_filter_script(path: &Path) -> Result<()> {
    let script = r#"#!/bin/sh
set -e

git rm -r --ignore-unmatch -q skills .skill-store >/dev/null 2>&1 || true

if [ -f AGENTS.md ]; then
  # Drop the generated agent-tools section, then the two repo-specific lines that
  # still name the toolkit. Everything else in AGENTS.md is left alone.
  awk 'BEGIN { skip=0 } /^## Read code and history with the agent tools/ { skip=1; next } skip && /^## / { skip=0 } { if (!skip) print }' AGENTS.md > AGENTS.md.new
  mv AGENTS.md.new AGENTS.md

  perl -pi -e 's/`playbook`, `agent-tools`/`playbook`/' AGENTS.md
  perl -pi -e 's/for `packages\.agent-tools`, and wires the binaries/and wires the binaries/' AGENTS.md
  perl -pi -e 's|^Use \[.*skills/capture-rule.*$|The short version:|' AGENTS.md

  grep -v -e 'at-' -e 'at_' -e 'agent-tools' -e 'skills/' AGENTS.md > AGENTS.md.new || true
  mv AGENTS.md.new AGENTS.md
  git add AGENTS.md
fi

git add -A
"#;
    fs::write(path, script).with_context(|| format!("writing {}", path.display()))
}

/// The unadvertised tip must not name the toolkit in `AGENTS.md`, and must not carry the skill
/// directories. This is the check that the treatment is really absent, not just hidden.
fn verify_advertisement(path: &Path, commit: &str) -> Result<()> {
    let agents = output(path, &["show", &format!("{commit}:AGENTS.md")]).unwrap_or_default();
    for needle in ["at-", "at_"] {
        if agents.contains(needle) {
            bail!("unadvertised snapshot still mentions `{needle}` in AGENTS.md");
        }
    }
    let tree = output(path, &["ls-tree", "-r", "--name-only", commit])?;
    for line in tree.lines() {
        if line == "skills"
            || line.starts_with("skills/")
            || line == ".skill-store"
            || line.starts_with(".skill-store/")
        {
            bail!("unadvertised snapshot still contains {line}");
        }
    }
    Ok(())
}

/// The `at-*` names the `absent` arm shadows. A stub is silent: the arm is not told the toolkit
/// exists, only that the command is not available.
fn write_stubs(bin: &Path) -> Result<PathBuf> {
    fs::create_dir_all(bin).with_context(|| format!("creating {}", bin.display()))?;
    for name in crate::stream::TOOLKIT {
        let path = bin.join(*name);
        fs::write(&path, "#!/bin/sh\nexit 127\n")
            .with_context(|| format!("writing {}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&path)
                .with_context(|| format!("reading {}", path.display()))?
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&path, permissions)
                .with_context(|| format!("marking {} executable", path.display()))?;
        }
    }
    Ok(bin.to_path_buf())
}

/// The target symlinks are files to git, and `target/` in `.gitignore` is a directory pattern.
fn exclude_targets(root: &Path) -> Result<()> {
    let path = root.join(".git/info/exclude");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let mut text = fs::read_to_string(&path).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str("/target\n/integrations/rig/target\n/integrations/rig-eval/target\n");
    fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// The corpus must be gone from both the working tree and every object reachable from a ref.
fn verify_clean(root: &Path, corpus: &[PathBuf]) -> Result<()> {
    for path in corpus {
        if root.join(path).exists() {
            bail!("snapshot working tree still contains {}", path.display());
        }
    }
    let objects = output(root, &["rev-list", "--objects", "--all"])?;
    for line in objects.lines() {
        let file = line
            .split_once(' ')
            .map(|(_, file)| file)
            .unwrap_or_default();
        for path in corpus {
            let needle = path.to_string_lossy();
            if file == needle || file.starts_with(&format!("{needle}/")) {
                bail!("snapshot history still contains {file}");
            }
        }
    }
    Ok(())
}

fn seed(root: &Path) -> Result<()> {
    let path = root.join(DIRTY_FILE);
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .with_context(|| format!("seeding {}", path.display()))?;
    file.write_all(MARKER.as_bytes())
        .with_context(|| format!("seeding {}", path.display()))?;
    Ok(())
}

/// A `direnv` for a snapshot: `exec` uses the real devshell, then runs in the snapshot.
///
/// The snapshot's `.envrc` is a copy, so direnv would block it until it was allowed for that
/// path — and a copy of `.direnv` is not the same cache. The shim delegates to the real checkout
/// once, then changes to the snapshot directory the agent asked for; no per-snapshot allow state.
fn write_wrapper(bin: &Path, args: &Args, stubs: Option<&Path>) -> Result<()> {
    let direnv = find_on_path("direnv").context("direnv is not on PATH")?;
    let real_direnv = shell_quote(&direnv);
    let real_root = shell_quote(&args.root);
    // Inside the devshell, prepend the stubs again: `direnv exec` would otherwise put the real
    // `at-*` binaries back on PATH and undo the absent treatment.
    let prefix = match stubs {
        Some(stubs) => format!("PATH={}:\\$PATH; ", shell_quote(stubs)),
        None => String::new(),
    };
    let script = format!(
        "#!/bin/sh\n\
         # Snapshot shim: the real devshell, run in the snapshot directory.\n\
         if [ \"$1\" = \"exec\" ] && [ $# -ge 2 ]; then\n\
           dir=\"$2\"\n\
           shift 2\n\
           case \"$dir\" in\n\
             /*) ;;\n\
             *) dir=\"$PWD/$dir\" ;;\n\
           esac\n\
           exec {real_direnv} exec {real_root} sh -c \"{prefix}cd \\\"\\$1\\\" && shift && exec \\\"\\$@\\\"\" sh \"$dir\" \"$@\"\n\
         fi\n\
         exec {real_direnv} \"$@\"\n"
    );
    let path = bin.join("direnv");
    fs::write(&path, script).with_context(|| format!("writing {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&path)
            .with_context(|| format!("reading {}", path.display()))?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions)
            .with_context(|| format!("marking {} executable", path.display()))?;
    }
    Ok(())
}

fn find_on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(program))
        .find(|path| path.is_file())
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

/// `integrations/rig` resolves its dev-dependencies at `../../../rig`. The snapshot sits one
/// directory deeper than the real checkout, so the sibling fork has to be linked beside it. In a
/// container the mount is `/repo`, so `../../../rig` is `/rig`, which the image already holds.
fn link_fork(base: &Path, args: &Args) -> Result<()> {
    if args.container.is_some() {
        return Ok(());
    }
    let Some(parent) = args.root.parent() else {
        return Ok(());
    };
    let fork = parent.join("rig");
    if !fork.is_dir() {
        return Ok(());
    }
    let link = base.join("rig");
    if !link.exists() {
        symlink(&fork, &link)?;
    }
    Ok(())
}

/// Point the snapshot's build directories at the warm ones. On the host those are the real
/// checkout's; in a container they are the image's, at the path the image built them at, so the
/// build fingerprints match and cargo does no work.
fn link_targets(args: &Args, snapshot: &Path) -> Result<()> {
    let container = args.container.is_some();
    for relative in [
        "target",
        "integrations/rig/target",
        "integrations/rig-eval/target",
    ] {
        let source = if container {
            Path::new(crate::container::IMAGE_ROOT).join(relative)
        } else {
            args.root.join(relative)
        };
        if !container && !source.is_dir() {
            continue;
        }
        let link = snapshot.join(relative);
        if fs::symlink_metadata(&link).is_ok() {
            continue;
        }
        if let Some(parent) = link.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        symlink(&source, &link)?;
    }
    Ok(())
}

#[cfg(unix)]
fn symlink(source: &Path, link: &Path) -> Result<()> {
    std::os::unix::fs::symlink(source, link)
        .with_context(|| format!("linking {} to {}", link.display(), source.display()))
}

#[cfg(not(unix))]
fn symlink(source: &Path, link: &Path) -> Result<()> {
    bail!(
        "snapshot build directories need symlinks: {} -> {}",
        link.display(),
        source.display()
    )
}

fn copy_dir(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)
        .with_context(|| format!("creating {}", destination.display()))?;
    for entry in fs::read_dir(source).with_context(|| format!("reading {}", source.display()))? {
        let entry = entry.with_context(|| format!("reading {}", source.display()))?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .with_context(|| format!("reading {}", from.display()))?;
        if file_type.is_symlink() {
            let target =
                fs::read_link(&from).with_context(|| format!("reading link {}", from.display()))?;
            symlink(&target, &to)?;
        } else if file_type.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            fs::copy(&from, &to)
                .with_context(|| format!("copying {} to {}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

fn git(directory: &Path, arguments: &[&str]) -> Result<()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .output()
        .with_context(|| format!("running git {}", arguments.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} in {}: {}",
            arguments.join(" "),
            directory.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn output(directory: &Path, arguments: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .output()
        .with_context(|| format!("running git {}", arguments.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} in {}: {}",
            arguments.join(" "),
            directory.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_quoted_for_the_filter_branch_shell() {
        assert_eq!(shell_quote(Path::new("/a b/c")), "'/a b/c'");
        assert_eq!(shell_quote(Path::new("/a'b")), "'/a'\\''b'");
    }

    #[test]
    fn the_repo_corpus_is_always_removed() {
        let root = Path::new("/repo");
        let args = Args {
            root: root.to_path_buf(),
            scenario_dir: root.join(REPO_CORPUS),
            only: Vec::new(),
            targets: Vec::new(),
            model: "model".to_owned(),
            rig: root.join("rig"),
            opencode_config: root.join("opencode.json"),
            pi: root.join("pi"),
            env_file: root.join("env"),
            out: root.join("out"),
            reuse: None,
            publish: None,
            repeat: 1,
            jobs: 1,
            timeout: None,
            container: None,
            dry_run: false,
        };
        assert_eq!(corpus_paths(&args), [PathBuf::from(REPO_CORPUS)]);
    }
}
