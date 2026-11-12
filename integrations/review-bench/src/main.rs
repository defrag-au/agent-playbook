//! Score vision models on the macroquad-storybook review corpus.
//!
//! The loop belongs to this harness: one completion per case, a typed answer,
//! deterministic scoring. The corpus is data; see
//! `agent-playbook/docs/design/visual-review.md`.

mod corpus;
mod provider;
mod registry;
mod score;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{ensure, Context, Result};
use clap::Parser;
use rig_core::completion::Usage;
use serde::Serialize;
use sha2::{Digest, Sha256};

use corpus::{Answer, Corpus, ExpectedKind, LoadedCase};
use registry::{ModelSpec, Registry};
use score::Score;

#[derive(Parser)]
#[command(about = "Score vision models on the macroquad-storybook review corpus")]
struct Args {
    /// The review corpus root: the directory holding RUBRIC.md and cases/.
    #[arg(long)]
    corpus: PathBuf,
    /// The model registry entry to run.
    #[arg(long)]
    model: String,
    /// How many times to run every case.
    #[arg(long, default_value_t = 1)]
    runs: u32,
    /// Where the answers and summaries are written.
    #[arg(long, default_value = ".tmp/review-runs")]
    out: PathBuf,
    /// The registry file; defaults to models.toml beside this crate.
    #[arg(long)]
    registry: Option<PathBuf>,
    /// Only cases whose slug contains this string.
    #[arg(long)]
    only: Option<String>,
    /// Permit a registry entry marked `heavy`.
    #[arg(long)]
    allow_heavy: bool,
    /// Print the plan and send nothing.
    #[arg(long)]
    dry_run: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(args.runs > 0, "--runs must be at least 1");
    let registry_path = args
        .registry
        .clone()
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models.toml"));
    let registry = Registry::load(&registry_path)?;
    let spec = registry.get(&args.model)?;
    ensure!(
        !spec.heavy || args.allow_heavy,
        "{} is marked heavy; pass --allow-heavy to run it",
        args.model
    );
    if let (Some(resident), Some(budget)) = (spec.resident_gb, spec.max_resident_gb) {
        ensure!(
            resident <= budget,
            "{} needs {resident:.0} GB but the budget is {budget:.0} GB",
            args.model
        );
    }
    let corpus = Corpus::load(&args.corpus)?;
    let cases = corpus.selected(args.only.as_deref())?;
    let provenance = corpus.provenance(&cases)?;
    if args.dry_run {
        print_plan(&args, spec, &corpus, &cases, &provenance);
        return Ok(());
    }
    run(&args, spec, &corpus, &cases, &provenance).await
}

fn print_plan(
    args: &Args,
    spec: &ModelSpec,
    corpus: &Corpus,
    cases: &[&LoadedCase],
    provenance: &(String, String),
) {
    let crops = cases.iter().filter(|case| case.case.crop.is_some()).count();
    println!(
        "model {} · {}/{}",
        args.model,
        spec.provider.as_str(),
        spec.model
    );
    println!(
        "corpus {} · {} cases · {}/{}",
        corpus.root.display(),
        cases.len(),
        provenance.0,
        provenance.1
    );
    println!(
        "runs {} · calls {}",
        args.runs,
        cases.len() * args.runs as usize
    );
    println!("images {} full · {crops} crops", cases.len());
    match (spec.resident_gb, spec.max_resident_gb) {
        (Some(resident), Some(budget)) => println!(
            "resident {resident:.0} GB of a {budget:.0} GB budget{}",
            if spec.heavy { " · heavy" } else { "" }
        ),
        _ => println!("resident budget: not configured"),
    }
    match spec.max_cost_usd {
        Some(cap) => {
            println!("cost cap ${cap:.2} per run (no per-token price table; the cap is a ceiling)")
        }
        None => println!("cost cap: none configured"),
    }
    if let Some(max) = spec.max_output_tokens {
        println!("output limit {max} tokens per call");
    }
    println!("cases:");
    for case in cases {
        println!(
            "  {:<24} {}{}",
            case.slug,
            case.expected.expected.as_str(),
            if case.case.crop.is_some() {
                " · full + crop"
            } else {
                " · full"
            }
        );
    }
    println!("dry run: nothing sent; no API key read");
}

