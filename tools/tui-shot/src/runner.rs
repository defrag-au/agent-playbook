//! Execute a scenario against the processes it declares.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use crate::bin;
use crate::keys;
use crate::pty::{CaptureOutcome, Tui, WaitOutcome};
use crate::report::{
    now_rfc3339, AssertionResult, CaptureRef, CaptureReport, CheckReport, CheckStep, CheckTui,
    Observation, RowInfo, RunReport, RunStatus, ScreenInfo, StepResult, TuiRunInfo,
};
use crate::scenario::{
    safe_relative, Assertion, Profile, RunConfig, Scenario, Size, Step, TuiSpec, Wait,
};

pub struct RunOptions {
    pub scenario: PathBuf,
    pub artifacts: Option<PathBuf>,
    pub json: bool,
}

pub struct CheckOptions {
    pub scenario: PathBuf,
    pub json: bool,
}

pub enum SingleAction {
    Key(String),
    Type(String),
    Wait(String),
}

pub struct SingleShot {
    pub bin: String,
    pub size: Size,
    pub out: PathBuf,
    pub profile: Profile,
    pub term: String,
    pub actions: Vec<SingleAction>,
    pub args: Vec<String>,
    pub timeout: Duration,
    pub settle: Duration,
    pub json: bool,
}

pub fn run(options: RunOptions) -> Result<i32> {
    let cwd = working_directory()?;
    let scenario = Scenario::load(&options.scenario)?;
    let configured = options
        .artifacts
        .clone()
        .unwrap_or_else(|| scenario.run.artifacts.clone());
    let artifacts = prepare_artifacts(&cwd, &configured)?;
    execute(
        &scenario,
        &options.scenario,
        &cwd,
        &artifacts,
        true,
        options.json,
    )
}

pub fn check(options: CheckOptions) -> Result<i32> {
    let scenario = Scenario::load(&options.scenario)?;
    let cwd = working_directory()?;
    let mut report = CheckReport::new(&options.scenario);

    for spec in &scenario.tuis {
        match bin::resolve(&spec.bin, spec.profile, &cwd) {
            Ok(resolved) => report.tuis.push(CheckTui {
                name: spec.name.clone(),
                bin: spec.bin.clone(),
                size: spec.size,
                profile: spec.profile.to_string(),
                path: Some(resolved.info.path),
                error: None,
            }),
            Err(error) => {
                report.valid = false;
                report.tuis.push(CheckTui {
                    name: spec.name.clone(),
                    bin: spec.bin.clone(),
                    size: spec.size,
                    profile: spec.profile.to_string(),
                    path: None,
                    error: Some(format!("{error:#}")),
                });
            }
        }
    }

    report.steps = scenario
        .steps
        .iter()
        .zip(&scenario.step_lines)
        .map(|(step, line)| CheckStep {
            line: *line,
            action: step.action().to_owned(),
            tui: step.tui().to_owned(),
            detail: step.detail(),
        })
        .collect();
    report.captures = scenario
        .captures()
        .iter()
        .map(|path| path.display().to_string())
        .collect();

    if !report.valid {
        report.error = Some(
            "one or more binaries are not built or not declared by this workspace; build them \
             first, as `cargo build --bin <name>`"
                .to_owned(),
        );
    }

    if options.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", report.render_text());
    }
    Ok(if report.valid { 0 } else { 2 })
}

pub fn single_shot(options: SingleShot) -> Result<i32> {
    let cwd = working_directory()?;
    if !safe_relative(&options.out) || options.out.file_name().is_none() {
        bail!(
            "out {} must be a relative file path inside this repository, with no `..`",
            options.out.display()
        );
    }
    let parent = options
        .out
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let artifacts = prepare_artifacts(&cwd, parent)?;
    let stem = capture_stem(Path::new(options.out.file_name().expect("checked above")));

    let name = "tui";
    let mut steps = Vec::new();
    let mut lines = Vec::new();
    // A key sent before the app has drawn can land in canonical mode and be eaten. Unless the
    // caller's first step is already a wait, let the first frame arrive first.
    if !matches!(options.actions.first(), Some(SingleAction::Wait(_))) {
        steps.push(Step::Wait {
            tui: name.to_owned(),
            waits: vec![Wait::Idle(options.settle)],
            timeout: None,
        });
        lines.push(1);
    }
    for action in options.actions {
        steps.push(match action {
            SingleAction::Wait(text) => Step::Wait {
                tui: name.to_owned(),
                waits: vec![Wait::Contains(text)],
                timeout: None,
            },
            SingleAction::Key(key) => Step::Key {
                tui: name.to_owned(),
                key,
            },
            SingleAction::Type(text) => Step::Type {
                tui: name.to_owned(),
                text,
            },
        });
        lines.push(1);
    }
    steps.push(Step::Capture {
        tui: name.to_owned(),
        out: stem,
    });
    lines.push(1);

    let scenario = Scenario {
        run: RunConfig {
            timeout: options.timeout,
            settle: options.settle,
            artifacts: artifacts.clone(),
        },
        tuis: vec![TuiSpec {
            name: name.to_owned(),
            bin: options.bin,
            size: options.size,
            profile: options.profile,
            args: options.args,
            term: options.term,
            timeout: None,
        }],
        steps,
        tui_lines: vec![1],
        step_lines: lines,
    };
    scenario.validate()?;

    execute(
        &scenario,
        Path::new("<single shot>"),
        &cwd,
        &artifacts,
        false,
        options.json,
    )
}

