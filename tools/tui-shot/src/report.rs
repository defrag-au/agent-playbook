//! The typed sidecar and the run report.
//!
//! A capture writes two files: `<stem>.txt` is the screen itself and is the review; `<stem>.json`
//! is the second check, listing what a glance misses (the cursor, styles, rows that reach the
//! right edge, control characters, whether the child was still drawing). The run writes
//! `run.txt` and `run.json` beside them.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};
use serde::Serialize;

use crate::scenario::Size;
use crate::screen::{Cursor, StyleRun, Warning};

#[derive(Debug, Clone, Serialize)]
pub struct ProgramInfo {
    pub bin: String,
    /// Relative to the workspace root, so a report is readable in another checkout.
    pub path: String,
    pub profile: String,
    pub bytes: u64,
    pub sha256: String,
    pub modified: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExitInfo {
    pub code: Option<i32>,
    pub signal: Option<String>,
    /// True when `tui-shot` sent the signal as it cleaned up, rather than the child exiting on its
    /// own. A normal end to a single-shot capture is this, not a crash.
    pub terminated_by_tool: bool,
}

impl ExitInfo {
    pub fn summary(&self) -> String {
        if self.terminated_by_tool {
            return "terminated by tui-shot".to_owned();
        }
        match (self.code, &self.signal) {
            (Some(code), _) => format!("exit {code}"),
            (None, Some(signal)) => format!("signal {signal}"),
            (None, None) => "exited".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Observation {
    pub chunks: u64,
    pub bytes: u64,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RowInfo {
    pub row: u16,
    /// Columns up to and including the last one with content.
    pub cells: u16,
    pub chars: usize,
    /// True when the row uses every column. A border legitimately does; this is not the same
    /// question as `screen.edge_rows`, which asks whether prose may have been cut.
    pub full: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScreenInfo {
    pub alternate: bool,
    pub cursor: Cursor,
    pub rows_used: usize,
    pub edge_rows: Vec<u16>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaptureReport {
    pub capture: String,
    pub program: ProgramInfo,
    pub size: Size,
    pub captured_at: String,
    pub reason: String,
    pub quiet_ms: u64,
    pub observation: Observation,
    pub screen: ScreenInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit: Option<ExitInfo>,
    pub rows: Vec<RowInfo>,
    pub styles: Vec<StyleRun>,
    pub styles_truncated: bool,
    pub warnings: Vec<Warning>,
}

impl CaptureReport {
    pub fn write(&self, stem: &Path, screen_text: &str) -> Result<(PathBuf, PathBuf)> {
        if let Some(parent) = stem.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        let text_path = append_suffix(stem, ".txt");
        let json_path = append_suffix(stem, ".json");
        fs::write(&text_path, screen_text)
            .with_context(|| format!("write {}", text_path.display()))?;
        let json = serde_json::to_string_pretty(self)?;
        fs::write(&json_path, format!("{json}\n"))
            .with_context(|| format!("write {}", json_path.display()))?;
        Ok((text_path, json_path))
    }

    pub fn summary(&self) -> String {
        let warnings = self.warnings.len();
        format!(
            "{}  {}  {}  {} style{}  {} warning{}",
            self.size,
            self.reason,
            self.exit
                .as_ref()
                .map_or_else(|| "running".to_owned(), ExitInfo::summary),
            self.styles.len(),
            plural(self.styles.len()),
            warnings,
            plural(warnings)
        )
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    Pass,
    Fail,
}

#[derive(Debug, Clone, Serialize)]
pub struct TuiRunInfo {
    pub name: String,
    pub bin: String,
    pub path: String,
    pub profile: String,
    pub pid: Option<u32>,
    pub args: Vec<String>,
    pub size: Size,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit: Option<ExitInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepResult {
    pub line: usize,
    pub action: String,
    pub tui: String,
    pub detail: String,
    pub outcome: String,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaptureRef {
    pub stem: String,
    pub text: String,
    pub json: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssertionResult {
    pub line: usize,
    pub tui: String,
    pub check: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunReport {
    pub scenario: String,
    pub artifacts: String,
    pub started_at: String,
    pub elapsed_ms: u64,
    pub tuis: Vec<TuiRunInfo>,
    pub steps: Vec<StepResult>,
    pub captures: Vec<CaptureRef>,
    pub assertions: Vec<AssertionResult>,
    pub status: RunStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RunReport {
    pub fn new(scenario: &Path, artifacts: &Path) -> Self {
        Self {
            scenario: scenario.display().to_string(),
            artifacts: artifacts.display().to_string(),
            started_at: now_rfc3339(),
            elapsed_ms: 0,
            tuis: Vec::new(),
            steps: Vec::new(),
            captures: Vec::new(),
            assertions: Vec::new(),
            status: RunStatus::Pass,
            error: None,
        }
    }

    pub fn fail(&mut self, error: impl Into<String>) {
        self.status = RunStatus::Fail;
        self.error = Some(error.into());
    }

    pub fn write(&self, directory: &Path) -> Result<(PathBuf, PathBuf)> {
        fs::create_dir_all(directory).with_context(|| format!("create {}", directory.display()))?;
        let text_path = directory.join("run.txt");
        let json_path = directory.join("run.json");
        fs::write(&text_path, self.render_text())
            .with_context(|| format!("write {}", text_path.display()))?;
        let json = serde_json::to_string_pretty(self)?;
        fs::write(&json_path, format!("{json}\n"))
            .with_context(|| format!("write {}", json_path.display()))?;
        Ok((text_path, json_path))
    }

    pub fn render_text(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        let _ = writeln!(out, "scenario   {}", self.scenario);
        let _ = writeln!(out, "artifacts  {}", self.artifacts);
        let _ = writeln!(out, "started    {}", self.started_at);
        let _ = writeln!(out, "elapsed    {} ms", self.elapsed_ms);
        let _ = writeln!(out, "tuis       {}", self.tuis.len());
        for tui in &self.tuis {
            let exit = tui
                .exit
                .as_ref()
                .map_or_else(|| "running".to_owned(), ExitInfo::summary);
            let pid = tui
                .pid
                .map_or_else(|| "-".to_owned(), |pid| pid.to_string());
            let _ = writeln!(
                out,
                "  {:<10} {:<18} {}  {:<7} pid {}  {}",
                tui.name, tui.bin, tui.size, tui.profile, pid, exit
            );
            let _ = writeln!(out, "  {:<10} {}", "", tui.path);
        }

        let _ = writeln!(out, "steps      {}", self.steps.len());
        for step in &self.steps {
            let _ = writeln!(
                out,
                "  {:>3}  {:<8} {:<10} {:<44} {}  {} ms",
                step.line,
                step.action,
                step.tui,
                truncate(&step.detail, 44),
                step.outcome,
                step.elapsed_ms
            );
        }

        if !self.captures.is_empty() {
            let _ = writeln!(out, "captures   {}", self.captures.len());
            for capture in &self.captures {
                let _ = writeln!(out, "  {:<28} {}", capture.stem, capture.summary);
            }
        }

        if !self.assertions.is_empty() {
            let _ = writeln!(out, "assertions {}", self.assertions.len());
            for assertion in &self.assertions {
                let _ = writeln!(
                    out,
                    "  {:>3}  {:<10} {:<40} {}",
                    assertion.line,
                    assertion.tui,
                    assertion.check,
                    if assertion.ok { "ok" } else { "FAILED" }
                );
            }
        }

        if let Some(error) = &self.error {
            let _ = writeln!(out, "error      {error}");
        }
        let _ = writeln!(
            out,
            "result     {}",
            match self.status {
                RunStatus::Pass => "pass",
                RunStatus::Fail => "FAIL",
            }
        );
        out
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckTui {
    pub name: String,
    pub bin: String,
    pub size: Size,
    pub profile: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckStep {
    pub line: usize,
    pub action: String,
    pub tui: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckReport {
    pub scenario: String,
    pub valid: bool,
    pub tuis: Vec<CheckTui>,
    pub steps: Vec<CheckStep>,
    pub captures: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl CheckReport {
    pub fn new(scenario: &Path) -> Self {
        Self {
            scenario: scenario.display().to_string(),
            valid: true,
            tuis: Vec::new(),
            steps: Vec::new(),
            captures: Vec::new(),
            error: None,
        }
    }

    pub fn render_text(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        let _ = writeln!(out, "scenario  {}", self.scenario);
        let _ = writeln!(out, "tuis      {}", self.tuis.len());
        for tui in &self.tuis {
            let state = match (&tui.path, &tui.error) {
                (Some(path), _) => path.clone(),
                (None, Some(error)) => format!("NOT BUILT: {error}"),
                (None, None) => "not found".to_owned(),
            };
            let _ = writeln!(
                out,
                "  {:<10} {:<18} {}  {:<7} {}",
                tui.name, tui.bin, tui.size, tui.profile, state
            );
        }
        let _ = writeln!(out, "steps     {}", self.steps.len());
        for step in &self.steps {
            let _ = writeln!(
                out,
                "  {:>3}  {:<8} {:<10} {}",
                step.line, step.action, step.tui, step.detail
            );
        }
        if !self.captures.is_empty() {
            let _ = writeln!(out, "captures  {}", self.captures.join(", "));
        }
        if let Some(error) = &self.error {
            let _ = writeln!(out, "error     {error}");
        }
        let _ = writeln!(
            out,
            "result    {}",
            if self.valid { "valid" } else { "INVALID" }
        );
        out
    }
}

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Hide the value after a secret-shaped flag. This is a mitigation, not a boundary: a positional
/// argument is recorded as written, and so is the scenario file that supplied it. The run report is
/// an artefact, and an artefact that quotes a token is one paste away from a leak.
pub fn redact_args(args: &[String]) -> Vec<String> {
    const SECRET: &[&str] = &[
        "token",
        "secret",
        "password",
        "passwd",
        "credential",
        "authorization",
        "api-key",
        "apikey",
        "access-key",
        "private-key",
    ];

    fn secret_flag(arg: &str) -> bool {
        if !arg.starts_with('-') {
            return false;
        }
        let name = arg.trim_start_matches('-');
        let name = name.split('=').next().unwrap_or(name).to_ascii_lowercase();
        SECRET.iter().any(|needle| name.contains(needle))
    }

    let mut out = Vec::with_capacity(args.len());
    let mut hide_next = false;
    for arg in args {
        if hide_next {
            out.push("<redacted>".to_owned());
            hide_next = false;
            continue;
        }
        if let Some((name, _)) = arg.split_once('=') {
            if secret_flag(name) {
                out.push(format!("{name}=<redacted>"));
                continue;
            }
        }
        out.push(arg.clone());
        if secret_flag(arg) {
            hide_next = true;
        }
    }
    out
}

fn append_suffix(stem: &Path, suffix: &str) -> PathBuf {
    let mut text = stem.as_os_str().to_os_string();
    text.push(suffix);
    PathBuf::from(text)
}

fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    let mut cut = text
        .chars()
        .take(width.saturating_sub(1))
        .collect::<String>();
    cut.push('…');
    cut
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_secret_shaped_arguments() {
        let args = [
            "--base",
            "ws://127.0.0.1:7100",
            "--token",
            "real-token",
            "--api-key=abc",
            "--session",
            "s-1",
        ]
        .map(str::to_owned)
        .to_vec();
        assert_eq!(
            redact_args(&args),
            [
                "--base",
                "ws://127.0.0.1:7100",
                "--token",
                "<redacted>",
                "--api-key=<redacted>",
                "--session",
                "s-1",
            ]
        );
    }

    #[test]
    fn does_not_treat_a_bare_key_flag_as_a_secret() {
        let args = ["--key", "1"].map(str::to_owned).to_vec();
        assert_eq!(redact_args(&args), ["--key", "1"]);
    }
}
