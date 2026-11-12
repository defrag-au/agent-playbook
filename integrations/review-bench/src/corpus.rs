//! The review corpus: model-facing case metadata, scorer-only answers, and the
//! typed answer shape.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};

/// `case.toml`: what the model is told, and which image(s) it sees.
#[derive(Debug, Clone, Deserialize)]
pub struct Case {
    pub story: String,
    pub arch: String,
    pub os: String,
    pub intent: String,
    pub image: String,
    #[serde(default)]
    pub crop: Option<String>,
    #[serde(default)]
    pub crop_box: Option<[f64; 4]>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExpectedKind {
    Defect,
    Clean,
}

impl ExpectedKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Defect => "defect",
            Self::Clean => "clean",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Must,
    Should,
}

impl Severity {
    pub fn weight(self) -> f64 {
        match self {
            Self::Must => 1.0,
            Self::Should => 0.5,
        }
    }
}

/// One problem the case contains, boxed in full-image pixels as `[x, y, w, h]`.
#[derive(Debug, Clone, Deserialize)]
pub struct ExpectedDefect {
    pub kind: String,
    #[serde(rename = "box")]
    pub bounds: [f64; 4],
    pub severity: Severity,
}

/// `expected.toml`: scorer-facing, never sent to the model.
#[derive(Debug, Clone, Deserialize)]
pub struct Expected {
    pub expected: ExpectedKind,
    #[serde(default)]
    pub defect: Vec<ExpectedDefect>,
}

impl Expected {
    fn validate(&self, slug: &str) -> Result<()> {
        match self.expected {
            ExpectedKind::Defect => {
                ensure!(
                    !self.defect.is_empty(),
                    "{slug}: a defect case needs at least one [[defect]]"
                );
                for defect in &self.defect {
                    ensure!(
                        defect.bounds[2] > 0.0 && defect.bounds[3] > 0.0,
                        "{slug}: a defect box needs a positive width and height"
                    );
                }
            }
            ExpectedKind::Clean => {
                ensure!(
                    self.defect.is_empty(),
                    "{slug}: a clean case must not declare a [[defect]]"
                );
            }
        }
        Ok(())
    }
}

/// The model's answer. Boxes are `[x0, y0, x1, y1]` in 0–1000, relative to
/// the full image, per `RUBRIC.md`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Answer {
    pub defects: Vec<Defect>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Defect {
    #[serde(default = "other_kind")]
    pub kind: String,
    #[serde(rename = "box", default)]
    pub bounds: Option<[f64; 4]>,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default)]
    pub note: String,
}

fn other_kind() -> String {
    "other".to_owned()
}

#[derive(Debug, Clone)]
pub struct LoadedCase {
    pub slug: String,
    pub dir: PathBuf,
    pub case: Case,
    pub expected: Expected,
    pub image_size: (u32, u32),
}

pub struct Corpus {
    pub root: PathBuf,
    pub rubric: String,
    pub cases: Vec<LoadedCase>,
}

impl Corpus {
    /// Read `RUBRIC.md` and every `cases/<slug>/{case.toml,expected.toml}`.
    pub fn load(root: &Path) -> Result<Self> {
        let rubric_path = root.join("RUBRIC.md");
        let rubric = fs::read_to_string(&rubric_path)
            .with_context(|| format!("read the rubric at {}", rubric_path.display()))?;
        let cases_root = root.join("cases");
        let mut dirs: Vec<PathBuf> = fs::read_dir(&cases_root)
            .with_context(|| format!("read the cases directory at {}", cases_root.display()))?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        dirs.sort();
        let mut cases = Vec::new();
        for dir in dirs {
            let slug = dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned();
            let case_path = dir.join("case.toml");
            let expected_path = dir.join("expected.toml");
            ensure!(
                case_path.is_file() && expected_path.is_file(),
                "{slug}: case.toml and expected.toml are both required"
            );
            let case: Case = toml::from_str(&fs::read_to_string(&case_path)?)
                .with_context(|| format!("parse {}", case_path.display()))?;
            let expected: Expected = toml::from_str(&fs::read_to_string(&expected_path)?)
                .with_context(|| format!("parse {}", expected_path.display()))?;
            expected.validate(&slug)?;
            let image_path = dir.join(&case.image);
            let image_size =
                png_size(&image_path).with_context(|| format!("{slug}: read the full image"))?;
            if let Some(crop) = &case.crop {
                let crop_path = dir.join(crop);
                ensure!(
                    crop_path.is_file(),
                    "{slug}: crop {} is missing",
                    crop_path.display()
                );
            }
            cases.push(LoadedCase {
                slug,
                dir,
                case,
                expected,
                image_size,
            });
        }
        ensure!(!cases.is_empty(), "{} has no cases", cases_root.display());
        Ok(Self {
            root: root.to_owned(),
            rubric,
            cases,
        })
    }

