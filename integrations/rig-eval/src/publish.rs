//! The public site projection of a measured run.
//!
//! A run directory is evidence: it keeps the raw streams, the model's answers and the harness
//! session. The site publishes a subset. The projection drops the answer and the session id,
//! keeps every scored row, and refuses a run whose identity was not recorded, whose checkout was
//! dirty, or whose result was contaminated.

use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::export;
use crate::manifest::Manifest;
use crate::report::RunResult;
use crate::tag::RunTag;

/// Write `<directory>/<run-id>/` and return the id.
///
/// `summary` is the text `report::write` produced for the run, so the site's tables are the same
/// bytes as the run directory's. `claim.md` is authored; it is copied from `run` when present and
/// never generated here.
pub fn write(
    directory: &Path,
    run: &Path,
    results: &[RunResult],
    manifest: &Manifest,
    summary: &str,
) -> Result<String> {
    if manifest.schema_version != crate::manifest::SCHEMA_VERSION {
        bail!(
            "manifest schema_version {}; this publisher writes {} — regenerate the run rather \
             than publishing a shape the site cannot read",
            manifest.schema_version,
            crate::manifest::SCHEMA_VERSION
        );
    }
    if manifest.checkout.dirty {
        bail!(
            "refusing to publish a run from a dirty checkout; publish from a clean commit, or \
             keep the run as a development record"
        );
    }
    if let Some(result) = results
        .iter()
        .find(|result| result.tags.contains(&RunTag::Contaminated))
    {
        bail!(
            "refusing to publish a contaminated run: {} · {} · run {}",
            result.scenario,
            result.target,
            result.repeat
        );
    }

    let id = run_id(manifest);
    let target = directory.join(&id);
    fs::create_dir_all(&target).with_context(|| format!("creating {}", target.display()))?;

    write_json(&target, "manifest.json", manifest)?;
    fs::write(target.join("summary.md"), summary)
        .with_context(|| format!("writing {}", target.join("summary.md").display()))?;

    let mut traces = export::traces(results);
    for row in &mut traces {
        row.answer = None;
        row.status.session = None;
    }
    write_json(&target, "traces.json", &traces)?;
    write_json(&target, "tools.json", &export::tools(results))?;
    write_json(&target, "checks.json", &export::checks(results))?;

    let notebook = target.join("observable.md");
    fs::write(&notebook, export::notebook())
        .with_context(|| format!("writing {}", notebook.display()))?;

    let claim = run.join("claim.md");
    if claim.is_file() {
        let destination = target.join("claim.md");
        fs::copy(&claim, &destination).with_context(|| format!("copying {}", claim.display()))?;
    }

    Ok(id)
}

/// A stable, sortable name for the run: time, then the inputs it measured.
pub fn run_id(manifest: &Manifest) -> String {
    let corpus = manifest
        .corpus
        .fingerprint
        .get(..7)
        .unwrap_or(&manifest.corpus.fingerprint);
    format!(
        "run-{}-{corpus}-{}",
        manifest.started_ms,
        slug(&manifest.model)
    )
}

fn slug(value: &str) -> String {
    let mut out = String::new();
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            out.push(character.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "model".to_owned()
    } else {
        out
    }
}

