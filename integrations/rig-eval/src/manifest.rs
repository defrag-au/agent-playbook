//! What a published run is: the corpus it read, the checkout it measured, and the arms it drove.
//!
//! `results.json` answers "what happened"; this answers "what was it a measurement of". Without it a
//! result cannot be tied to a corpus revision, and a second run that changes a scenario or an arm is
//! indistinguishable from a change in the harness.
//!
//! Everything here is content-addressed with `git hash-object`, so a reader can verify a corpus or
//! an arm's config without the runner and without trusting this code. Environment variables are
//! recorded by name only: the run needs a key, and a manifest is meant to be published.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::cli::Args;
use crate::report::RunResult;
use crate::target::Target;

/// The manifest and export schema the site's loaders read. Bump it when a shape changes; a page
/// that silently charts the old shape is worse than a failed build.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    /// The shape of this manifest and the export files beside it. The site's loaders refuse a
    /// mismatch rather than charting a re-shaped result.
    #[serde(default)]
    pub schema_version: u32,
    /// The runner that wrote it, so a result can be matched to the binary that produced it.
    pub runner: Version,
    /// Unix milliseconds. A run that spans midnight is still one run.
    pub started_ms: u64,
    pub finished_ms: u64,
    pub model: String,
    /// Names only. A published manifest must never carry a credential.
    pub env_names: Vec<String>,
    pub corpus: Corpus,
    pub checkout: Checkout,
    pub snapshot: Snapshot,
    /// The arms, in the order the run drove them.
    pub targets: Vec<Arm>,
    pub jobs: usize,
    pub repeat: u32,
    /// Every run's outcome, summed. The rows are in `results.json`.
    pub totals: Totals,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Version {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Corpus {
    /// Relative to the checkout when it is inside it, so a published path is meaningful.
    pub directory: String,
    /// The fingerprint of the file listing below: same files and contents, same fingerprint.
    pub fingerprint: String,
    pub files: Vec<CorpusFile>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CorpusFile {
    pub name: String,
    pub bytes: u64,
    /// `git hash-object` of the content, so the file can be checked with the same git.
    pub blob: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Checkout {
    pub head: Option<String>,
    pub branch: Option<String>,
    /// Anything uncommitted. A published run should normally be false.
    pub dirty: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Snapshot {
    /// The commit a run executes at: history with the corpus rewritten out of it.
    pub commit: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Arm {
    pub label: String,
    pub kind: String,
    /// The home or config, relative to the checkout where possible.
    pub location: String,
    /// The blob of the config file the arm is defined by, when it has one.
    pub config: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Totals {
    pub runs: usize,
    pub cost_usd: f64,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub model_calls: usize,
    pub tool_calls: usize,
    pub passed: usize,
    pub checks: usize,
}

pub fn now_ms() -> Result<u64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("the system clock is before the Unix epoch")?;
    Ok(elapsed.as_millis() as u64)
}

pub fn build(
    args: &Args,
    env_names: &[String],
    snapshot_commit: &str,
    started_ms: u64,
    results: &[RunResult],
) -> Result<Manifest> {
    let corpus = corpus(args)?;
    Ok(Manifest {
        schema_version: SCHEMA_VERSION,
        runner: Version {
            name: "rig-eval".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        },
        started_ms,
        finished_ms: now_ms()?,
        model: args.model.clone(),
        env_names: env_names.to_vec(),
        corpus,
        checkout: checkout(&args.root),
        snapshot: Snapshot {
            commit: snapshot_commit.to_owned(),
        },
        targets: arms(args)?,
        jobs: args.jobs,
        repeat: args.repeat,
        totals: totals(results),
    })
}

pub fn write(args: &Args, manifest: &Manifest) -> Result<()> {
    let path = args.out.join("manifest.json");
    let text = serde_json::to_string_pretty(manifest).context("encoding the manifest")?;
    fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
}

/// Read a run's identity back, for `--reuse`. A missing manifest is not an error: it is a reason
/// for the summary to say that the identity was never recorded.
pub fn read(directory: &Path) -> Result<Manifest> {
    let path = directory.join("manifest.json");
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

/// Every scenario file, hashed by content, and the fingerprint of that listing.
fn corpus(args: &Args) -> Result<Corpus> {
    let mut paths: Vec<PathBuf> = fs::read_dir(&args.scenario_dir)
        .with_context(|| format!("reading {}", args.scenario_dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    paths.sort();

    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let bytes = fs::metadata(&path)
            .with_context(|| format!("reading {}", path.display()))?
            .len();
        files.push(CorpusFile {
            name,
            bytes,
            blob: blob(&path)?,
        });
    }

    let listing: String = files
        .iter()
        .map(|file| format!("{}\0{}\n", file.name, file.blob))
        .collect();
    Ok(Corpus {
        directory: relative(&args.root, &args.scenario_dir),
        fingerprint: hash_stdin(&listing)?,
        files,
    })
}

fn checkout(root: &Path) -> Checkout {
    Checkout {
        head: git_line(root, &["rev-parse", "HEAD"]),
        branch: git_line(root, &["rev-parse", "--abbrev-ref", "HEAD"]),
        // `--no-optional-locks` because a read must not refresh the index: this is a measurement
        // tool, and it does not get to write to the checkout it measures.
        dirty: Command::new("git")
            .args(["--no-optional-locks", "-C"])
            .arg(root)
            .args(["status", "--porcelain"])
            .output()
            .map(|output| output.status.success() && !output.stdout.is_empty())
            .unwrap_or(false),
    }
}

fn arms(args: &Args) -> Result<Vec<Arm>> {
    args.targets
        .iter()
        .map(|target| match target {
            Target::Rig(home) => {
                let settings = home.path.join("settings.toml");
                Ok(Arm {
                    label: target.label(),
                    kind: "rig".to_owned(),
                    location: relative(&args.root, &home.path),
                    config: settings.is_file().then(|| blob(&settings)).transpose()?,
                })
            }
            Target::Opencode => Ok(Arm {
                label: target.label(),
                kind: "opencode".to_owned(),
                location: relative(&args.root, &args.opencode_config),
                config: args
                    .opencode_config
                    .is_file()
                    .then(|| blob(&args.opencode_config))
                    .transpose()?,
            }),
            Target::Pi => Ok(Arm {
                label: target.label(),
                kind: "pi".to_owned(),
                location: relative(&args.root, &args.pi),
                config: None,
            }),
        })
        .collect()
}

fn totals(results: &[RunResult]) -> Totals {
    let mut totals = Totals::default();
    for result in results {
        totals.runs += 1;
        totals.cost_usd += result.cost_usd;
        totals.input_tokens += result.input_tokens;
        totals.cached_input_tokens += result.cached_input_tokens;
        totals.output_tokens += result.output_tokens;
        totals.model_calls += result.model_calls;
        totals.tool_calls += result.tool_call_total;
        totals.passed += result.passed;
        totals.checks += result.checks_total;
    }
    totals
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn blob(path: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["hash-object", "--no-filters", "--"])
        .arg(path)
        .output()
        .with_context(|| format!("hashing {}", path.display()))?;
    if !output.status.success() {
        bail!(
            "git hash-object {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The hash of a listing, computed the same way a blob is, so a fingerprint is reproducible with
/// `git hash-object --stdin` and needs no crate of our own.
fn hash_stdin(text: &str) -> Result<String> {
    let mut child = Command::new("git")
        .args(["hash-object", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("running git hash-object")?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .context("writing the corpus listing")?;
    }
    let output = child
        .wait_with_output()
        .context("waiting for git hash-object")?;
    if !output.status.success() {
        bail!(
            "git hash-object --stdin: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn git_line(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(["--no-optional-locks", "-C"])
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn fixture(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rig-eval-manifest-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("scenarios")).expect("scenario directory");
        fs::create_dir_all(dir.join("home")).expect("home directory");
        fs::write(dir.join("scenarios/a.json"), r#"{"name":"a","prompt":"p"}"#)
            .expect("a scenario");
        fs::write(dir.join("scenarios/b.json"), r#"{"name":"b","prompt":"p"}"#)
            .expect("another scenario");
        fs::write(dir.join("home/settings.toml"), "[plugins]\n").expect("settings");
        dir
    }

    fn args(root: &Path) -> Args {
        Args {
            root: root.to_path_buf(),
            scenario_dir: root.join("scenarios"),
            only: Vec::new(),
            targets: vec![Target::Rig(crate::cli::Home {
                label: "test".to_owned(),
                path: root.join("home"),
            })],
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
            dry_run: false,
        }
    }

    fn build_at(root: &Path) -> Manifest {
        build(&args(root), &[], "snapshot-commit", 1, &[]).expect("a manifest")
    }

    #[test]
    fn a_manifest_names_its_own_schema() {
        let root = fixture("schema");
        assert_eq!(build_at(&root).schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn the_corpus_fingerprint_is_stable_and_changes_with_content() {
        let root = fixture("fingerprint");
        let first = build_at(&root).corpus.fingerprint;
        assert_eq!(first, build_at(&root).corpus.fingerprint);
        fs::write(
            root.join("scenarios/a.json"),
            r#"{"name":"a","prompt":"changed"}"#,
        )
        .expect("edit");
        assert_ne!(first, build_at(&root).corpus.fingerprint);
    }

    #[test]
    fn every_scenario_file_is_named_and_hashed() {
        let root = fixture("files");
        let corpus = build_at(&root).corpus;
        let names: Vec<&str> = corpus.files.iter().map(|file| file.name.as_str()).collect();
        assert_eq!(names, ["a.json", "b.json"]);
        assert!(corpus.files.iter().all(|file| file.blob.len() == 40));
    }

    #[test]
    fn an_arm_records_the_blob_of_its_settings() {
        let root = fixture("arm");
        let manifest = build_at(&root);
        let settings = fs::read_to_string(root.join("home/settings.toml")).expect("settings");
        let expected = Command::new("git")
            .args(["hash-object", "--stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                child
                    .stdin
                    .take()
                    .expect("stdin")
                    .write_all(settings.as_bytes())?;
                child.wait_with_output()
            })
            .expect("hash");
        assert_eq!(
            manifest.targets[0].config.as_deref(),
            Some(String::from_utf8_lossy(&expected.stdout).trim())
        );
        assert_eq!(manifest.targets[0].label, "rig:test");
    }

    #[test]
    fn a_run_with_no_results_totals_to_zero() {
        let root = fixture("totals");
        let totals = build_at(&root).totals;
        assert_eq!(totals.runs, 0);
        assert_eq!(totals.cost_usd, 0.0);
    }
}
