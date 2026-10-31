//! Running a scenario: decode once, run every step in order, and write the report.
//!
//! A step either succeeds, fails an assertion (exit 2) or hits an operational error (exit 1).
//! Assertion failures do not stop the run, so one report can name every check that did not hold.
//! An operational error does stop it, because the next step would act on an image that does not
//! exist.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context as _, Result};
use image::imageops;
use image::{DynamicImage, Rgba, RgbaImage};

use crate::artifact::{display, Artifacts};
use crate::dsl::{self, Action, AssertCheck, Compare, OutputSpec, Scenario, Step};
use crate::inspect::{self, Diff, Stats};
use crate::op::{Format, Op, Quality, Region, Rotation, Size};
use crate::source::Source;
use crate::transform::{self, Filter, RenderOptions};

pub fn run_cmd(
    args: &[String],
    cwd: &Path,
    artifacts_override: Option<&str>,
    max_pixels_override: Option<u64>,
) -> Result<i32> {
    let mut args = args.to_vec();
    let scenario_path = crate::take_value(&mut args, "--scenario")?
        .ok_or_else(|| anyhow!("run needs --scenario <file>"))?;
    let from_override = crate::take_value(&mut args, "--from")?;
    let json = crate::take_switch(&mut args, "--json");
    if !args.is_empty() {
        bail!("run: unexpected argument `{}`", args[0]);
    }

    let path = resolve(cwd, &scenario_path);
    let label = display(&path, cwd);
    let text = std::fs::read_to_string(&path).with_context(|| format!("read {label}"))?;
    let scenario = dsl::parse(&text)?;

    let artifacts_dir = artifacts_override
        .or(scenario.artifacts.as_deref())
        .unwrap_or(".tmp/image-shot");
    let artifacts_path = resolve(cwd, artifacts_dir);
    let artifacts = Artifacts::new(Some(artifacts_dir), cwd)?;
    let from = from_override
        .or(scenario.from.clone())
        .map(|value| resolve(cwd, &value))
        .unwrap_or_else(|| cwd.to_path_buf());
    let max_pixels = max_pixels_override
        .or(scenario.max_pixels)
        .unwrap_or(crate::DEFAULT_MAX_PIXELS);

    let (report, artifacts) = execute(
        &scenario,
        label,
        display(&artifacts_path, cwd),
        display(&from, cwd),
        max_pixels,
        artifacts,
        from.clone(),
    );

    let text_report = report.to_text();
    let json_report = report.to_json();
    let text_path = artifacts.resolve(Some("run.txt"), "run.txt")?;
    let json_path = artifacts.resolve(Some("run.json"), "run.json")?;
    std::fs::write(&text_path, &text_report)
        .with_context(|| format!("write {}", display(&text_path, cwd)))?;
    std::fs::write(&json_path, &json_report)
        .with_context(|| format!("write {}", display(&json_path, cwd)))?;

    if json {
        print!("{json_report}");
    } else {
        print!("{text_report}");
    }
    Ok(report.exit_code())
}

pub fn check_cmd(args: &[String], cwd: &Path) -> Result<i32> {
    let mut args = args.to_vec();
    let scenario_path = crate::take_value(&mut args, "--scenario")?
        .ok_or_else(|| anyhow!("check needs --scenario <file>"))?;
    if !args.is_empty() {
        bail!("check: unexpected argument `{}`", args[0]);
    }
    let path = resolve(cwd, &scenario_path);
    let label = display(&path, cwd);
    let text = std::fs::read_to_string(&path).with_context(|| format!("read {label}"))?;
    let scenario = dsl::parse(&text)?;
    println!("ok {} steps · {label}", scenario.steps.len());
    Ok(0)
}

pub fn fmt_cmd(args: &[String], cwd: &Path) -> Result<i32> {
    let mut args = args.to_vec();
    let scenario_path = crate::take_value(&mut args, "--scenario")?
        .ok_or_else(|| anyhow!("fmt needs --scenario <file>"))?;
    let write = crate::take_switch(&mut args, "--write");
    let check = crate::take_switch(&mut args, "--check");
    if write && check {
        bail!("fmt: --write and --check are mutually exclusive");
    }
    if !args.is_empty() {
        bail!("fmt: unexpected argument `{}`", args[0]);
    }

    let path = resolve(cwd, &scenario_path);
    let label = display(&path, cwd);
    let text = std::fs::read_to_string(&path).with_context(|| format!("read {label}"))?;
    let scenario = dsl::parse(&text)?;
    let formatted = scenario.format();
    if check {
        if formatted == text {
            Ok(0)
        } else {
            println!("would reformat {label}");
            Ok(2)
        }
    } else if write {
        std::fs::write(&path, formatted).with_context(|| format!("write {label}"))?;
        println!("wrote {label}");
        Ok(0)
    } else {
        print!("{formatted}");
        Ok(0)
    }
}