async fn run(
    args: &Args,
    spec: &ModelSpec,
    corpus: &Corpus,
    cases: &[&LoadedCase],
    provenance: &(String, String),
) -> Result<()> {
    let out = args.out.join(args.model.as_str());
    fs::create_dir_all(&out)?;
    let mut rows = Vec::new();
    for run_number in 1..=args.runs {
        for case in cases {
            let started = Instant::now();
            let completion = provider::complete(spec, case, &corpus.rubric).await;
            let elapsed_ms = started.elapsed().as_millis();
            let (answer, usage, error, raw_response) = match completion {
                Ok(completion) => match corpus::parse_answer(&completion.text) {
                    Ok(answer) => (Some(answer), Some(completion.usage), None, None),
                    Err(err) => (
                        None,
                        Some(completion.usage),
                        Some(format!("parse: {err:#}")),
                        Some(completion.text),
                    ),
                },
                Err(err) => (None, None, Some(format!("request: {err:#}")), None),
            };
            let score_value = answer
                .as_ref()
                .map(|answer| score::score(&case.expected, Some(answer), case.image_size));
            let row = RunResult {
                run: run_number,
                slug: case.slug.clone(),
                story: case.case.story.clone(),
                arch: case.case.arch.clone(),
                os: case.case.os.clone(),
                provider: spec.provider.as_str().to_owned(),
                model: spec.model.clone(),
                expected: case.expected.expected,
                elapsed_ms,
                usage,
                error,
                raw_response,
                answer,
                score: score_value,
                image_sha256: sha_file(&case.dir.join(case.case.image.as_str()))?,
                crop_sha256: case
                    .case
                    .crop
                    .as_ref()
                    .map(|crop| sha_file(&case.dir.join(crop.as_str())))
                    .transpose()?,
                crop_box: case.case.crop_box,
                prompt_sha256: sha_text(&corpus::prompt(&corpus.rubric, &case.case)),
            };
            let dir = out.join(format!("run-{run_number}"));
            fs::create_dir_all(&dir)?;
            let path = dir.join(format!("{}.json", case.slug.replace('/', "-")));
            fs::write(&path, serde_json::to_string_pretty(&row)?)?;
            let state = if row.error.is_some() { "ERR" } else { "ok" };
            println!(
                "[{run_number}/{}] {:<22} {state} {elapsed_ms:>5} ms",
                args.runs, case.slug
            );
            rows.push(row);
        }
    }
    let summary = Summary::new(&args.model, spec, cases, &rows, provenance);
    fs::write(
        out.join("summary.json"),
        serde_json::to_string_pretty(&summary)?,
    )?;
    let text = summary.to_text();
    fs::write(out.join("summary.txt"), &text)?;
    println!("\n{text}");
    Ok(())
}

#[derive(Serialize)]
struct RunResult {
    run: u32,
    slug: String,
    story: String,
    arch: String,
    os: String,
    provider: String,
    model: String,
    expected: ExpectedKind,
    elapsed_ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<Usage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    raw_response: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    answer: Option<Answer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    score: Option<Score>,
    image_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    crop_sha256: Option<String>,
    /// Where the crop was taken from, in full-image pixels, when the case has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    crop_box: Option<[f64; 4]>,
    prompt_sha256: String,
}

#[derive(Serialize)]
struct RowSummary {
    run: u32,
    slug: String,
    expected: ExpectedKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    detected: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    iou: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    false_positives: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    elapsed_ms: u128,
}

impl From<&RunResult> for RowSummary {
    fn from(row: &RunResult) -> Self {
        let score = row.score.as_ref();
        Self {
            run: row.run,
            slug: row.slug.clone(),
            expected: row.expected,
            detected: score.map(|score| score.detected),
            iou: score.and_then(|score| score.iou),
            false_positives: score.map(|score| score.false_positives),
            error: row.error.clone(),
            elapsed_ms: row.elapsed_ms,
        }
    }
}

#[derive(Serialize)]
struct Summary {
    model: String,
    provider: String,
    wire_model: String,
    arch: String,
    os: String,
    runs: u32,
    cases: usize,
    requests: usize,
    defect_rows: usize,
    detected: usize,
    detection: f64,
    weighted_detection: f64,
    clean_rows: usize,
    clean_false_positive_rows: usize,
    clean_false_positives: usize,
    mean_iou: Option<f64>,
    parse_or_request_errors: usize,
    mean_latency_ms: f64,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    unstable_cases: Vec<String>,
    rows: Vec<RowSummary>,
}