fn write_json<T: Serialize>(directory: &Path, name: &str, value: &T) -> Result<()> {
    let path = directory.join(name);
    let text = serde_json::to_string_pretty(value).with_context(|| format!("encoding {name}"))?;
    fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::manifest::{Arm, Checkout, Corpus, Snapshot, Version};
    use crate::score::{Check, Source};

    fn manifest() -> Manifest {
        Manifest {
            schema_version: crate::manifest::SCHEMA_VERSION,
            runner: Version {
                name: "rig-eval".to_owned(),
                version: "0.1.0".to_owned(),
            },
            started_ms: 1_763_000_000_000,
            finished_ms: 1_763_000_001_000,
            model: "deepseek/deepseek-flash".to_owned(),
            env_names: vec!["DEEPSEEK_API_KEY".to_owned()],
            corpus: Corpus {
                directory: "integrations/rig-eval/scenarios".to_owned(),
                fingerprint: "11a2a7894e671df91621c06b3d87e8b4794a9651".to_owned(),
                files: Vec::new(),
            },
            checkout: Checkout {
                head: Some("f7f9ac7".to_owned()),
                branch: Some("main".to_owned()),
                dirty: false,
            },
            snapshot: Snapshot {
                commit: "a1e3d3f".to_owned(),
            },
            targets: vec![Arm {
                label: "opencode".to_owned(),
                kind: "opencode".to_owned(),
                location: ".tmp/opencode/opencode.json".to_owned(),
                config: None,
            }],
            jobs: 2,
            repeat: 3,
            totals: Default::default(),
        }
    }

    fn run_result() -> RunResult {
        RunResult {
            scenario: "recover-investigation".to_owned(),
            target: "opencode".to_owned(),
            repeat: 1,
            model: "deepseek/deepseek-flash".to_owned(),
            workdir: ".".to_owned(),
            session: Some("ses_private".to_owned()),
            answer: Some("the full model answer".to_owned()),
            checks: vec![Check {
                required: "docs/design/rig.md:143".to_owned(),
                source: Source::Path,
                found: true,
            }],
            passed: 1,
            checks_total: 1,
            score: 1.0,
            tags: BTreeSet::new(),
            ..RunResult::default()
        }
    }

    fn directory(name: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("rig-eval-publish-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn the_run_id_is_sortable_and_names_its_inputs() {
        let id = run_id(&manifest());
        assert!(id.starts_with("run-1763000000000-11a2a78-deepseek-deepseek-flash"));
    }

    #[test]
    fn the_public_projection_drops_the_answer_and_the_session() {
        let directory = directory("projection");
        let id = write(
            &directory,
            &directory.join("run"),
            &[run_result()],
            &manifest(),
            "# rig-eval\n",
        )
        .expect("a projection");
        let target = directory.join(&id);

        let traces: Vec<export::TraceRow> =
            serde_json::from_str(&fs::read_to_string(target.join("traces.json")).expect("traces"))
                .expect("an array");
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].answer, None);
        assert_eq!(traces[0].status.session, None);
        assert!(target.join("manifest.json").is_file());
        assert!(target.join("summary.md").is_file());
        assert!(target.join("tools.json").is_file());
        assert!(target.join("checks.json").is_file());
        assert!(target.join("observable.md").is_file());
        assert!(!target.join("claim.md").exists());
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn an_unversioned_manifest_is_refused() {
        let directory = directory("schema");
        let mut manifest = manifest();
        manifest.schema_version = 0;
        let error = write(
            &directory,
            &directory.join("run"),
            &[run_result()],
            &manifest,
            "summary",
        )
        .expect_err("refused");
        assert!(error.to_string().contains("schema_version"));
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn an_authored_claim_is_copied() {
        let directory = directory("claim");
        let run = directory.join("run");
        fs::create_dir_all(&run).expect("run directory");
        fs::write(run.join("claim.md"), "This is the claim.").expect("claim");
        let id =
            write(&directory, &run, &[run_result()], &manifest(), "summary").expect("a projection");
        assert_eq!(
            fs::read_to_string(directory.join(&id).join("claim.md")).expect("claim"),
            "This is the claim."
        );
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_dirty_checkout_is_refused() {
        let directory = directory("dirty");
        let mut manifest = manifest();
        manifest.checkout.dirty = true;
        let error = write(
            &directory,
            &directory.join("run"),
            &[run_result()],
            &manifest,
            "summary",
        )
        .expect_err("refused");
        assert!(error.to_string().contains("dirty checkout"));
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_contaminated_run_is_refused() {
        let directory = directory("contaminated");
        let mut run = run_result();
        run.tags.insert(RunTag::Contaminated);
        let error = write(
            &directory,
            &directory.join("run"),
            &[run],
            &manifest(),
            "summary",
        )
        .expect_err("refused");
        assert!(error.to_string().contains("contaminated"));
        let _ = fs::remove_dir_all(&directory);
    }
}