fn resolve(cwd: &Path, given: &str) -> PathBuf {
    if Path::new(given).is_absolute() {
        PathBuf::from(given)
    } else {
        cwd.join(given)
    }
}

struct Context {
    artifacts: Artifacts,
    from: PathBuf,
    max_pixels: u64,
    images: BTreeMap<String, DynamicImage>,
    paths: BTreeMap<String, PathBuf>,
    outputs: Vec<String>,
}

struct Outcome {
    status: StepStatus,
    image: Option<(u32, u32)>,
    notes: Vec<String>,
    stats: Option<Stats>,
    diff: Option<Diff>,
}

#[derive(Debug, Clone)]
enum StepStatus {
    Ok,
    Failed(String),
    Error(String),
}

impl StepStatus {
    fn failure(&self) -> Option<&str> {
        match self {
            StepStatus::Ok => None,
            StepStatus::Failed(reason) | StepStatus::Error(reason) => Some(reason),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            StepStatus::Ok => "ok",
            StepStatus::Failed(_) => "failed",
            StepStatus::Error(_) => "error",
        }
    }

    fn text(&self) -> &'static str {
        match self {
            StepStatus::Ok => "ok",
            StepStatus::Failed(_) => "FAIL",
            StepStatus::Error(_) => "ERROR",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunResult {
    Pass,
    Failed,
    Error,
}

impl RunResult {
    fn as_str(self) -> &'static str {
        match self {
            RunResult::Pass => "pass",
            RunResult::Failed => "failed",
            RunResult::Error => "error",
        }
    }
}

struct Failure {
    line: usize,
    action: String,
    message: String,
}

struct StepReport {
    line: usize,
    action: String,
    name: Option<String>,
    out: Option<String>,
    status: StepStatus,
    image: Option<(u32, u32)>,
    notes: Vec<String>,
    stats: Option<Stats>,
    diff: Option<Diff>,
    message: Option<String>,
}

struct RunReport {
    scenario: String,
    artifacts: String,
    from: String,
    max_pixels: u64,
    result: RunResult,
    steps: Vec<StepReport>,
    outputs: Vec<String>,
    failures: Vec<Failure>,
}

impl RunReport {
    fn exit_code(&self) -> i32 {
        match self.result {
            RunResult::Pass => 0,
            RunResult::Failed => 2,
            RunResult::Error => 1,
        }
    }

    fn to_text(&self) -> String {
        let mut out = String::new();
        writeln!(
            out,
            "# image-shot run · {} · {} steps",
            self.scenario,
            self.steps.len()
        )
        .unwrap();
        writeln!(
            out,
            "artifacts {} · from {} · max-pixels {}",
            self.artifacts, self.from, self.max_pixels
        )
        .unwrap();
        for (index, step) in self.steps.iter().enumerate() {
            write!(
                out,
                "[{}/{}] {} · {}",
                index + 1,
                self.steps.len(),
                step.action,
                step.status.text()
            )
            .unwrap();
            if let Some(name) = &step.name {
                write!(out, " · as {name}").unwrap();
            }
            if let Some(step_out) = &step.out {
                write!(out, " · out {step_out}").unwrap();
            }
            if let Some((width, height)) = step.image {
                write!(out, " · {width}x{height}").unwrap();
            }
            out.push('\n');
            if let Some(reason) = step.status.failure() {
                writeln!(out, "    {reason}").unwrap();
            }
            if let Some(message) = &step.message {
                writeln!(out, "    message {message}").unwrap();
            }
            for note in &step.notes {
                writeln!(out, "    {note}").unwrap();
            }
        }
        writeln!(
            out,
            "result {} · {} outputs",
            self.result.as_str(),
            self.outputs.len()
        )
        .unwrap();
        for failure in &self.failures {
            writeln!(
                out,
                "fail   line {} · {} · {}",
                failure.line, failure.action, failure.message
            )
            .unwrap();
        }
        out
    }

    fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        writeln!(out, "  \"version\": 1,").unwrap();
        writeln!(out, "  \"scenario\": {},", json_string(&self.scenario)).unwrap();
        writeln!(out, "  \"artifacts\": {},", json_string(&self.artifacts)).unwrap();
        writeln!(out, "  \"from\": {},", json_string(&self.from)).unwrap();
        writeln!(out, "  \"max_pixels\": {},", self.max_pixels).unwrap();
        writeln!(out, "  \"result\": {},", json_string(self.result.as_str())).unwrap();
        out.push_str("  \"steps\": [\n");
        for (index, step) in self.steps.iter().enumerate() {
            out.push_str("    {\n");
            writeln!(out, "      \"line\": {},", step.line).unwrap();
            writeln!(out, "      \"action\": {},", json_string(&step.action)).unwrap();
            writeln!(out, "      \"name\": {},", json_option(&step.name)).unwrap();
            writeln!(out, "      \"out\": {},", json_option(&step.out)).unwrap();
            writeln!(
                out,
                "      \"status\": {},",
                json_string(step.status.as_str())
            )
            .unwrap();
            writeln!(
                out,
                "      \"failure\": {},",
                json_optional_string(step.status.failure())
            )
            .unwrap();
            writeln!(
                out,
                "      \"message\": {},",
                json_optional_string(step.message.as_deref())
            )
            .unwrap();
            writeln!(
                out,
                "      \"image\": {},",
                step.image
                    .map(|(width, height)| json_string(&format!("{width}x{height}")))
                    .unwrap_or_else(|| "null".to_owned())
            )
            .unwrap();
            writeln!(out, "      \"notes\": {},", json_strings(&step.notes)).unwrap();
            writeln!(
                out,
                "      \"stats\": {},",
                step.stats
                    .as_ref()
                    .map(stats_json)
                    .unwrap_or_else(|| "null".to_owned())
            )
            .unwrap();
            writeln!(
                out,
                "      \"diff\": {}",
                step.diff
                    .as_ref()
                    .map(diff_json)
                    .unwrap_or_else(|| "null".to_owned())
            )
            .unwrap();
            out.push_str(if index + 1 == self.steps.len() {
                "    }\n"
            } else {
                "    },\n"
            });
        }
        out.push_str("  ],\n");
        writeln!(out, "  \"outputs\": {},", json_strings(&self.outputs)).unwrap();
        out.push_str("  \"failures\": [\n");
        for (index, failure) in self.failures.iter().enumerate() {
            write!(
                out,
                "    {{\"line\": {}, \"action\": {}, \"message\": {}}}",
                failure.line,
                json_string(&failure.action),
                json_string(&failure.message)
            )
            .unwrap();
            out.push_str(if index + 1 == self.failures.len() {
                "\n"
            } else {
                ",\n"
            });
        }
        out.push_str("  ]\n}\n");
        out
    }
}

fn execute(
    scenario: &Scenario,
    scenario_label: String,
    artifacts_label: String,
    from_label: String,
    max_pixels: u64,
    artifacts: Artifacts,
    from: PathBuf,
) -> (RunReport, Artifacts) {
    let mut context = Context {
        artifacts,
        from,
        max_pixels,
        images: BTreeMap::new(),
        paths: BTreeMap::new(),
        outputs: Vec::new(),
    };
    let mut report = RunReport {
        scenario: scenario_label,
        artifacts: artifacts_label,
        from: from_label,
        max_pixels,
        result: RunResult::Pass,
        steps: Vec::new(),
        outputs: Vec::new(),
        failures: Vec::new(),
    };

    for (index, step) in scenario.steps.iter().enumerate() {
        eprintln!(
            "[{}/{}] {}",
            index + 1,
            scenario.steps.len(),
            step.action.describe()
        );
        match execute_step(step, &mut context) {
            Ok(outcome) => {
                if let Some(reason) = outcome.status.failure() {
                    report.failures.push(Failure {
                        line: step.line,
                        action: step.action.label(),
                        message: reason.to_owned(),
                    });
                }
                report.steps.push(step_report(step, outcome));
            }
            Err(error) => {
                report.result = RunResult::Error;
                report.steps.push(StepReport {
                    line: step.line,
                    action: step.action.label(),
                    name: step.action.defined_name().map(str::to_owned),
                    out: step.action.out().map(str::to_owned),
                    status: StepStatus::Error(format!("{error:#}")),
                    image: None,
                    notes: Vec::new(),
                    stats: None,
                    diff: None,
                    message: match &step.action {
                        Action::Assert { message, .. } => message.clone(),
                        _ => None,
                    },
                });
                break;
            }
        }
    }

    if report.result != RunResult::Error {
        report.result = if report.failures.is_empty() {
            RunResult::Pass
        } else {
            RunResult::Failed
        };
    }
    report.outputs = context.outputs;
    (report, context.artifacts)
}

