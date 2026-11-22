//! Findings captured in `.playbook/findings/`, and the decisions recorded in `.playbook/FINDINGS.md`.
//!
//! A finding is an observation recorded before anyone decided what to do about it; the loop is
//! documented in `docs/findings.md`. The two halves of the record are deliberately separate:
//!
//! - `findings/` is the **working set**. One file per open observation, not committed.
//! - `FINDINGS.md` is the **decision log**. One section per decided finding, committed, append-only.
//!
//! This module only reads. A finding that does not parse becomes a warning rather than being
//! dropped — a record that disappears quietly is the failure mode the composer exists to prevent.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::frontmatter::split_document;

/// The four outcomes a decision may name.
pub const OUTCOMES: [&str; 4] = ["fix-now", "fix-later", "accept-as-is", "reject"];

const LOG_FILE: &str = "FINDINGS.md";

const SKIP_DIRS: [&str; 10] = [
    ".git",
    ".direnv",
    ".tmp",
    "target",
    "node_modules",
    "dist",
    "vendor",
    ".build",
    ".svelte-kit",
    "out",
];

/// How deep below the scan root a repository may sit. `~/code/<org>/<repo>` is two.
const DEFAULT_DEPTH: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub path: PathBuf,
    pub slug: String,
    pub captured: String,
    pub project: String,
    pub thread: String,
    pub message: String,
    pub class: String,
    pub source: String,
}

impl Finding {
    /// `abcd1234@42` — short enough for a table, precise enough to reopen the trace.
    pub fn pointer(&self) -> Option<String> {
        if self.thread.is_empty() {
            return None;
        }
        let short: String = self.thread.chars().take(8).collect();
        if self.message.is_empty() {
            Some(short)
        } else {
            Some(format!("{short}@{}", self.message))
        }
    }
}

/// One decided finding, read from the repository's `FINDINGS.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub path: PathBuf,
    pub captured: String,
    pub title: String,
    pub outcome: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default)]
pub struct Scan {
    pub findings: Vec<Finding>,
    pub decisions: Vec<Decision>,
    pub warnings: Vec<String>,
    pub repos: usize,
}

/// Find every repository at or below `dir` that carries a `.playbook/findings/` or a
/// `.playbook/FINDINGS.md`, or the repository enclosing `dir`.
///
/// A directory is a repository when it has either; the walk stops there, so a nested checkout is
/// not descended into. An unreadable directory becomes a warning; the scan continues.
pub fn scan(dir: &Path) -> Scan {
    scan_to_depth(dir, DEFAULT_DEPTH)
}

pub fn scan_to_depth(dir: &Path, max_depth: usize) -> Scan {
    let mut roots = Vec::new();
    if let Some(found) = playbook_dir(dir) {
        roots.push(found);
    } else if let Some(found) = dir.ancestors().find_map(playbook_dir) {
        roots.push(found);
    } else {
        walk(dir, 0, max_depth, &mut roots);
    }
    roots.sort();
    roots.dedup();

    let mut scan = Scan {
        repos: roots.len(),
        ..Scan::default()
    };
    for playbook in roots {
        collect(&playbook, &mut scan);
    }
    scan.findings.sort_by(|left, right| {
        left.captured
            .cmp(&right.captured)
            .then_with(|| left.slug.cmp(&right.slug))
    });
    scan
}

fn playbook_dir(dir: &Path) -> Option<PathBuf> {
    let playbook = dir.join(".playbook");
    let has_findings = playbook.join("findings").is_dir();
    let has_log = playbook.join(LOG_FILE).is_file();
    (has_findings || has_log).then_some(playbook)
}

