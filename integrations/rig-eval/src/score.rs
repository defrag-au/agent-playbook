//! Citations in an answer, and the checks a scenario is scored against.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::scenario::Scenario;

/// A `path`, optionally with the line (or range) the answer cites. A bare path is kept because
/// `extra_paths` reads it and a precision denominator should count it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Citation {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line: Option<usize>,
}

impl Citation {
    pub fn render(&self) -> String {
        match (self.line, self.end_line) {
            (Some(start), Some(end)) if start != end => format!("{}:{start}-{end}", self.path),
            (Some(line), _) => format!("{}:{line}", self.path),
            (None, _) => self.path.clone(),
        }
    }
}

/// One pattern, because the answer's citation shape is one shape: a path with a known extension,
/// then optionally `:line` or `:start-end`. `regex` is linear-time, so a long answer is safe.
static CITATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?P<path>[A-Za-z0-9_./~-]+\.(?:rs|md|toml|nix|json|jsonl|yaml|yml|sh|ts|tsx|js|sql|proto|lock|html|css|txt))(?::(?P<start>\d+)(?:-(?P<end>\d+))?)?",
    )
    .expect("the citation pattern is a constant")
});

/// Every citation in `text`, deduplicated. A URL's path is not a citation of the working tree.
pub fn citations(text: &str) -> BTreeSet<Citation> {
    let mut found = BTreeSet::new();
    for captures in CITATION.captures_iter(text) {
        let Some(path) = captures.name("path") else {
            continue;
        };
        let path = path.as_str();
        if path.starts_with("//") {
            continue;
        }
        found.insert(Citation {
            path: path.trim_start_matches("./").to_owned(),
            line: captures
                .name("start")
                .and_then(|line| line.as_str().parse().ok()),
            end_line: captures
                .name("end")
                .and_then(|line| line.as_str().parse().ok()),
        });
    }
    found
}

/// A gold line is hit by an exact citation or by a range that contains it. The cited path may be
/// shorter than the gold path when the answer is written from a directory ("`src/lib.rs:82`").
pub fn check_gold(gold: &str, cited: &BTreeSet<Citation>) -> bool {
    let Some((path, line)) = gold.rsplit_once(':') else {
        return false;
    };
    let Ok(line) = line.parse::<usize>() else {
        return false;
    };
    cited.iter().any(|citation| {
        same_path(&citation.path, path)
            && citation.line.is_some_and(|start| {
                let end = citation.end_line.unwrap_or(start);
                start <= line && line <= end
            })
    })
}

fn same_path(cited: &str, gold: &str) -> bool {
    cited == gold || cited.ends_with(&format!("/{gold}")) || gold.ends_with(&format!("/{cited}"))
}