fn step_report(step: &Step, outcome: Outcome) -> StepReport {
    StepReport {
        line: step.line,
        action: step.action.label(),
        name: step.action.defined_name().map(str::to_owned),
        out: step.action.out().map(str::to_owned),
        status: outcome.status,
        image: outcome.image,
        notes: outcome.notes,
        stats: outcome.stats,
        diff: outcome.diff,
        message: match &step.action {
            Action::Assert { message, .. } => message.clone(),
            _ => None,
        },
    }
}

fn execute_step(step: &Step, context: &mut Context) -> Result<Outcome> {
    match &step.action {
        Action::Source { name, from } => {
            let source = Source::read(from, &context.from, context.max_pixels)?;
            let image = source.decode()?;
            let (width, height) = (image.width(), image.height());
            let notes = vec![format!(
                "{} · {} · {}x{} · {}",
                source.label,
                source.format,
                width,
                height,
                inspect::bytes_size(source.bytes.len() as u64)
            )];
            context.paths.insert(name.clone(), source.path.clone());
            context.images.insert(name.clone(), image);
            Ok(Outcome {
                status: StepStatus::Ok,
                image: Some((width, height)),
                notes,
                stats: None,
                diff: None,
            })
        }
        Action::Render { source, op, output } => {
            let image = get(context, source)?.clone();
            let filter = output.filter.unwrap_or(Filter::Lanczos3);
            render_image(image, op, output, filter, context)
        }
        Action::Fit {
            source,
            edge,
            upscale,
            output,
        } => {
            let image = get(context, source)?.clone();
            let (width, height) = (image.width(), image.height());
            let format = output.format.unwrap_or(Format::Png);
            let (target_width, target_height) = crate::fit_dimensions(width, height, *edge);
            let size = if !upscale && *edge >= width.max(height) {
                Size::Exact {
                    w: width,
                    h: height,
                }
            } else if width >= height {
                Size::Width(target_width)
            } else {
                Size::Height(target_height)
            };
            let op = Op {
                region: Region::Full,
                size,
                rotation: Rotation {
                    mirror: false,
                    degrees: 0.0,
                },
                quality: Quality::Default,
                format,
            };
            let mut outcome = render_image(
                image,
                &op,
                output,
                output.filter.unwrap_or(Filter::Lanczos3),
                context,
            )?;
            if !upscale && *edge >= width.max(height) {
                outcome.notes.insert(0, format!("already within {edge}px"));
            }
            Ok(outcome)
        }
        Action::Crop {
            source,
            x,
            y,
            width,
            height,
            zoom,
            smooth,
            output,
        } => {
            let image = get(context, source)?.clone();
            let (source_width, source_height) = (image.width(), image.height());
            if *x >= source_width || *y >= source_height {
                bail!("--box: {x},{y} starts outside the {source_width}x{source_height} source");
            }
            if *width > source_width - x || *height > source_height - y {
                bail!(
                    "--box: {width}x{height} at {x},{y} extends past the {source_width}x{source_height} source"
                );
            }
            let output_width = width
                .checked_mul(*zoom)
                .ok_or_else(|| anyhow!("zoom: the output dimensions overflow"))?;
            let output_height = height
                .checked_mul(*zoom)
                .ok_or_else(|| anyhow!("zoom: the output dimensions overflow"))?;
            let filter = output.filter.unwrap_or(if *zoom > 1 && !smooth {
                Filter::Nearest
            } else {
                Filter::Lanczos3
            });
            let op = Op {
                region: Region::Absolute {
                    x: *x,
                    y: *y,
                    w: *width,
                    h: *height,
                },
                size: Size::Exact {
                    w: output_width,
                    h: output_height,
                },
                rotation: Rotation {
                    mirror: false,
                    degrees: 0.0,
                },
                quality: Quality::Default,
                format: output.format.unwrap_or(Format::Png),
            };
            render_image(image, &op, output, filter, context)
        }
        Action::Cover {
            source,
            width,
            height,
            upscale,
            output,
        } => {
            let image = get(context, source)?.clone();
            let (source_width, source_height) = (image.width(), image.height());
            let (x, y, region_width, region_height) =
                crate::cover_region(source_width, source_height, *width, *height);
            if !upscale && (*width > region_width || *height > region_height) {
                bail!(
                    "cover: {region_width}x{region_height} would be upscaled to {width}x{height}; \
                     pass upscale if that is intended"
                );
            }
            let op = Op {
                region: Region::Absolute {
                    x,
                    y,
                    w: region_width,
                    h: region_height,
                },
                size: Size::Exact {
                    w: *width,
                    h: *height,
                },
                rotation: Rotation {
                    mirror: false,
                    degrees: 0.0,
                },
                quality: Quality::Default,
                format: output.format.unwrap_or(Format::Png),
            };
            render_image(
                image,
                &op,
                output,
                output.filter.unwrap_or(Filter::Lanczos3),
                context,
            )
        }
        Action::Square {
            source,
            size,
            upscale,
            output,
        } => {
            let image = get(context, source)?.clone();
            let shorter = image.width().min(image.height());
            if !upscale && *size > shorter {
                bail!(
                    "square: the source's shorter side is {shorter}px; {size}px would upscale it. \
                     Pass upscale if that is intended"
                );
            }
            let op = Op {
                region: Region::Square,
                size: Size::Width(*size),
                rotation: Rotation {
                    mirror: false,
                    degrees: 0.0,
                },
                quality: Quality::Default,
                format: output.format.unwrap_or(Format::Png),
            };
            render_image(
                image,
                &op,
                output,
                output.filter.unwrap_or(Filter::Lanczos3),
                context,
            )
        }
        Action::Trim {
            source,
            tolerance,
            output,
        } => {
            let image = get(context, source)?.clone();
            let (trimmed, changed) = trim_image(&image, *tolerance);
            let notes = vec![if changed {
                format!(
                    "trimmed {tolerance}-tolerance border · {}x{}",
                    trimmed.width(),
                    trimmed.height()
                )
            } else {
                "nothing to trim".to_owned()
            }];
            finish_image(trimmed, output, context, notes)
        }
        Action::Sheet {
            from,
            cols,
            cell_width,
            cell_height,
            output,
        } => {
            let images = from
                .iter()
                .map(|name| get(context, name).cloned())
                .collect::<Result<Vec<_>>>()?;
            let rows = (images.len() as u32).div_ceil(*cols);
            let canvas_width = cols
                .checked_mul(*cell_width)
                .ok_or_else(|| anyhow!("sheet: the canvas width overflows"))?;
            let canvas_height = rows
                .checked_mul(*cell_height)
                .ok_or_else(|| anyhow!("sheet: the canvas height overflows"))?;
            let pixels = u64::from(canvas_width) * u64::from(canvas_height);
            if pixels > context.max_pixels {
                bail!(
                    "sheet: {canvas_width}x{canvas_height} is above the {} pixel cap",
                    context.max_pixels
                );
            }
            let mut canvas = RgbaImage::new(canvas_width, canvas_height);
            if let Some(background) = output.background {
                for pixel in canvas.pixels_mut() {
                    *pixel = Rgba([background[0], background[1], background[2], 255]);
                }
            }
            let filter = output.filter.unwrap_or(Filter::Lanczos3);
            for (index, image) in images.iter().enumerate() {
                let column = index as u32 % cols;
                let row = index as u32 / cols;
                let (thumb_width, thumb_height) =
                    fit_cell(image.width(), image.height(), *cell_width, *cell_height);
                let thumb = image.resize_exact(thumb_width, thumb_height, filter.image());
                let x = column * cell_width + (*cell_width - thumb_width) / 2;
                let y = row * cell_height + (*cell_height - thumb_height) / 2;
                imageops::overlay(&mut canvas, &thumb.to_rgba8(), i64::from(x), i64::from(y));
            }
            let notes = vec![
                format!(
                    "{} images · {cols} columns · {}x{} cells",
                    images.len(),
                    cell_width,
                    cell_height
                ),
                format!("filter {}", filter.as_str()),
            ];
            finish_image(DynamicImage::ImageRgba8(canvas), output, context, notes)
        }
        Action::Overlay {
            base,
            layer,
            x,
            y,
            output,
        } => {
            let layer_name = layer.clone();
            let mut canvas = get(context, base)?.to_rgba8();
            let layer = get(context, layer)?.to_rgba8();
            imageops::overlay(&mut canvas, &layer, *x, *y);
            let notes = vec![format!("layer {layer_name} at {x},{y}")];
            finish_image(DynamicImage::ImageRgba8(canvas), output, context, notes)
        }
        Action::Stats { source } => {
            let stats = inspect::stats_image(get(context, source)?);
            let notes = vec![format!(
                "mean {:.1} {:.1} {:.1} {:.1} · alpha {:.1}% · colours {}{}",
                stats.mean[0],
                stats.mean[1],
                stats.mean[2],
                stats.mean[3],
                stats.alpha_fraction,
                stats.colours,
                if stats.colours_capped {
                    " (capped)"
                } else {
                    ""
                }
            )];
            Ok(Outcome {
                status: StepStatus::Ok,
                image: Some((stats.width, stats.height)),
                notes,
                stats: Some(stats),
                diff: None,
            })
        }
        Action::Diff {
            a,
            b,
            out,
            tolerance,
        } => {
            let left = get(context, a)?;
            let right = get(context, b)?;
            let result = inspect::diff(left, right, *tolerance)?;
            let mut notes = vec![format!(
                "{} px differ ({:.3}%) · max Δ {} · mean Δ {:.2}",
                result.changed,
                result.fraction(),
                result.max_delta,
                result.mean_delta
            )];
            if let Some(out) = out {
                let view = inspect::diff_view(left, right, *tolerance)?;
                write_output(context, out, &view)?;
                notes.push(format!("out {out}"));
            }
            Ok(Outcome {
                status: StepStatus::Ok,
                image: None,
                notes,
                stats: None,
                diff: Some(result),
            })
        }
        Action::Assert { check, .. } => {
            let failure = evaluate(check, context)?;
            Ok(Outcome {
                status: failure.map(StepStatus::Failed).unwrap_or(StepStatus::Ok),
                image: None,
                notes: Vec::new(),
                stats: None,
                diff: None,
            })
        }
    }
}