fn walk(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<PathBuf>) {
    if let Some(found) = playbook_dir(dir) {
        out.push(found);
        return;
    }
    if depth >= max_depth {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut children: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .filter(|path| !is_skipped(path))
        .collect();
    children.sort();
    for child in children {
        walk(&child, depth + 1, max_depth, out);
    }
}

fn is_skipped(path: &Path) -> bool {
    match path.file_name().and_then(|name| name.to_str()) {
        Some(name) => name.starts_with('.') || SKIP_DIRS.contains(&name),
        None => true,
    }
}

fn collect(playbook: &Path, scan: &mut Scan) {
    let findings = playbook.join("findings");
    if findings.is_dir() {
        collect_findings(&findings, scan);
    }

    let log = playbook.join(LOG_FILE);
    if log.is_file() {
        match parse_log(&log) {
            Ok(decisions) => scan.decisions.extend(decisions),
            Err(message) => scan.warnings.push(message),
        }
    }
}

fn collect_findings(dir: &Path, scan: &mut Scan) {
    let Ok(entries) = fs::read_dir(dir) else {
        scan.warnings
            .push(format!("could not read {}", dir.display()));
        return;
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .collect();
    files.sort();
    for path in files {
        match parse_finding(&path) {
            Ok(Some(finding)) => scan.findings.push(finding),
            Ok(None) => {}
            Err(message) => scan.warnings.push(message),
        }
    }
}

/// A file with no frontmatter is not a finding (the directory's `README.md`). A file with
/// frontmatter and no `captured` key is a malformed finding, and says so.
fn parse_finding(path: &Path) -> Result<Option<Finding>, String> {
    let source =
        fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let split = split_document(&source);
    if split.fields.is_empty() {
        return Ok(None);
    }
    let captured = split.get("captured");
    if captured.is_empty() {
        return Err(format!("{}: frontmatter has no `captured`", path.display()));
    }

    Ok(Some(Finding {
        path: path.to_path_buf(),
        slug: path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_owned(),
        captured: captured.to_owned(),
        project: split.get("project").to_owned(),
        thread: split.get("thread").to_owned(),
        message: split.get("message").to_owned(),
        class: split.get("class").to_owned(),
        source: split.get("source").to_owned(),
    }))
}

/// The log is append-only: a heading starts a decision, and the first outcome token in the lines
/// under it is the outcome. A heading with no outcome is a warning, and is still listed.
fn parse_log(path: &Path) -> Result<Vec<Decision>, String> {
    let source =
        fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;

    let mut decisions = Vec::new();
    let mut heading: Option<String> = None;
    let mut body = String::new();
    let mut in_fence = false;

    let flush = |heading: &mut Option<String>, body: &mut String, out: &mut Vec<Decision>| {
        let Some(title) = heading.take() else {
            return;
        };
        let (captured, title) = split_heading(&title);
        let (outcome, detail) = split_body(body);
        out.push(Decision {
            path: path.to_path_buf(),
            captured,
            title,
            outcome,
            detail,
        });
        body.clear();
    };

    for line in source.lines() {
        // A fenced example inside the log is documentation, not a decision.
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }

        if let Some(title) = line.strip_prefix("## ") {
            flush(&mut heading, &mut body, &mut decisions);
            heading = Some(title.trim().to_owned());
        } else if heading.is_some() {
            body.push_str(line);
            body.push('\n');
        }
    }
    flush(&mut heading, &mut body, &mut decisions);

    Ok(decisions)
}

/// `2026-11-05 · slice exits name the rest of the file` — the date is optional.
fn split_heading(heading: &str) -> (String, String) {
    if let Some((date, title)) = heading.split_once(" · ") {
        if parse_date(date).is_some() {
            return (date.trim().to_owned(), title.trim().to_owned());
        }
    }
    (String::new(), heading.trim().to_owned())
}

/// The outcome is the first of [`OUTCOMES`] named in the body; the detail is what follows it.
fn split_body(body: &str) -> (String, String) {
    for outcome in OUTCOMES {
        let needle = format!("`{outcome}`");
        if let Some(index) = body.find(&needle) {
            let detail = body[index + needle.len()..]
                .trim()
                .trim_start_matches('·')
                .trim()
                .to_owned();
            return (outcome.to_owned(), detail);
        }
    }
    (String::new(), body.trim().to_owned())
}

/// Days since the Unix epoch, so age needs no date library. The composer has no dependencies.
pub fn today() -> i64 {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    (seconds / 86_400) as i64
}

pub fn age_days(captured: &str, today: i64) -> Option<i64> {
    parse_date(captured).map(|day| (today - day).max(0))
}

fn parse_date(value: &str) -> Option<i64> {
    let mut parts = value.trim().split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(days_from_civil(year, month, day))
}

/// Howard Hinnant's `days_from_civil`. Exact for the proleptic Gregorian calendar.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_index = if month > 2 { month - 3 } else { month + 9 } as i64;
    let day_of_year = (153 * month_index + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// What to print: the open findings the filters kept, every decision read, and the counts.
pub struct Report<'a> {
    pub findings: Vec<&'a Finding>,
    pub decisions: &'a [Decision],
    pub repos: usize,
    pub total: usize,
    pub today: i64,
    pub show_decisions: bool,
}

/// The human report. Open findings first, then the decision log when asked for it.
pub fn print(report: &Report<'_>) {
    let by_outcome = outcome_summary(report.decisions);
    let finding_word = if report.findings.len() == 1 {
        "finding"
    } else {
        "findings"
    };
    let repo_word = if report.repos == 1 {
        "repository"
    } else {
        "repositories"
    };
    println!(
        "# {} {finding_word} open · {} decided{by_outcome} · {} {repo_word}",
        report.findings.len(),
        report.decisions.len(),
        report.repos
    );

    if report.findings.is_empty() {
        if report.total == 0 {
            println!("# nothing captured yet");
        } else {
            println!("# nothing matches; {} captured in all", report.total);
        }
    } else {
        println!();
        print_findings(&report.findings, report.today);
    }

    if report.show_decisions && !report.decisions.is_empty() {
        println!();
        print_decisions(report.decisions);
    }
}

fn print_findings(findings: &[&Finding], today: i64) {
    let class_width = findings
        .iter()
        .map(|finding| display_class(finding).len())
        .max()
        .unwrap_or(5)
        .max(5);
    let project_width = findings
        .iter()
        .map(|finding| finding.project.len())
        .max()
        .unwrap_or(7)
        .max(7);

    println!(
        "captured    age  {:<class_width$}  {:<project_width$}  {:<13}  finding",
        "class", "project", "where"
    );

    for finding in findings {
        let age = age_days(&finding.captured, today)
            .map(|days| format!("{days}d"))
            .unwrap_or_else(|| "-".to_owned());
        println!(
            "{:<10}  {:>3}  {:<class_width$}  {:<project_width$}  {:<13}  {}",
            finding.captured,
            age,
            display_class(finding),
            if finding.project.is_empty() {
                "-"
            } else {
                &finding.project
            },
            finding.pointer().unwrap_or_else(|| "-".to_owned()),
            finding.slug
        );
    }

    let mut classes: Vec<(&str, usize)> = Vec::new();
    for finding in findings {
        let class = display_class(finding);
        match classes.iter_mut().find(|(name, _)| *name == class) {
            Some((_, count)) => *count += 1,
            None => classes.push((class, 1)),
        }
    }
    classes.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(right.0)));

    println!();
    println!(
        "# by class: {}",
        classes
            .iter()
            .map(|(name, count)| format!("{name} {count}"))
            .collect::<Vec<String>>()
            .join(" · ")
    );
}