    pub fn selected(&self, only: Option<&str>) -> Result<Vec<&LoadedCase>> {
        let cases: Vec<&LoadedCase> = self
            .cases
            .iter()
            .filter(|case| only.is_none_or(|needle| case.slug.contains(needle)))
            .collect();
        ensure!(!cases.is_empty(), "no case matches {only:?}");
        Ok(cases)
    }

    /// The one architecture and OS this corpus was captured on.
    pub fn provenance(&self, cases: &[&LoadedCase]) -> Result<(String, String)> {
        let set: BTreeSet<(&str, &str)> = cases
            .iter()
            .map(|case| (case.case.arch.as_str(), case.case.os.as_str()))
            .collect();
        ensure!(
            set.len() == 1,
            "the corpus mixes architectures or OSes: {set:?}"
        );
        let (arch, os) = set.into_iter().next().expect("one provenance entry");
        Ok((arch.to_owned(), os.to_owned()))
    }
}

/// `RUBRIC.md` + the screen's intent, and the image-order statement.
pub fn prompt(rubric: &str, case: &Case) -> String {
    format!(
        "{rubric}\n\n## Screen intent\n\n{}\n\nThe first image is the full screen. A second image, when present, is a magnification of one region of it. Return only the JSON object.\n",
        case.intent.trim()
    )
}

/// Parse the model's JSON, accepting a fenced block or prose around the object.
pub fn parse_answer(text: &str) -> Result<Answer> {
    let trimmed = text.trim();
    let fenced = trimmed
        .strip_prefix("```")
        .and_then(|rest| rest.strip_suffix("```"))
        .unwrap_or(trimmed);
    let fenced = fenced
        .strip_prefix("json")
        .or_else(|| fenced.strip_prefix("JSON"))
        .unwrap_or(fenced)
        .trim();
    let json = if fenced.starts_with('{') {
        fenced
    } else if let (Some(start), Some(end)) = (fenced.find('{'), fenced.rfind('}')) {
        &fenced[start..=end]
    } else {
        bail!("the answer is not JSON: {trimmed:.120}");
    };
    serde_json::from_str(json).with_context(|| "the answer does not match {\"defects\": [...]}")
}

/// The width and height from a PNG's IHDR chunk.
pub fn png_size(path: &Path) -> Result<(u32, u32)> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    ensure!(
        bytes.len() >= 24 && bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "{} is not a PNG",
        path.display()
    );
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    Ok((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_answer_parses() {
        let answer = parse_answer(r#"{"defects": []}"#).expect("the answer parses");
        assert!(answer.defects.is_empty());
    }

    #[test]
    fn a_fenced_answer_parses() {
        let answer = parse_answer("```json\n{\"defects\": []}\n```").expect("the answer parses");
        assert!(answer.defects.is_empty());
    }

    #[test]
    fn prose_around_the_object_is_ignored() {
        let answer =
            parse_answer(r#"Here it is: {"defects": []} Done."#).expect("the answer parses");
        assert!(answer.defects.is_empty());
    }

    #[test]
    fn a_defect_without_a_box_is_still_a_report() {
        let answer = parse_answer(
            r#"{"defects": [{"kind": "clipped", "confidence": 0.8, "note": "right edge"}]}"#,
        )
        .expect("the answer parses");
        assert_eq!(answer.defects.len(), 1);
        assert_eq!(answer.defects[0].bounds, None);
        assert_eq!(answer.defects[0].kind, "clipped");
    }

    #[test]
    fn a_defect_case_needs_a_defect() {
        let expected = Expected {
            expected: ExpectedKind::Defect,
            defect: Vec::new(),
        };
        assert!(expected.validate("case").is_err());
    }

    #[test]
    fn a_clean_case_may_not_declare_a_defect() {
        let expected = Expected {
            expected: ExpectedKind::Clean,
            defect: vec![ExpectedDefect {
                kind: "clipped".to_owned(),
                bounds: [0.0, 0.0, 10.0, 10.0],
                severity: Severity::Must,
            }],
        };
        assert!(expected.validate("case").is_err());
    }
}