impl Summary {
    fn new(
        name: &str,
        spec: &ModelSpec,
        cases: &[&LoadedCase],
        rows: &[RunResult],
        provenance: &(String, String),
    ) -> Self {
        let weights: BTreeMap<&str, f64> = cases
            .iter()
            .map(|case| {
                let weight = case
                    .expected
                    .defect
                    .iter()
                    .map(|defect| defect.severity.weight())
                    .sum::<f64>();
                (case.slug.as_str(), weight)
            })
            .collect();
        let defect_rows: Vec<&RunResult> = rows
            .iter()
            .filter(|row| row.expected == ExpectedKind::Defect)
            .collect();
        let clean_rows: Vec<&RunResult> = rows
            .iter()
            .filter(|row| row.expected == ExpectedKind::Clean)
            .collect();
        let detected = defect_rows
            .iter()
            .filter(|row| row.score.as_ref().is_some_and(|score| score.detected))
            .count();
        let weighted_total: f64 = defect_rows
            .iter()
            .filter_map(|row| weights.get(row.slug.as_str()).copied())
            .sum();
        let weighted_hit: f64 = defect_rows
            .iter()
            .filter(|row| row.score.as_ref().is_some_and(|score| score.detected))
            .filter_map(|row| weights.get(row.slug.as_str()).copied())
            .sum();
        let clean_false_positive_rows = clean_rows
            .iter()
            .filter(|row| {
                row.score
                    .as_ref()
                    .is_some_and(|score| score.false_positives > 0)
            })
            .count();
        let clean_false_positives = clean_rows
            .iter()
            .filter_map(|row| row.score.as_ref())
            .map(|score| score.false_positives)
            .sum();
        let ious: Vec<f64> = defect_rows
            .iter()
            .filter_map(|row| row.score.as_ref().and_then(|score| score.iou))
            .collect();
        let mean_iou = (!ious.is_empty()).then(|| ious.iter().sum::<f64>() / ious.len() as f64);
        let errors = rows.iter().filter(|row| row.error.is_some()).count();
        let mean_latency_ms = if rows.is_empty() {
            0.0
        } else {
            rows.iter().map(|row| row.elapsed_ms as f64).sum::<f64>() / rows.len() as f64
        };
        Self {
            model: name.to_owned(),
            provider: spec.provider.as_str().to_owned(),
            wire_model: spec.model.clone(),
            arch: provenance.0.clone(),
            os: provenance.1.clone(),
            runs: spec_runs(rows),
            cases: cases.len(),
            requests: rows.len(),
            defect_rows: defect_rows.len(),
            detected,
            detection: if defect_rows.is_empty() {
                0.0
            } else {
                detected as f64 / defect_rows.len() as f64
            },
            weighted_detection: if weighted_total > 0.0 {
                weighted_hit / weighted_total
            } else {
                0.0
            },
            clean_rows: clean_rows.len(),
            clean_false_positive_rows,
            clean_false_positives,
            mean_iou,
            parse_or_request_errors: errors,
            mean_latency_ms,
            input_tokens: sum_usage(rows, |usage| usage.input_tokens),
            output_tokens: sum_usage(rows, |usage| usage.output_tokens),
            unstable_cases: unstable_cases(rows),
            rows: rows.iter().map(RowSummary::from).collect(),
        }
    }

    fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "model {} · {}/{}\n",
            self.model, self.provider, self.wire_model
        ));
        out.push_str(&format!(
            "corpus {}/{} · runs {} · cases {} · requests {}\n",
            self.arch, self.os, self.runs, self.cases, self.requests
        ));
        out.push_str(&format!(
            "defect detection {}/{} ({:.0}%)\n",
            self.detected,
            self.defect_rows,
            self.detection * 100.0
        ));
        out.push_str(&format!(
            "weighted detection {:.0}%\n",
            self.weighted_detection * 100.0
        ));
        out.push_str(&format!(
            "clean false-positive rows {}/{} ({} reports)\n",
            self.clean_false_positive_rows, self.clean_rows, self.clean_false_positives
        ));
        out.push_str(&format!("mean IoU {}\n", option(self.mean_iou)));
        out.push_str(&format!("errors {}\n", self.parse_or_request_errors));
        out.push_str(&format!("mean latency {:.0} ms\n", self.mean_latency_ms));
        out.push_str(&format!(
            "tokens in/out {}/{}\n",
            option(self.input_tokens),
            option(self.output_tokens)
        ));
        if !self.unstable_cases.is_empty() {
            out.push_str(&format!(
                "unstable answers: {}\n",
                self.unstable_cases.join(", ")
            ));
        }
        out.push_str(
            "\nslug                   run  expected  detected  iou    fp   ms     error\n",
        );
        for row in &self.rows {
            out.push_str(&format!(
                "{:<22} {:>3}  {:<8}  {:<8}  {:<5}  {:<3}  {:<5}  {}\n",
                row.slug,
                row.run,
                row.expected.as_str(),
                option(row.detected),
                row.iou
                    .map(|iou| format!("{iou:.2}"))
                    .unwrap_or_else(|| "-".to_owned()),
                option(row.false_positives),
                row.elapsed_ms,
                row.error.as_deref().unwrap_or(""),
            ));
        }
        out
    }
}