fn get<'a>(context: &'a Context, name: &str) -> Result<&'a DynamicImage> {
    context
        .images
        .get(name)
        .ok_or_else(|| anyhow!("`{name}` is not available"))
}

fn render_image(
    image: DynamicImage,
    op: &Op,
    output: &OutputSpec,
    filter: Filter,
    context: &mut Context,
) -> Result<Outcome> {
    let options = RenderOptions {
        filter,
        compression: output.compression.unwrap_or(85),
        background: output.background,
    };
    let rendered = transform::render(
        image,
        op,
        &options,
        crate::MAX_DIMENSION,
        context.max_pixels,
    )?;
    let mut notes = rendered.notes;
    notes.push(format!("filter {}", rendered.filter.as_str()));
    let (width, height) = (rendered.width, rendered.height);
    if let Some(out) = &output.out {
        write_output(context, out, &rendered.bytes)?;
        notes.push(format!("out {out}"));
    }
    if let Some(name) = &output.as_name {
        context.images.insert(name.clone(), rendered.image);
        notes.push(format!("as {name}"));
    }
    Ok(Outcome {
        status: StepStatus::Ok,
        image: Some((width, height)),
        notes,
        stats: None,
        diff: None,
    })
}

fn finish_image(
    image: DynamicImage,
    output: &OutputSpec,
    context: &mut Context,
    mut notes: Vec<String>,
) -> Result<Outcome> {
    let format = output.format.unwrap_or(Format::Png).resolve();
    let options = RenderOptions {
        filter: output.filter.unwrap_or(Filter::Lanczos3),
        compression: output.compression.unwrap_or(85),
        background: output.background,
    };
    let (width, height) = (image.width(), image.height());
    let bytes = transform::encode(&image, format, &options, &mut notes)?;
    if let Some(out) = &output.out {
        write_output(context, out, &bytes)?;
        notes.push(format!("out {out}"));
    }
    if let Some(name) = &output.as_name {
        context.images.insert(name.clone(), image);
        notes.push(format!("as {name}"));
    }
    Ok(Outcome {
        status: StepStatus::Ok,
        image: Some((width, height)),
        notes,
        stats: None,
        diff: None,
    })
}