fn execute(
    scenario: &Scenario,
    label: &Path,
    cwd: &Path,
    artifacts: &Path,
    write_run_report: bool,
    json: bool,
) -> Result<i32> {
    crate::signals::install();
    let started = Instant::now();
    let mut report = RunReport::new(label, artifacts);
    let mut live: BTreeMap<String, Tui> = BTreeMap::new();

    for spec in &scenario.tuis {
        let resolved = bin::resolve(&spec.bin, spec.profile, cwd)
            .with_context(|| format!("tui {}", spec.name))?;
        let tui = Tui::spawn(spec, &resolved, cwd)?;
        report.tuis.push(TuiRunInfo {
            name: spec.name.clone(),
            bin: spec.bin.clone(),
            path: resolved.info.path.clone(),
            profile: resolved.info.profile.clone(),
            pid: tui.pid,
            args: crate::report::redact_args(&spec.args),
            size: spec.size,
            exit: None,
        });
        live.insert(spec.name.clone(), tui);
    }

    let total = scenario.steps.len();
    let mut first_failure: Option<String> = None;

    for (index, (step, line)) in scenario.steps.iter().zip(&scenario.step_lines).enumerate() {
        eprintln!(
            "[{}/{}] {:<8} {:<10} {}",
            index + 1,
            total,
            step.action(),
            step.tui(),
            step.detail()
        );

        let step_started = Instant::now();
        let spec = scenario
            .tui(step.tui())
            .expect("validated: every step names a declared tui");
        let tui = live
            .get_mut(step.tui())
            .expect("every declared tui was spawned");

        let (outcome, stop) = match step {
            Step::Wait { waits, timeout, .. } => {
                let bound = timeout.unwrap_or_else(|| spec.effective_timeout(&scenario.run));
                match tui.wait(waits, bound, scenario.run.settle)? {
                    WaitOutcome::Met {
                        quiet_ms,
                        still_changing,
                        ..
                    } => (
                        if still_changing {
                            format!("met (still changing, quiet {quiet_ms} ms)")
                        } else {
                            format!("met ({quiet_ms} ms quiet)")
                        },
                        None,
                    ),
                    WaitOutcome::TimedOut { reason } | WaitOutcome::Exited { reason } => {
                        let stem = PathBuf::from(format!("{}-failure-{line}", tui.name));
                        let (capture, _) = write_capture(
                            tui,
                            artifacts,
                            &stem,
                            &format!("wait failed: {reason}"),
                            Duration::ZERO,
                            Duration::ZERO,
                        )?;
                        let message = format!(
                            "line {line}: wait {}: {reason} · screen written to {}",
                            waits
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join(" · "),
                            capture.text
                        );
                        report.captures.push(capture);
                        ("failed".to_owned(), Some(message))
                    }
                }
            }
            Step::Key { key, .. } => {
                if key.eq_ignore_ascii_case("esc") {
                    tui.send_escape()?;
                } else {
                    tui.send(&keys::encode(key)?)?;
                }
                ("sent".to_owned(), None)
            }
            Step::Type { text, .. } => {
                tui.send(text.as_bytes())?;
                ("typed".to_owned(), None)
            }
            Step::Resize { size, .. } => {
                tui.resize(*size)?;
                (format!("resized to {size}"), None)
            }
            Step::Capture { out, .. } => {
                let stem = capture_stem(out);
                let (capture, detail) = write_capture(
                    tui,
                    artifacts,
                    &stem,
                    "capture",
                    scenario.run.settle,
                    spec.effective_timeout(&scenario.run),
                )?;
                report.captures.push(capture);
                (detail.summary(), None)
            }
            Step::Assert { check, message, .. } => {
                let snapshot = tui.snapshot();
                let (ok, saw) = match check {
                    Assertion::Contains(text) => (
                        snapshot.contains(text),
                        if snapshot.contains(text) {
                            "present".to_owned()
                        } else {
                            "absent".to_owned()
                        },
                    ),
                    Assertion::Absent(text) => (
                        !snapshot.contains(text),
                        if snapshot.contains(text) {
                            "present".to_owned()
                        } else {
                            "absent".to_owned()
                        },
                    ),
                    Assertion::Size(size) => (snapshot.size == *size, snapshot.size.to_string()),
                };
                report.assertions.push(AssertionResult {
                    line: *line,
                    tui: tui.name.clone(),
                    check: match message {
                        Some(message) => format!("{check} · {message:?}"),
                        None => check.to_string(),
                    },
                    ok,
                    message: message.clone(),
                });
                if !ok && first_failure.is_none() {
                    first_failure = Some(format!("line {line}: assert {check} failed (saw {saw})"));
                }
                (
                    if ok {
                        "ok".to_owned()
                    } else {
                        "FAILED".to_owned()
                    },
                    None,
                )
            }
        };

        report.steps.push(StepResult {
            line: *line,
            action: step.action().to_owned(),
            tui: step.tui().to_owned(),
            detail: step.detail(),
            outcome,
            elapsed_ms: step_started.elapsed().as_millis() as u64,
        });

        if let Some(message) = stop {
            report.fail(message);
            break;
        }
    }

    for (name, mut tui) in live {
        tui.kill();
        if let Some(entry) = report.tuis.iter_mut().find(|entry| entry.name == name) {
            entry.exit = tui.exit().cloned();
        }
    }

    report.elapsed_ms = started.elapsed().as_millis() as u64;
    if report.status == RunStatus::Pass {
        if let Some(message) = first_failure {
            report.fail(message);
        }
    }

    if write_run_report {
        report.write(artifacts)?;
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", report.render_text());
    }

    Ok(match report.status {
        RunStatus::Pass => 0,
        RunStatus::Fail => 2,
    })
}