fn spec_runs(rows: &[RunResult]) -> u32 {
    rows.iter().map(|row| row.run).max().unwrap_or(0)
}

fn option<T: std::fmt::Display>(value: Option<T>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_owned())
}

fn sum_usage(rows: &[RunResult], field: impl Fn(&Usage) -> Option<u64>) -> Option<u64> {
    let mut total = 0u64;
    let mut seen = false;
    for row in rows {
        if let Some(usage) = &row.usage {
            if let Some(value) = field(usage) {
                total += value;
                seen = true;
            }
        }
    }
    seen.then_some(total)
}

fn unstable_cases(rows: &[RunResult]) -> Vec<String> {
    let mut by_case: BTreeMap<&str, Vec<(bool, bool, String)>> = BTreeMap::new();
    for row in rows {
        let detected = row.score.as_ref().is_some_and(|score| score.detected);
        let error = row.error.is_some();
        by_case.entry(row.slug.as_str()).or_default().push((
            detected,
            error,
            answer_signature(row),
        ));
    }
    by_case
        .into_iter()
        .filter(|(_, answers)| answers.windows(2).any(|pair| pair[0] != pair[1]))
        .map(|(slug, _)| slug.to_owned())
        .collect()
}

/// What the model said, in comparable form: each reported kind and box.
/// The note's wording is left out — a reworded note is the same finding, but a
/// moved box is not.
fn answer_signature(row: &RunResult) -> String {
    let Some(answer) = &row.answer else {
        return String::new();
    };
    let mut signature = String::new();
    for defect in &answer.defects {
        signature.push_str(&defect.kind.to_ascii_lowercase());
        signature.push('@');
        match defect.bounds {
            Some([x0, y0, x1, y1]) => {
                signature.push_str(&format!("{x0:.1},{y0:.1},{x1:.1},{y1:.1}"));
            }
            None => signature.push_str("none"),
        }
        signature.push(';');
    }
    signature
}

fn sha_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(sha_bytes(&bytes))
}

fn sha_text(text: &str) -> String {
    sha_bytes(text.as_bytes())
}

fn sha_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::Defect;
    use crate::score::Score;

    fn row(slug: &str, detected: bool, kind: &str, bounds: Option<[f64; 4]>) -> RunResult {
        RunResult {
            run: 1,
            slug: slug.to_owned(),
            story: String::new(),
            arch: "aarch64".to_owned(),
            os: "macos".to_owned(),
            provider: "ollama".to_owned(),
            model: "test".to_owned(),
            expected: ExpectedKind::Defect,
            elapsed_ms: 0,
            usage: None,
            error: None,
            raw_response: None,
            answer: Some(Answer {
                defects: vec![Defect {
                    kind: kind.to_owned(),
                    bounds,
                    confidence: 1.0,
                    note: String::new(),
                }],
            }),
            score: Some(Score {
                detected,
                iou: None,
                false_positives: 0,
                matched: 0,
            }),
            image_sha256: String::new(),
            crop_sha256: None,
            crop_box: None,
            prompt_sha256: String::new(),
        }
    }

    #[test]
    fn a_moved_box_is_an_unstable_answer() {
        let rows = [
            row("case", true, "clipped", Some([10.0, 10.0, 20.0, 20.0])),
            row("case", true, "clipped", Some([30.0, 30.0, 40.0, 40.0])),
        ];
        assert_eq!(unstable_cases(&rows), vec!["case".to_owned()]);
    }

    #[test]
    fn an_unchanged_answer_is_stable() {
        let rows = [
            row("case", true, "clipped", Some([10.0, 10.0, 20.0, 20.0])),
            row("case", true, "clipped", Some([10.0, 10.0, 20.0, 20.0])),
        ];
        assert!(unstable_cases(&rows).is_empty());
    }

    #[test]
    fn a_reworded_note_is_not_an_unstable_answer() {
        let mut rows = [
            row("case", true, "clipped", Some([10.0, 10.0, 20.0, 20.0])),
            row("case", true, "clipped", Some([10.0, 10.0, 20.0, 20.0])),
        ];
        if let Some(answer) = rows[1].answer.as_mut() {
            answer.defects[0].note = "the right edge cuts the label".to_owned();
        }
        assert!(unstable_cases(&rows).is_empty());
    }
}