fn write_output(context: &mut Context, out: &str, bytes: &[u8]) -> Result<()> {
    if out == "run.txt" || out == "run.json" {
        bail!("out {out} is reserved for the run report");
    }
    let path = context.artifacts.resolve(Some(out), out)?;
    if context.paths.values().any(|source| source == &path) {
        bail!("out {out} names an input; a derived view is a new file");
    }
    std::fs::write(&path, bytes).with_context(|| format!("write {out}"))?;
    context.outputs.push(out.to_owned());
    Ok(())
}

fn fit_cell(width: u32, height: u32, cell_width: u32, cell_height: u32) -> (u32, u32) {
    let scale = (f64::from(cell_width) / f64::from(width.max(1)))
        .min(f64::from(cell_height) / f64::from(height.max(1)))
        .min(1.0);
    (
        (f64::from(width) * scale).round().max(1.0) as u32,
        (f64::from(height) * scale).round().max(1.0) as u32,
    )
}

fn trim_image(image: &DynamicImage, tolerance: u8) -> (DynamicImage, bool) {
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let background = rgba.get_pixel(0, 0).0;
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0;
    let mut max_y = 0;
    let mut found = false;
    for (x, y, pixel) in rgba.enumerate_pixels() {
        let delta = (0..4)
            .map(|channel| pixel[channel].abs_diff(background[channel]))
            .max()
            .unwrap_or(0);
        if delta > tolerance {
            found = true;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    if !found {
        return (image.clone(), false);
    }
    (
        image.crop_imm(min_x, min_y, max_x - min_x + 1, max_y - min_y + 1),
        true,
    )
}

fn evaluate(check: &AssertCheck, context: &Context) -> Result<Option<String>> {
    match check {
        AssertCheck::Size {
            name,
            width,
            height,
        } => {
            let image = get(context, name)?;
            if (image.width(), image.height()) == (*width, *height) {
                Ok(None)
            } else {
                Ok(Some(format!(
                    "size is {}x{}, expected {width}x{height}",
                    image.width(),
                    image.height()
                )))
            }
        }
        AssertCheck::Alpha {
            name,
            compare,
            value,
        } => {
            let stats = inspect::stats_image(get(context, name)?);
            if compare.matches(stats.alpha_fraction, *value) {
                Ok(None)
            } else {
                Ok(Some(format!(
                    "alpha is {:.3}%, expected {} {value}%",
                    stats.alpha_fraction,
                    compare_key(*compare)
                )))
            }
        }
        AssertCheck::Same {
            a,
            b,
            max,
            tolerance,
        } => {
            let result = match inspect::diff(get(context, a)?, get(context, b)?, *tolerance) {
                Ok(result) => result,
                Err(error) => return Ok(Some(format!("{error:#}"))),
            };
            let fraction = result.changed as f64 / result.total.max(1) as f64;
            if fraction <= *max {
                Ok(None)
            } else {
                Ok(Some(format!(
                    "changed {:.3}% of pixels, expected at most {:.3}%",
                    fraction * 100.0,
                    max * 100.0
                )))
            }
        }
        AssertCheck::Different {
            a,
            b,
            min,
            tolerance,
        } => {
            let result = match inspect::diff(get(context, a)?, get(context, b)?, *tolerance) {
                Ok(result) => result,
                Err(error) => return Ok(Some(format!("{error:#}"))),
            };
            let fraction = result.changed as f64 / result.total.max(1) as f64;
            if fraction >= *min {
                Ok(None)
            } else {
                Ok(Some(format!(
                    "changed {:.3}% of pixels, expected at least {:.3}%",
                    fraction * 100.0,
                    min * 100.0
                )))
            }
        }
        AssertCheck::Colours {
            name,
            compare,
            value,
        } => {
            let stats = inspect::stats_image(get(context, name)?);
            let actual = stats.colours as f64;
            if compare.matches(actual, *value as f64) {
                Ok(None)
            } else {
                Ok(Some(format!(
                    "colours is {}{}, expected {} {value}",
                    stats.colours,
                    if stats.colours_capped {
                        " (capped)"
                    } else {
                        ""
                    },
                    compare_key(*compare)
                )))
            }
        }
    }
}

fn compare_key(compare: Compare) -> &'static str {
    match compare {
        Compare::Greater => "greater than",
        Compare::Less => "less than",
        Compare::Equal => "equal to",
    }
}

fn stats_json(stats: &Stats) -> String {
    let ink = stats
        .ink
        .map(|(x0, y0, x1, y1)| format!("[{x0}, {y0}, {x1}, {y1}]"))
        .unwrap_or_else(|| "null".to_owned());
    format!(
        "{{\"mean\": [{:.4}, {:.4}, {:.4}, {:.4}], \"sd\": [{:.4}, {:.4}, {:.4}, {:.4}], \
         \"alpha_percent\": {:.4}, \"colours\": {}, \"colours_capped\": {}, \"ink\": {ink}, \
         \"sharpness\": {:.4}}}",
        stats.mean[0],
        stats.mean[1],
        stats.mean[2],
        stats.mean[3],
        stats.sd[0],
        stats.sd[1],
        stats.sd[2],
        stats.sd[3],
        stats.alpha_fraction,
        stats.colours,
        stats.colours_capped,
        stats.sharpness
    )
}

fn diff_json(diff: &Diff) -> String {
    let bbox = diff
        .bbox
        .map(|(x0, y0, x1, y1)| format!("[{x0}, {y0}, {x1}, {y1}]"))
        .unwrap_or_else(|| "null".to_owned());
    format!(
        "{{\"changed\": {}, \"total\": {}, \"fraction\": {:.6}, \"max_delta\": {}, \
         \"mean_delta\": {:.4}, \"bbox\": {bbox}}}",
        diff.changed,
        diff.total,
        diff.changed as f64 / diff.total.max(1) as f64,
        diff.max_delta,
        diff.mean_delta
    )
}

fn json_option(value: &Option<String>) -> String {
    json_optional_string(value.as_deref())
}

fn json_optional_string(value: Option<&str>) -> String {
    value.map(json_string).unwrap_or_else(|| "null".to_owned())
}

fn json_strings(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| json_string(value))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => write!(out, "\\u{:04x}", c as u32).unwrap(),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::parse;
    use image::{Rgba, RgbaImage};

    fn workspace(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("image-shot-scenario-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fixture(dir: &Path, name: &str, width: u32, height: u32) -> PathBuf {
        let mut image = RgbaImage::new(width, height);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = Rgba([
                20_u8.wrapping_add((x * 7) as u8),
                40_u8.wrapping_add((y * 11) as u8),
                80,
                255,
            ]);
        }
        let path = dir.join(name);
        image.save(&path).unwrap();
        path
    }

    fn run_scenario(source: &Path, text: &str) -> (RunReport, PathBuf) {
        let dir = source.parent().unwrap();
        let artifacts = dir.join("artifacts");
        let scenario = parse(text).unwrap();
        let (report, _) = execute(
            &scenario,
            "scenario.dsl".to_owned(),
            "artifacts".to_owned(),
            ".".to_owned(),
            64_000_000,
            Artifacts::new(Some("artifacts"), dir).unwrap(),
            dir.to_path_buf(),
        );
        (report, artifacts)
    }

    #[test]
    fn runs_a_complete_pipeline() {
        let dir = workspace("pipeline");
        fixture(&dir, "source.png", 40, 30);
        let text = r#"
run artifacts=artifacts
source home from=source.png
crop home box=0,0,20,10 as=top out=crops/top.png
fit top edge=10 as=small out=crops/small.png
sheet cols=2 cell=12x12 from=top,small out=sheets/both.png
overlay home layer=top x=5 y=5 as=marked out=out/marked.png
trim home as=trimmed out=out/trimmed.png
stats home
diff home home out=diffs/same.png
assert size top eq=20x10
assert same home home max=0
assert different home marked min=0.001
"#;
        let (report, artifacts) = run_scenario(&dir.join("source.png"), text);
        assert_eq!(report.result, RunResult::Pass, "{}", report.to_text());
        assert!(artifacts.join("crops/top.png").exists());
        assert!(artifacts.join("sheets/both.png").exists());
        assert!(artifacts.join("out/marked.png").exists());
        assert!(artifacts.join("diffs/same.png").exists());
        assert!(report.outputs.len() >= 6);
    }

    #[test]
    fn an_assertion_failure_sets_exit_two_and_keeps_going() {
        let dir = workspace("assert");
        fixture(&dir, "source.png", 12, 8);
        let text = r#"
run artifacts=artifacts
source home from=source.png
assert size home eq=99x99 message="wrong size"
stats home
"#;
        let (report, artifacts) = run_scenario(&dir.join("source.png"), text);
        assert_eq!(report.result, RunResult::Failed);
        assert_eq!(report.exit_code(), 2);
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.steps.len(), 3);
        assert!(report.to_text().contains("wrong size"));
        let _ = artifacts;
    }

    #[test]
    fn a_missing_source_is_an_operational_error() {
        let dir = workspace("missing");
        fixture(&dir, "source.png", 4, 4);
        let text = "run artifacts=artifacts\nsource home from=missing.png\n";
        let (report, _) = run_scenario(&dir.join("source.png"), text);
        assert_eq!(report.result, RunResult::Error);
        assert_eq!(report.exit_code(), 1);
        assert!(report.steps[0]
            .status
            .failure()
            .unwrap()
            .contains("missing.png"));
    }

    #[test]
    fn out_may_not_name_a_source() {
        let dir = workspace("overwrite");
        fixture(&dir, "source.png", 8, 6);
        let text =
            "run artifacts=.\nsource home from=source.png\ncrop home box=0,0,2,2 out=source.png\n";
        let scenario = parse(text).unwrap();
        let (report, _) = execute(
            &scenario,
            "scenario.dsl".to_owned(),
            ".".to_owned(),
            ".".to_owned(),
            64_000_000,
            Artifacts::new(Some("."), &dir).unwrap(),
            dir.clone(),
        );
        assert_eq!(report.result, RunResult::Error);
        assert!(report
            .steps
            .last()
            .unwrap()
            .status
            .failure()
            .unwrap()
            .contains("names an input"));
    }

    #[test]
    fn json_escapes_a_quoted_message() {
        let value = json_string("a \"b\"\n");
        assert_eq!(value, "\"a \\\"b\\\"\\n\"");
    }
}