fn write_capture(
    tui: &mut Tui,
    artifacts: &Path,
    stem: &Path,
    reason: &str,
    settle: Duration,
    timeout: Duration,
) -> Result<(CaptureRef, CaptureReport)> {
    let CaptureOutcome {
        snapshot,
        quiet_ms,
        still_changing,
    } = tui.capture(settle, timeout);

    let reason = if reason == "capture" {
        if still_changing {
            format!("still changing (quiet {quiet_ms} ms)")
        } else {
            format!("settled ({quiet_ms} ms quiet)")
        }
    } else {
        reason.to_owned()
    };

    let rows = snapshot
        .row_cells
        .iter()
        .enumerate()
        .map(|(index, cells)| RowInfo {
            row: index as u16,
            cells: *cells,
            chars: snapshot.lines[index].chars().count(),
            full: *cells == snapshot.size.cols,
        })
        .collect::<Vec<_>>();

    let report = CaptureReport {
        capture: stem.to_string_lossy().replace('\\', "/"),
        program: tui.info.clone(),
        size: snapshot.size,
        captured_at: now_rfc3339(),
        reason,
        quiet_ms,
        observation: Observation {
            chunks: tui.chunks(),
            bytes: tui.bytes(),
            elapsed_ms: tui.elapsed_ms(),
        },
        screen: ScreenInfo {
            alternate: snapshot.alternate_screen,
            cursor: snapshot.cursor,
            rows_used: snapshot
                .lines
                .iter()
                .filter(|line| !line.is_empty())
                .count(),
            edge_rows: snapshot.edge_rows.clone(),
        },
        exit: tui.exit().cloned(),
        rows,
        styles: snapshot.styles.clone(),
        styles_truncated: snapshot.styles_truncated,
        warnings: snapshot.warnings.clone(),
    };

    let (text_path, json_path) = report.write(&artifacts.join(stem), &snapshot.text())?;
    let capture = CaptureRef {
        stem: report.capture.clone(),
        text: relative(artifacts, &text_path),
        json: relative(artifacts, &json_path),
        summary: format!(
            "{}{}",
            report.summary(),
            if still_changing {
                " · still changing"
            } else {
                ""
            }
        ),
    };
    Ok((capture, report))
}

fn working_directory() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("read the working directory")?;
    fs::canonicalize(&cwd).with_context(|| format!("resolve {}", cwd.display()))
}

/// Create the artifact directory and refuse one outside the repository. A relative path with a
/// `..` is rejected before anything is created, so a typo cannot make a directory elsewhere and
/// then be told about it.
fn prepare_artifacts(cwd: &Path, configured: &Path) -> Result<PathBuf> {
    if configured.as_os_str().is_empty() {
        bail!("artifacts must not be empty");
    }
    if configured
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        bail!(
            "artifacts {} contains `..`; name a directory inside the repository",
            configured.display()
        );
    }
    let absolute = if configured.is_absolute() {
        configured.to_path_buf()
    } else {
        cwd.join(configured)
    };
    if !absolute.starts_with(cwd) {
        bail!(
            "artifacts {} is outside {} · captures are written inside the repository",
            absolute.display(),
            cwd.display()
        );
    }
    fs::create_dir_all(&absolute).with_context(|| format!("create {}", absolute.display()))?;
    let canonical =
        fs::canonicalize(&absolute).with_context(|| format!("resolve {}", absolute.display()))?;
    if !canonical.starts_with(cwd) {
        bail!(
            "artifacts {} resolves outside {}",
            absolute.display(),
            cwd.display()
        );
    }
    Ok(canonical)
}

fn capture_stem(out: &Path) -> PathBuf {
    let text = out.to_string_lossy();
    let trimmed = text
        .strip_suffix(".txt")
        .or_else(|| text.strip_suffix(".json"))
        .unwrap_or(&text);
    PathBuf::from(trimmed)
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