fn print_decisions(decisions: &[Decision]) {
    println!("# decided");

    let title_width = decisions
        .iter()
        .map(|decision| decision.outcome.len())
        .max()
        .unwrap_or(7)
        .max(7);

    for decision in decisions {
        println!(
            "{:<10}  {:<title_width$}  {}",
            if decision.captured.is_empty() {
                "-"
            } else {
                &decision.captured
            },
            if decision.outcome.is_empty() {
                "-"
            } else {
                &decision.outcome
            },
            decision.title
        );
        if !decision.detail.is_empty() {
            println!("            {:<title_width$}  {}", "", decision.detail);
        }
    }
}

fn display_class(finding: &Finding) -> &str {
    if finding.class.is_empty() {
        "unknown"
    } else {
        &finding.class
    }
}

fn outcome_summary(decisions: &[Decision]) -> String {
    if decisions.is_empty() {
        return String::new();
    }
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for decision in decisions {
        let outcome = if decision.outcome.is_empty() {
            "-"
        } else {
            &decision.outcome
        };
        match counts.iter_mut().find(|(name, _)| *name == outcome) {
            Some((_, count)) => *count += 1,
            None => counts.push((outcome, 1)),
        }
    }
    counts.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(right.0)));
    format!(
        " ({})",
        counts
            .iter()
            .map(|(name, count)| format!("{name} {count}"))
            .collect::<Vec<String>>()
            .join(" · ")
    )
}