/// A path is named by itself, by a citation with a shorter path, or by a directory that contains
/// it. `git status` collapses an untracked tree to its top directory, and that is as correct an
/// answer as listing every file under it.
fn names_path(answer: &str, path: &str) -> bool {
    if answer.contains(path) {
        return true;
    }
    if citations(answer)
        .iter()
        .any(|citation| same_path(&citation.path, path))
    {
        return true;
    }
    answer
        .split(|c: char| {
            c.is_whitespace()
                || matches!(
                    c,
                    '`' | '"' | '\'' | '(' | ')' | '[' | ']' | ',' | ';' | ':'
                )
        })
        .filter(|token| token.ends_with('/'))
        .any(|directory| path.starts_with(directory))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Gold,
    Path,
    Answer,
    Tool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Check {
    pub required: String,
    pub source: Source,
    pub found: bool,
}

/// What a run scored, before the process facts are attached.
pub struct Scored {
    pub checks: Vec<Check>,
    pub passed: usize,
    pub total: usize,
    pub score: f64,
    pub precision: Option<f64>,
    pub cited: BTreeSet<Citation>,
}

pub fn score(scenario: &Scenario, answer: Option<&str>, tool_output: &str) -> Scored {
    let cited = answer.map(citations).unwrap_or_default();
    let mut checks = Vec::new();

    for cite in &scenario.gold {
        checks.push(Check {
            required: cite.clone(),
            source: Source::Gold,
            found: check_gold(cite, &cited),
        });
    }
    for path in &scenario.extra_paths {
        checks.push(Check {
            required: path.clone(),
            source: Source::Path,
            found: answer.is_some_and(|answer| names_path(answer, path)),
        });
    }
    if let Some(branch) = &scenario.git_branch {
        checks.push(Check {
            required: branch.clone(),
            source: Source::Answer,
            found: answer.is_some_and(|answer| answer.contains(branch)),
        });
    }
    for path in &scenario.git_paths {
        checks.push(Check {
            required: path.clone(),
            source: Source::Path,
            found: answer.is_some_and(|answer| names_path(answer, path)),
        });
    }
    for text in &scenario.must_say {
        checks.push(Check {
            required: text.clone(),
            source: Source::Answer,
            found: answer.is_some_and(|answer| answer.contains(text)),
        });
    }
    for text in &scenario.tool_says {
        checks.push(Check {
            required: text.clone(),
            source: Source::Tool,
            found: tool_output.contains(text),
        });
    }

    let passed = checks.iter().filter(|check| check.found).count();
    let total = checks.len();
    let score = if total == 0 {
        0.0
    } else {
        passed as f64 / total as f64
    };
    let gold_hits = checks
        .iter()
        .filter(|check| check.source == Source::Gold && check.found)
        .count();
    let precision = if scenario.gold.is_empty() || cited.is_empty() {
        None
    } else {
        Some(gold_hits as f64 / cited.len() as f64)
    };

    Scored {
        checks,
        passed,
        total,
        score,
        precision,
        cited,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn citations_carry_the_line_and_a_range() {
        let found = citations(
            "See `integrations/rig/src/lib.rs:82` and src/redirect.rs:166-170, not \
             https://example.com/docs/other.rs:1.",
        );
        let rendered: Vec<String> = found.iter().map(Citation::render).collect();
        assert!(rendered.contains(&"integrations/rig/src/lib.rs:82".to_owned()));
        assert!(rendered.contains(&"src/redirect.rs:166-170".to_owned()));
        assert!(!rendered
            .iter()
            .any(|citation| citation.contains("other.rs")));
    }

    #[test]
    fn a_gold_line_matches_a_range_and_a_shorter_cited_path() {
        let found = citations("`src/lib.rs:80-90`");
        assert!(check_gold("integrations/rig/src/lib.rs:82", &found));
        assert!(!check_gold(
            "integrations/rig/src/lib.rs:82",
            &citations("`src/lib.rs:90`")
        ));
    }

    #[test]
    fn a_path_without_a_line_is_not_a_gold_hit() {
        let found = citations("`integrations/rig/src/lib.rs`");
        assert!(!check_gold("integrations/rig/src/lib.rs:82", &found));
        assert_eq!(found.len(), 1);
        assert_eq!(found.iter().next().and_then(|citation| citation.line), None);
    }

    #[test]
    fn a_directory_names_the_paths_under_it() {
        assert!(names_path("?? src/driver/", "src/driver/mod.rs"));
        assert!(names_path("src/driver/", "src/driver/nested/file.rs"));
        assert!(!names_path("src/other/", "src/driver/mod.rs"));
    }

    #[test]
    fn a_shorter_citation_names_the_path() {
        assert!(names_path(
            "`src/tools.rs`",
            "integrations/rig/src/tools.rs"
        ));
        assert!(names_path(
            "`integrations/rig/src/tools.rs`",
            "integrations/rig/src/tools.rs"
        ));
        assert!(!names_path(
            "`src/other.rs`",
            "integrations/rig/src/tools.rs"
        ));
    }
}