/// A typed JSON document, written by hand because the composer carries no dependencies.
pub fn to_json(report: &Report<'_>) -> String {
    let mut out = String::from("{\n  \"repos\": ");
    out.push_str(&report.repos.to_string());
    out.push_str(",\n  \"findings\": [");

    for (index, finding) in report.findings.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let age = age_days(&finding.captured, report.today);
        out.push_str("\n    {\n");
        out.push_str(&format!(
            "      \"captured\": {},\n",
            json_string(&finding.captured)
        ));
        out.push_str(&format!(
            "      \"age_days\": {},\n",
            age.map(|days| days.to_string())
                .unwrap_or_else(|| "null".to_owned())
        ));
        out.push_str(&format!(
            "      \"project\": {},\n",
            json_string(&finding.project)
        ));
        out.push_str(&format!(
            "      \"class\": {},\n",
            json_string(display_class(finding))
        ));
        out.push_str(&format!(
            "      \"source\": {},\n",
            json_string(&finding.source)
        ));
        out.push_str(&format!(
            "      \"thread\": {},\n",
            json_string(&finding.thread)
        ));
        out.push_str(&format!(
            "      \"message\": {},\n",
            json_string(&finding.message)
        ));
        out.push_str(&format!(
            "      \"slug\": {},\n",
            json_string(&finding.slug)
        ));
        out.push_str(&format!(
            "      \"path\": {}\n",
            json_string(&finding.path.display().to_string())
        ));
        out.push_str("    }");
    }

    out.push_str("\n  ],\n  \"decisions\": [");
    for (index, decision) in report.decisions.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("\n    {\n");
        out.push_str(&format!(
            "      \"captured\": {},\n",
            json_string(&decision.captured)
        ));
        out.push_str(&format!(
            "      \"title\": {},\n",
            json_string(&decision.title)
        ));
        out.push_str(&format!(
            "      \"outcome\": {},\n",
            json_string(&decision.outcome)
        ));
        out.push_str(&format!(
            "      \"detail\": {}\n",
            json_string(&decision.detail)
        ));
        out.push_str("    }");
    }

    out.push_str("\n  ]\n}\n");
    out
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                let code = control as u32;
                out.push_str(&format!("\\u{code:04x}"));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(prefix: &str) -> PathBuf {
        let id = NEXT.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "playbook-findings-{prefix}-{}-{id}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("temp dir");
        path
    }

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent directory");
        }
        fs::write(path, content).expect("write fixture");
    }

    fn finding(project: &str, class: &str) -> String {
        format!(
            "---\ncaptured: 2026-11-05\nproject: {project}\nthread: abcdef0123456789\nmessage: 42\nclass: {class}\nsource: instrument\n---\n\nSomething happened.\n"
        )
    }

    const LOG: &str = "# Findings\n\nDecisions.\n\n## 2026-11-05 · slice exits name the rest of the file\n`fix-now` · rule `rules/org/defrag/slice-exits-window`\n\n## 2026-11-05 · exits assume depth-first reading\n`accept-as-is` · the footer is a hint\n";

    #[test]
    fn parses_a_finding() {
        let dir = temp_dir("parse");
        let path = dir.join("2026-11-05-thing.md");
        write(&path, &finding("mitos", "missing-capability"));

        let parsed = parse_finding(&path).expect("parses").expect("is a finding");
        assert_eq!(parsed.project, "mitos");
        assert_eq!(parsed.class, "missing-capability");
        assert_eq!(parsed.pointer().as_deref(), Some("abcdef01@42"));
    }

    #[test]
    fn a_readme_without_frontmatter_is_not_a_finding() {
        let dir = temp_dir("readme");
        let path = dir.join("README.md");
        write(&path, "# Findings\n\nWhat this directory is.\n");

        assert!(parse_finding(&path).expect("parses").is_none());
    }

    #[test]
    fn a_malformed_finding_is_a_warning_and_not_dropped() {
        let dir = temp_dir("malformed");
        let path = dir.join(".playbook/findings/broken.md");
        write(&path, "---\nproject: mitos\nclass: unknown\n---\n\nBody.\n");

        let scan = scan_to_depth(&dir, DEFAULT_DEPTH);
        assert!(scan.findings.is_empty());
        assert_eq!(scan.warnings.len(), 1);
        assert!(scan.warnings[0].contains("no `captured`"));
    }

    #[test]
    fn parses_the_decision_log() {
        let dir = temp_dir("log");
        let path = dir.join("FINDINGS.md");
        write(&path, LOG);

        let decisions = parse_log(&path).expect("parses");
        assert_eq!(decisions.len(), 2);
        assert_eq!(decisions[0].captured, "2026-11-05");
        assert_eq!(decisions[0].title, "slice exits name the rest of the file");
        assert_eq!(decisions[0].outcome, "fix-now");
        assert_eq!(
            decisions[0].detail,
            "rule `rules/org/defrag/slice-exits-window`"
        );
        assert_eq!(decisions[1].outcome, "accept-as-is");
    }

    #[test]
    fn a_fenced_example_is_not_a_decision() {
        let dir = temp_dir("fenced");
        let path = dir.join("FINDINGS.md");
        write(
            &path,
            "# Findings\n\n```markdown\n## 2026-11-05 · an example\n`fix-now` · rule `x`\n```\n",
        );

        let decisions = parse_log(&path).expect("parses");
        assert!(decisions.is_empty());
    }

    #[test]
    fn a_heading_without_a_date_keeps_its_title() {
        let dir = temp_dir("undated");
        let path = dir.join("FINDINGS.md");
        write(&path, "## a finding with no date\n`reject` · not real\n");

        let decisions = parse_log(&path).expect("parses");
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].captured, "");
        assert_eq!(decisions[0].title, "a finding with no date");
        assert_eq!(decisions[0].outcome, "reject");
    }

    #[test]
    fn a_heading_with_no_outcome_is_listed_with_an_empty_one() {
        let dir = temp_dir("no-outcome");
        let path = dir.join("FINDINGS.md");
        write(&path, "## 2026-11-05 · undecided\n\nSome prose.\n");

        let decisions = parse_log(&path).expect("parses");
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].outcome, "");
    }

    #[test]
    fn discovers_repositories_below_the_scan_root_and_stops_at_each() {
        let root = temp_dir("discover");
        write(
            &root.join("a/.playbook/findings/2026-11-05-a.md"),
            &finding("a", "unknown"),
        );
        write(
            &root.join("b/nested/.playbook/FINDINGS.md"),
            "## 2026-11-05 · decided\n`reject` · no\n",
        );
        write(
            &root.join("b/nested/inner/.playbook/findings/2026-11-05-c.md"),
            &finding("inner", "unknown"),
        );
        write(
            &root.join("a/.playbook/findings/README.md"),
            "# Findings\n\nNo frontmatter.\n",
        );

        let scan = scan_to_depth(&root, DEFAULT_DEPTH);
        assert_eq!(scan.repos, 2);
        assert_eq!(scan.findings.len(), 1);
        assert_eq!(scan.decisions.len(), 1);
        assert!(scan.warnings.is_empty());
    }

    #[test]
    fn finds_the_enclosing_repository_from_a_subdirectory() {
        let root = temp_dir("enclosing");
        write(
            &root.join("a/.playbook/findings/2026-11-05-a.md"),
            &finding("a", "unknown"),
        );

        let scan = scan_to_depth(&root.join("a/src/deep"), DEFAULT_DEPTH);
        assert_eq!(scan.repos, 1);
        assert_eq!(scan.findings.len(), 1);
    }

    #[test]
    fn civil_dates_are_exact() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(
            days_from_civil(2026, 11, 5) - days_from_civil(2026, 11, 4),
            1
        );
        assert_eq!(
            age_days("2026-11-05", days_from_civil(2026, 11, 8)),
            Some(3)
        );
        assert_eq!(age_days("not a date", 0), None);
    }

    #[test]
    fn json_escapes_control_characters_and_quotes() {
        assert_eq!(json_string("a\"b\\c\nd"), "\"a\\\"b\\\\c\\nd\"");
    }

    #[test]
    fn json_carries_the_decisions() {
        let finding = Finding {
            path: PathBuf::from("/repo/a.md"),
            slug: "a".to_owned(),
            captured: "2026-11-05".to_owned(),
            project: "mitos".to_owned(),
            thread: "abcdef01".to_owned(),
            message: "42".to_owned(),
            class: "unknown".to_owned(),
            source: "instrument".to_owned(),
        };
        let decision = Decision {
            path: PathBuf::from("/repo/FINDINGS.md"),
            captured: "2026-11-05".to_owned(),
            title: "something".to_owned(),
            outcome: "reject".to_owned(),
            detail: "not real".to_owned(),
        };
        let report = Report {
            findings: vec![&finding],
            decisions: std::slice::from_ref(&decision),
            repos: 1,
            total: 1,
            today: 0,
            show_decisions: true,
        };

        let json = to_json(&report);
        assert!(json.contains("\"findings\": ["));
        assert!(json.contains("\"outcome\": \"reject\""));
        assert!(json.contains("\"age_days\": 0"));
    }
}
