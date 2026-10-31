//! The scenario grammar: one action per line, parsed in full before anything is read.
//!
//! A scenario is data, not a script. There is no shell, no expression language and no network:
//! every action names images and writes under the artifacts directory. The grammar is deliberately
//! close to `browser-shot` and `tui-shot` so a reader moves between them without learning a second
//! shape.

use std::collections::HashSet;
use std::fmt::Write as _;

use anyhow::{anyhow, bail, Result};

use crate::op::{Format, Op};
use crate::transform::Filter;

#[derive(Debug, Clone, PartialEq)]
pub struct Scenario {
    pub artifacts: Option<String>,
    pub from: Option<String>,
    pub max_pixels: Option<u64>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone)]
pub struct Step {
    pub line: usize,
    pub action: Action,
}

impl PartialEq for Step {
    fn eq(&self, other: &Self) -> bool {
        self.action == other.action
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Source {
        name: String,
        from: String,
    },
    Render {
        source: String,
        op: Op,
        output: OutputSpec,
    },
    Fit {
        source: String,
        edge: u32,
        upscale: bool,
        output: OutputSpec,
    },
    Crop {
        source: String,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        zoom: u32,
        smooth: bool,
        output: OutputSpec,
    },
    Cover {
        source: String,
        width: u32,
        height: u32,
        upscale: bool,
        output: OutputSpec,
    },
    Square {
        source: String,
        size: u32,
        upscale: bool,
        output: OutputSpec,
    },
    Trim {
        source: String,
        tolerance: u8,
        output: OutputSpec,
    },
    Sheet {
        from: Vec<String>,
        cols: u32,
        cell_width: u32,
        cell_height: u32,
        output: OutputSpec,
    },
    Overlay {
        base: String,
        layer: String,
        x: i64,
        y: i64,
        output: OutputSpec,
    },
    Stats {
        source: String,
    },
    Diff {
        a: String,
        b: String,
        out: Option<String>,
        tolerance: u8,
    },
    Assert {
        check: AssertCheck,
        message: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct OutputSpec {
    pub as_name: Option<String>,
    pub out: Option<String>,
    pub format: Option<Format>,
    pub filter: Option<Filter>,
    pub compression: Option<u8>,
    pub background: Option<[u8; 3]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compare {
    Greater,
    Less,
    Equal,
}

impl Compare {
    pub(crate) fn matches(self, value: f64, target: f64) -> bool {
        match self {
            Compare::Greater => value > target,
            Compare::Less => value < target,
            Compare::Equal => (value - target).abs() < f64::EPSILON,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AssertCheck {
    Size {
        name: String,
        width: u32,
        height: u32,
    },
    Alpha {
        name: String,
        compare: Compare,
        value: f64,
    },
    Same {
        a: String,
        b: String,
        max: f64,
        tolerance: u8,
    },
    Different {
        a: String,
        b: String,
        min: f64,
        tolerance: u8,
    },
    Colours {
        name: String,
        compare: Compare,
        value: u64,
    },
}

impl AssertCheck {
    pub fn kind(&self) -> &'static str {
        match self {
            AssertCheck::Size { .. } => "size",
            AssertCheck::Alpha { .. } => "alpha",
            AssertCheck::Same { .. } => "same",
            AssertCheck::Different { .. } => "different",
            AssertCheck::Colours { .. } => "colours",
        }
    }

    fn describe(&self) -> String {
        match self {
            AssertCheck::Size { name, .. }
            | AssertCheck::Alpha { name, .. }
            | AssertCheck::Colours { name, .. } => format!("{} {name}", self.kind()),
            AssertCheck::Same { a, b, .. } | AssertCheck::Different { a, b, .. } => {
                format!("{} {a} {b}", self.kind())
            }
        }
    }
}

impl Action {
    pub fn kind(&self) -> &'static str {
        match self {
            Action::Source { .. } => "source",
            Action::Render { .. } => "render",
            Action::Fit { .. } => "fit",
            Action::Crop { .. } => "crop",
            Action::Cover { .. } => "cover",
            Action::Square { .. } => "square",
            Action::Trim { .. } => "trim",
            Action::Sheet { .. } => "sheet",
            Action::Overlay { .. } => "overlay",
            Action::Stats { .. } => "stats",
            Action::Diff { .. } => "diff",
            Action::Assert { .. } => "assert",
        }
    }

    pub fn label(&self) -> String {
        match self {
            Action::Assert { check, .. } => format!("assert {}", check.kind()),
            other => other.kind().to_owned(),
        }
    }

    pub fn defined_name(&self) -> Option<&str> {
        match self {
            Action::Source { name, .. } => Some(name),
            Action::Render { output, .. }
            | Action::Fit { output, .. }
            | Action::Crop { output, .. }
            | Action::Cover { output, .. }
            | Action::Square { output, .. }
            | Action::Trim { output, .. }
            | Action::Sheet { output, .. }
            | Action::Overlay { output, .. } => output.as_name.as_deref(),
            Action::Stats { .. } | Action::Diff { .. } | Action::Assert { .. } => None,
        }
    }

    fn references(&self) -> Vec<&str> {
        match self {
            Action::Source { .. } => Vec::new(),
            Action::Render { source, .. }
            | Action::Fit { source, .. }
            | Action::Crop { source, .. }
            | Action::Cover { source, .. }
            | Action::Square { source, .. }
            | Action::Trim { source, .. }
            | Action::Stats { source } => vec![source],
            Action::Sheet { from, .. } => from.iter().map(String::as_str).collect(),
            Action::Overlay { base, layer, .. } => vec![base, layer],
            Action::Diff { a, b, .. } => vec![a, b],
            Action::Assert { check, .. } => match check {
                AssertCheck::Size { name, .. }
                | AssertCheck::Alpha { name, .. }
                | AssertCheck::Colours { name, .. } => vec![name],
                AssertCheck::Same { a, b, .. } | AssertCheck::Different { a, b, .. } => vec![a, b],
            },
        }
    }

    pub fn out(&self) -> Option<&str> {
        match self {
            Action::Render { output, .. }
            | Action::Fit { output, .. }
            | Action::Crop { output, .. }
            | Action::Cover { output, .. }
            | Action::Square { output, .. }
            | Action::Trim { output, .. }
            | Action::Sheet { output, .. }
            | Action::Overlay { output, .. } => output.out.as_deref(),
            Action::Diff { out, .. } => out.as_deref(),
            Action::Source { .. } | Action::Stats { .. } | Action::Assert { .. } => None,
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Action::Source { name, from } => format!("source {name} from={from}"),
            Action::Render { source, .. } => format!("render {source}"),
            Action::Fit { source, .. } => format!("fit {source}"),
            Action::Crop { source, .. } => format!("crop {source}"),
            Action::Cover { source, .. } => format!("cover {source}"),
            Action::Square { source, .. } => format!("square {source}"),
            Action::Trim { source, .. } => format!("trim {source}"),
            Action::Sheet { from, .. } => format!("sheet {}", from.join(",")),
            Action::Overlay { base, layer, .. } => format!("overlay {base} + {layer}"),
            Action::Stats { source } => format!("stats {source}"),
            Action::Diff { a, b, .. } => format!("diff {a} {b}"),
            Action::Assert { check, .. } => format!("assert {}", check.describe()),
        }
    }
}

impl Scenario {
    pub fn format(&self) -> String {
        let mut out = String::new();
        out.push_str("run");
        if let Some(value) = &self.artifacts {
            write!(out, " artifacts={}", quote(value)).unwrap();
        }
        if let Some(value) = &self.from {
            write!(out, " from={}", quote(value)).unwrap();
        }
        if let Some(value) = self.max_pixels {
            write!(out, " max-pixels={value}").unwrap();
        }
        out.push('\n');
        for step in &self.steps {
            out.push_str(&step.format());
            out.push('\n');
        }
        out
    }
}

impl Step {
    fn format(&self) -> String {
        match &self.action {
            Action::Source { name, from } => format!("source {name} from={}", quote(from)),
            Action::Render { source, op, output } => format!(
                "render {source} op={}{}",
                quote(&op.canonical()),
                format_output(output, false)
            ),
            Action::Fit {
                source,
                edge,
                upscale,
                output,
            } => {
                let flag = if *upscale { " upscale" } else { "" };
                format!(
                    "fit {source} edge={edge}{flag}{}",
                    format_output(output, true)
                )
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
                let zoom = if *zoom == 1 {
                    String::new()
                } else {
                    format!(" zoom={zoom}")
                };
                let smooth = if *smooth { " smooth" } else { "" };
                format!(
                    "crop {source} box={x},{y},{width},{height}{zoom}{smooth}{}",
                    format_output(output, true)
                )
            }
            Action::Cover {
                source,
                width,
                height,
                upscale,
                output,
            } => {
                let flag = if *upscale { " upscale" } else { "" };
                format!(
                    "cover {source} size={width}x{height}{flag}{}",
                    format_output(output, true)
                )
            }
            Action::Square {
                source,
                size,
                upscale,
                output,
            } => {
                let flag = if *upscale { " upscale" } else { "" };
                format!(
                    "square {source} size={size}{flag}{}",
                    format_output(output, true)
                )
            }
            Action::Trim {
                source,
                tolerance,
                output,
            } => {
                let tolerance = if *tolerance == 8 {
                    String::new()
                } else {
                    format!(" tolerance={tolerance}")
                };
                format!("trim {source}{tolerance}{}", format_output(output, true))
            }
            Action::Sheet {
                from,
                cols,
                cell_width,
                cell_height,
                output,
            } => format!(
                "sheet cols={cols} cell={cell_width}x{cell_height} from={}{}",
                quote(&from.join(",")),
                format_output(output, true)
            ),
            Action::Overlay {
                base,
                layer,
                x,
                y,
                output,
            } => {
                let x = if *x == 0 {
                    String::new()
                } else {
                    format!(" x={x}")
                };
                let y = if *y == 0 {
                    String::new()
                } else {
                    format!(" y={y}")
                };
                format!(
                    "overlay {base} layer={layer}{x}{y}{}",
                    format_output(output, true)
                )
            }
            Action::Stats { source } => format!("stats {source}"),
            Action::Diff {
                a,
                b,
                out,
                tolerance,
            } => {
                let out = out
                    .as_ref()
                    .map(|value| format!(" out={}", quote(value)))
                    .unwrap_or_default();
                let tolerance = if *tolerance == 0 {
                    String::new()
                } else {
                    format!(" tolerance={tolerance}")
                };
                format!("diff {a} {b}{out}{tolerance}")
            }
            Action::Assert { check, message } => {
                let message = message
                    .as_ref()
                    .map(|value| format!(" message={}", quote(value)))
                    .unwrap_or_default();
                let check = match check {
                    AssertCheck::Size {
                        name,
                        width,
                        height,
                    } => format!("size {name} eq={width}x{height}"),
                    AssertCheck::Alpha {
                        name,
                        compare,
                        value,
                    } => format!("alpha {name} {}={value}", compare_key(*compare)),
                    AssertCheck::Same {
                        a,
                        b,
                        max,
                        tolerance,
                    } => {
                        let tolerance = if *tolerance == 0 {
                            String::new()
                        } else {
                            format!(" tolerance={tolerance}")
                        };
                        format!("same {a} {b} max={max}{tolerance}")
                    }
                    AssertCheck::Different {
                        a,
                        b,
                        min,
                        tolerance,
                    } => {
                        let tolerance = if *tolerance == 0 {
                            String::new()
                        } else {
                            format!(" tolerance={tolerance}")
                        };
                        format!("different {a} {b} min={min}{tolerance}")
                    }
                    AssertCheck::Colours {
                        name,
                        compare,
                        value,
                    } => format!("colours {name} {}={value}", compare_key(*compare)),
                };
                format!("assert {check}{message}")
            }
        }
    }
}

fn compare_key(compare: Compare) -> &'static str {
    match compare {
        Compare::Greater => "gt",
        Compare::Less => "lt",
        Compare::Equal => "eq",
    }
}

fn format_output(output: &OutputSpec, allow_format: bool) -> String {
    let mut out = String::new();
    if let Some(value) = &output.as_name {
        write!(out, " as={}", quote(value)).unwrap();
    }
    if let Some(value) = &output.out {
        write!(out, " out={}", quote(value)).unwrap();
    }
    if allow_format {
        if let Some(value) = output.format {
            write!(out, " format={value}").unwrap();
        }
    }
    if let Some(value) = output.filter {
        write!(out, " filter={}", value.as_str()).unwrap();
    }
    if let Some(value) = output.compression {
        write!(out, " compression={value}").unwrap();
    }
    if let Some(value) = output.background {
        write!(
            out,
            " background=#{:02x}{:02x}{:02x}",
            value[0], value[1], value[2]
        )
        .unwrap();
    }
    out
}

fn quote(text: &str) -> String {
    let needs_quote = text.is_empty()
        || text
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '#' | '=' | '"' | '\''));
    if !needs_quote {
        return text.to_owned();
    }
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Parse the whole file before any action runs.
pub fn parse(text: &str) -> Result<Scenario> {
    let mut scenario = Scenario {
        artifacts: None,
        from: None,
        max_pixels: None,
        steps: Vec::new(),
    };
    let mut seen_run = false;
    let mut defined: HashSet<String> = HashSet::new();

    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let raw = raw.trim_end_matches('\r');
        let tokens = tokenize(raw, line)?;
        if tokens.is_empty() {
            continue;
        }
        let action = tokens[0].as_str();
        if action != "run" && !seen_run {
            bail!("line {line}: the first action must be `run`");
        }
        let rest = &tokens[1..];
        let parsed = match action {
            "run" => {
                if seen_run {
                    bail!("line {line}: `run` may appear only once");
                }
                seen_run = true;
                let mut fields = Fields::parse(rest, line)?;
                scenario.artifacts = fields
                    .take("artifacts")
                    .map(|value| non_empty(value, "artifacts", line))
                    .transpose()?;
                scenario.from = fields
                    .take("from")
                    .map(|value| non_empty(value, "from", line))
                    .transpose()?;
                scenario.max_pixels = fields
                    .take("max-pixels")
                    .map(|value| {
                        crate::parse_pixels(&value)
                            .map_err(|error| anyhow!("line {line}: max-pixels: {error}"))
                    })
                    .transpose()?;
                fields.finish("run")?;
                None
            }
            "source" => Some(parse_source(rest, line)?),
            "render" => Some(parse_render(rest, line)?),
            "fit" => Some(parse_fit(rest, line)?),
            "crop" => Some(parse_crop(rest, line)?),
            "cover" => Some(parse_cover(rest, line)?),
            "square" => Some(parse_square(rest, line)?),
            "trim" => Some(parse_trim(rest, line)?),
            "sheet" => Some(parse_sheet(rest, line)?),
            "overlay" => Some(parse_overlay(rest, line)?),
            "stats" => Some(parse_stats(rest, line)?),
            "diff" => Some(parse_diff(rest, line)?),
            "assert" => Some(parse_assert(rest, line)?),
            other => bail!(
                "line {line}: unknown action `{other}`; expected source, render, fit, crop, cover, \
                 square, trim, sheet, overlay, stats, diff or assert"
            ),
        };

        if let Some(action) = parsed {
            for reference in action.references() {
                if !defined.contains(reference) {
                    bail!("line {line}: `{reference}` is not defined before this step");
                }
            }
            if let Some(name) = action.defined_name() {
                defined.insert(name.to_owned());
            }
            scenario.steps.push(Step { line, action });
        }
    }

    if !seen_run {
        bail!("the scenario needs a `run` line first");
    }
    if scenario.steps.is_empty() {
        bail!("the scenario has no steps");
    }
    Ok(scenario)
}

fn parse_source(tokens: &[String], line: usize) -> Result<Action> {
    let (positionals, mut fields) = positionals(tokens, 1, line, "source")?;
    let name = valid_name(positionals[0], line)?;
    let from = non_empty(required(&mut fields, "from", "source", line)?, "from", line)?;
    fields.finish("source")?;
    Ok(Action::Source { name, from })
}

fn parse_render(tokens: &[String], line: usize) -> Result<Action> {
    let (positionals, mut fields) = positionals(tokens, 1, line, "render")?;
    let source = valid_reference(positionals[0], line)?;
    let op = Op::parse(&required(&mut fields, "op", "render", line)?)
        .map_err(|error| anyhow!("line {line}: {error}"))?;
    let output = parse_output(&mut fields, line, "render", false)?;
    fields.finish("render")?;
    require_output(&output, "render", line)?;
    Ok(Action::Render { source, op, output })
}

fn parse_fit(tokens: &[String], line: usize) -> Result<Action> {
    let (positionals, mut fields) = positionals(tokens, 1, line, "fit")?;
    let source = valid_reference(positionals[0], line)?;
    let edge = parse_positive_u32(&required(&mut fields, "edge", "fit", line)?, "edge", line)?;
    let upscale = fields.flag("upscale");
    let output = parse_output(&mut fields, line, "fit", true)?;
    fields.finish("fit")?;
    require_output(&output, "fit", line)?;
    Ok(Action::Fit {
        source,
        edge,
        upscale,
        output,
    })
}

fn parse_crop(tokens: &[String], line: usize) -> Result<Action> {
    let (positionals, mut fields) = positionals(tokens, 1, line, "crop")?;
    let source = valid_reference(positionals[0], line)?;
    let (x, y, width, height) = crate::parse_box(&required(&mut fields, "box", "crop", line)?)
        .map_err(|error| anyhow!("line {line}: {error}"))?;
    let zoom = fields
        .take("zoom")
        .map(|value| parse_positive_u32(&value, "zoom", line))
        .transpose()?
        .unwrap_or(1);
    let smooth = fields.flag("smooth");
    let output = parse_output(&mut fields, line, "crop", true)?;
    fields.finish("crop")?;
    require_output(&output, "crop", line)?;
    Ok(Action::Crop {
        source,
        x,
        y,
        width,
        height,
        zoom,
        smooth,
        output,
    })
}

fn parse_cover(tokens: &[String], line: usize) -> Result<Action> {
    let (positionals, mut fields) = positionals(tokens, 1, line, "cover")?;
    let source = valid_reference(positionals[0], line)?;
    let (width, height) = crate::parse_size_pair(&required(&mut fields, "size", "cover", line)?)
        .map_err(|error| anyhow!("line {line}: {error}"))?;
    let upscale = fields.flag("upscale");
    let output = parse_output(&mut fields, line, "cover", true)?;
    fields.finish("cover")?;
    require_output(&output, "cover", line)?;
    Ok(Action::Cover {
        source,
        width,
        height,
        upscale,
        output,
    })
}

fn parse_square(tokens: &[String], line: usize) -> Result<Action> {
    let (positionals, mut fields) = positionals(tokens, 1, line, "square")?;
    let source = valid_reference(positionals[0], line)?;
    let size = parse_positive_u32(
        &required(&mut fields, "size", "square", line)?,
        "size",
        line,
    )?;
    let upscale = fields.flag("upscale");
    let output = parse_output(&mut fields, line, "square", true)?;
    fields.finish("square")?;
    require_output(&output, "square", line)?;
    Ok(Action::Square {
        source,
        size,
        upscale,
        output,
    })
}

fn parse_trim(tokens: &[String], line: usize) -> Result<Action> {
    let (positionals, mut fields) = positionals(tokens, 1, line, "trim")?;
    let source = valid_reference(positionals[0], line)?;
    let tolerance = fields
        .take("tolerance")
        .map(|value| {
            value
                .parse::<u8>()
                .map_err(|_| anyhow!("line {line}: tolerance is not 0-255"))
        })
        .transpose()?
        .unwrap_or(8);
    let output = parse_output(&mut fields, line, "trim", true)?;
    fields.finish("trim")?;
    require_output(&output, "trim", line)?;
    Ok(Action::Trim {
        source,
        tolerance,
        output,
    })
}

fn parse_sheet(tokens: &[String], line: usize) -> Result<Action> {
    let mut fields = Fields::parse(tokens, line)?;
    let cols = parse_positive_u32(&required(&mut fields, "cols", "sheet", line)?, "cols", line)?;
    let (cell_width, cell_height) =
        crate::parse_size_pair(&required(&mut fields, "cell", "sheet", line)?)
            .map_err(|error| anyhow!("line {line}: {error}"))?;
    let from = required(&mut fields, "from", "sheet", line)?
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| valid_reference(name, line))
        .collect::<Result<Vec<_>>>()?;
    if from.is_empty() {
        bail!("line {line}: sheet needs at least one source in from=");
    }
    let output = parse_output(&mut fields, line, "sheet", true)?;
    fields.finish("sheet")?;
    require_output(&output, "sheet", line)?;
    Ok(Action::Sheet {
        from,
        cols,
        cell_width,
        cell_height,
        output,
    })
}

fn parse_overlay(tokens: &[String], line: usize) -> Result<Action> {
    let (positionals, mut fields) = positionals(tokens, 1, line, "overlay")?;
    let base = valid_reference(positionals[0], line)?;
    let layer = valid_reference(&required(&mut fields, "layer", "overlay", line)?, line)?;
    let x = parse_i64(&mut fields, "x", 0, line)?;
    let y = parse_i64(&mut fields, "y", 0, line)?;
    let output = parse_output(&mut fields, line, "overlay", true)?;
    fields.finish("overlay")?;
    require_output(&output, "overlay", line)?;
    Ok(Action::Overlay {
        base,
        layer,
        x,
        y,
        output,
    })
}

fn parse_stats(tokens: &[String], line: usize) -> Result<Action> {
    let (positionals, fields) = positionals(tokens, 1, line, "stats")?;
    fields.finish("stats")?;
    Ok(Action::Stats {
        source: valid_reference(positionals[0], line)?,
    })
}

fn parse_diff(tokens: &[String], line: usize) -> Result<Action> {
    let (positionals, mut fields) = positionals(tokens, 2, line, "diff")?;
    let a = valid_reference(positionals[0], line)?;
    let b = valid_reference(positionals[1], line)?;
    let out = fields.take("out");
    let tolerance = fields
        .take("tolerance")
        .map(|value| {
            value
                .parse::<u8>()
                .map_err(|_| anyhow!("line {line}: tolerance is not 0-255"))
        })
        .transpose()?
        .unwrap_or(0);
    fields.finish("diff")?;
    Ok(Action::Diff {
        a,
        b,
        out,
        tolerance,
    })
}

fn parse_assert(tokens: &[String], line: usize) -> Result<Action> {
    let Some(kind) = tokens.first().map(String::as_str) else {
        bail!("line {line}: assert needs size, alpha, same, different or colours");
    };
    let (positionals, mut fields) = match kind {
        "size" | "alpha" | "colours" => positionals(tokens, 2, line, "assert")?,
        "same" | "different" => positionals(tokens, 3, line, "assert")?,
        other => bail!("line {line}: unknown assertion `{other}`"),
    };
    let message = fields.take("message");
    let check = match kind {
        "size" => {
            let (width, height) =
                crate::parse_size_pair(&required(&mut fields, "eq", "assert", line)?)
                    .map_err(|error| anyhow!("line {line}: {error}"))?;
            AssertCheck::Size {
                name: valid_reference(positionals[1], line)?,
                width,
                height,
            }
        }
        "alpha" => {
            let (compare, value) = parse_compare_f64(&mut fields, line, 0.0, 100.0)?;
            AssertCheck::Alpha {
                name: valid_reference(positionals[1], line)?,
                compare,
                value,
            }
        }
        "colours" => {
            let (compare, value) = parse_compare_u64(&mut fields, line)?;
            AssertCheck::Colours {
                name: valid_reference(positionals[1], line)?,
                compare,
                value,
            }
        }
        "same" => {
            let max = fraction(&required(&mut fields, "max", "assert same", line)?, line)?;
            let tolerance = parse_tolerance(&mut fields, line)?;
            AssertCheck::Same {
                a: valid_reference(positionals[1], line)?,
                b: valid_reference(positionals[2], line)?,
                max,
                tolerance,
            }
        }
        "different" => {
            let min = fraction(
                &required(&mut fields, "min", "assert different", line)?,
                line,
            )?;
            let tolerance = parse_tolerance(&mut fields, line)?;
            AssertCheck::Different {
                a: valid_reference(positionals[1], line)?,
                b: valid_reference(positionals[2], line)?,
                min,
                tolerance,
            }
        }
        _ => unreachable!("kind was checked above"),
    };
    fields.finish("assert")?;
    Ok(Action::Assert { check, message })
}

fn parse_output(
    fields: &mut Fields,
    line: usize,
    action: &str,
    allow_format: bool,
) -> Result<OutputSpec> {
    let output = OutputSpec {
        as_name: fields
            .take("as")
            .map(|value| valid_name(&value, line))
            .transpose()?,
        out: fields
            .take("out")
            .map(|value| non_empty(value, "out", line))
            .transpose()?,
        format: if allow_format {
            fields
                .take("format")
                .map(|value| Format::parse(&value).map_err(|error| anyhow!("line {line}: {error}")))
                .transpose()?
        } else {
            if fields.take("format").is_some() {
                bail!("line {line}: {action} takes its format from op=");
            }
            None
        },
        filter: fields
            .take("filter")
            .map(|value| Filter::parse(&value).map_err(|error| anyhow!("line {line}: {error}")))
            .transpose()?,
        compression: fields
            .take("compression")
            .map(|value| {
                let quality = value
                    .parse::<u8>()
                    .map_err(|_| anyhow!("line {line}: compression is not 1-100"))?;
                if !(1..=100).contains(&quality) {
                    bail!("line {line}: compression must be between 1 and 100");
                }
                Ok(quality)
            })
            .transpose()?,
        background: fields
            .take("background")
            .map(|value| {
                crate::transform::parse_background(&value)
                    .map_err(|error| anyhow!("line {line}: {error}"))
            })
            .transpose()?,
    };
    Ok(output)
}

fn require_output(output: &OutputSpec, action: &str, line: usize) -> Result<()> {
    if output.as_name.is_none() && output.out.is_none() {
        bail!("line {line}: {action} needs out=, as=, or both");
    }
    Ok(())
}

fn parse_compare_f64(
    fields: &mut Fields,
    line: usize,
    min: f64,
    max: f64,
) -> Result<(Compare, f64)> {
    let mut found = Vec::new();
    for (key, compare) in [
        ("gt", Compare::Greater),
        ("lt", Compare::Less),
        ("eq", Compare::Equal),
    ] {
        if let Some(value) = fields.take(key) {
            let value = value
                .parse::<f64>()
                .map_err(|_| anyhow!("line {line}: {key} is not a number"))?;
            if !value.is_finite() || !(min..=max).contains(&value) {
                bail!("line {line}: {key} must be between {min} and {max}");
            }
            found.push((compare, value));
        }
    }
    match found.as_slice() {
        [(compare, value)] => Ok((*compare, *value)),
        [] => bail!("line {line}: assert needs gt=, lt= or eq="),
        _ => bail!("line {line}: assert takes exactly one of gt=, lt= or eq="),
    }
}

fn parse_compare_u64(fields: &mut Fields, line: usize) -> Result<(Compare, u64)> {
    let mut found = Vec::new();
    for (key, compare) in [
        ("gt", Compare::Greater),
        ("lt", Compare::Less),
        ("eq", Compare::Equal),
    ] {
        if let Some(value) = fields.take(key) {
            let value = value
                .parse::<u64>()
                .map_err(|_| anyhow!("line {line}: {key} is not a count"))?;
            found.push((compare, value));
        }
    }
    match found.as_slice() {
        [(compare, value)] => Ok((*compare, *value)),
        [] => bail!("line {line}: assert needs gt=, lt= or eq="),
        _ => bail!("line {line}: assert takes exactly one of gt=, lt= or eq="),
    }
}

fn parse_tolerance(fields: &mut Fields, line: usize) -> Result<u8> {
    fields
        .take("tolerance")
        .map(|value| {
            value
                .parse::<u8>()
                .map_err(|_| anyhow!("line {line}: tolerance is not 0-255"))
        })
        .transpose()
        .map(|value| value.unwrap_or(0))
}

fn parse_i64(fields: &mut Fields, key: &str, default: i64, line: usize) -> Result<i64> {
    fields
        .take(key)
        .map(|value| {
            value
                .parse::<i64>()
                .map_err(|_| anyhow!("line {line}: {key} is not an integer"))
        })
        .transpose()
        .map(|value| value.unwrap_or(default))
}

fn parse_positive_u32(text: &str, name: &str, line: usize) -> Result<u32> {
    let value = text
        .parse::<u32>()
        .map_err(|_| anyhow!("line {line}: {name} is not a positive integer"))?;
    if value == 0 {
        bail!("line {line}: {name} must be positive");
    }
    Ok(value)
}

fn fraction(text: &str, line: usize) -> Result<f64> {
    let value = text
        .parse::<f64>()
        .map_err(|_| anyhow!("line {line}: `{text}` is not a fraction"))?;
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        bail!("line {line}: a fraction must be between 0 and 1");
    }
    Ok(value)
}

fn non_empty(value: String, key: &str, line: usize) -> Result<String> {
    if value.is_empty() {
        bail!("line {line}: {key} must not be empty");
    }
    Ok(value)
}

fn required(fields: &mut Fields, key: &str, action: &str, line: usize) -> Result<String> {
    fields
        .take(key)
        .ok_or_else(|| anyhow!("line {line}: {action} needs {key}="))
}

fn valid_reference(value: &str, line: usize) -> Result<String> {
    valid_name(value, line)
}

fn valid_name(value: &str, line: usize) -> Result<String> {
    if value.is_empty()
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
    {
        bail!("line {line}: `{value}` is not a valid name");
    }
    Ok(value.to_owned())
}

fn positionals<'a>(
    tokens: &'a [String],
    count: usize,
    line: usize,
    action: &str,
) -> Result<(Vec<&'a str>, Fields)> {
    if tokens.len() < count {
        bail!("line {line}: {action} expects {count} positional argument(s)");
    }
    let positionals = tokens[..count].iter().map(String::as_str).collect();
    let fields = Fields::parse(&tokens[count..], line)?;
    Ok((positionals, fields))
}

struct Fields {
    line: usize,
    values: Vec<(String, String)>,
    flags: Vec<String>,
}

impl Fields {
    fn parse(tokens: &[String], line: usize) -> Result<Self> {
        let mut values = Vec::new();
        let mut flags = Vec::new();
        for token in tokens {
            if let Some((key, value)) = token.split_once('=') {
                if key.is_empty() {
                    bail!("line {line}: a field name is empty");
                }
                values.push((key.to_owned(), value.to_owned()));
            } else {
                flags.push(token.clone());
            }
        }
        Ok(Self {
            line,
            values,
            flags,
        })
    }

    fn take(&mut self, key: &str) -> Option<String> {
        let index = self.values.iter().position(|(name, _)| name == key)?;
        Some(self.values.remove(index).1)
    }

    fn flag(&mut self, name: &str) -> bool {
        if let Some(index) = self.flags.iter().position(|flag| flag == name) {
            self.flags.remove(index);
            true
        } else {
            false
        }
    }

    fn finish(self, action: &str) -> Result<()> {
        if let Some((key, _)) = self.values.first() {
            bail!("line {}: unknown field `{key}` for {action}", self.line);
        }
        if let Some(flag) = self.flags.first() {
            bail!("line {}: unknown flag `{flag}` for {action}", self.line);
        }
        Ok(())
    }
}

fn tokenize(line: &str, line_number: usize) -> Result<Vec<String>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();
    let mut quote: Option<char> = None;

    while let Some(c) = chars.next() {
        if let Some(delimiter) = quote {
            match c {
                '\\' => {
                    let escaped = chars
                        .next()
                        .ok_or_else(|| anyhow!("line {line_number}: trailing escape"))?;
                    match escaped {
                        'n' => current.push('\n'),
                        't' => current.push('\t'),
                        '\\' => current.push('\\'),
                        '"' => current.push('"'),
                        '\'' => current.push('\''),
                        other => {
                            bail!("line {line_number}: unknown escape `\\{other}`")
                        }
                    }
                }
                c if c == delimiter => quote = None,
                other => current.push(other),
            }
            continue;
        }

        if c.is_whitespace() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            continue;
        }
        if c == '#' && current.is_empty() {
            break;
        }
        if c == '"' || c == '\'' {
            quote = Some(c);
            continue;
        }
        current.push(c);
    }

    if quote.is_some() {
        bail!("line {line_number}: unterminated quoted value");
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
run artifacts=.tmp/review-run from=.tmp/capture-run max-pixels=64M

source home from=page/home.png
crop home box=0,0,390,120 as=top out=crops/top.png
fit top edge=24 as=top24 out=crops/top-24.png
sheet cols=2 cell=64x64 from=top24,home out=sheets/home.png
overlay home layer=top x=10 y=20 as=marked out=out/marked.png
diff home home out=diffs/home.png
assert same home home max=0.001 message="the header did not move"
assert size top eq=390x120
assert alpha home gt=0
assert colours home lt=256
"#;

    #[test]
    fn parses_a_complete_scenario() {
        let scenario = parse(SAMPLE).unwrap();
        assert_eq!(scenario.artifacts.as_deref(), Some(".tmp/review-run"));
        assert_eq!(scenario.from.as_deref(), Some(".tmp/capture-run"));
        assert_eq!(scenario.max_pixels, Some(64_000_000));
        assert_eq!(scenario.steps.len(), 10);
        assert_eq!(scenario.steps[1].action.kind(), "crop");
    }

    #[test]
    fn quoted_messages_survive_and_reformat() {
        let scenario = parse(SAMPLE).unwrap();
        let formatted = scenario.format();
        assert!(formatted.contains("message=\"the header did not move\""));
        let again = parse(&formatted).unwrap();
        assert_eq!(scenario, again);
    }

    #[test]
    fn rejects_unknown_fields_and_names() {
        let unknown = "run\nsource home from=a.png\ncrop home box=0,0,1,1 nope=1 out=a.png\n";
        assert!(parse(unknown).is_err());

        let undefined = "run\ncrop missing box=0,0,1,1 out=a.png\n";
        assert!(parse(undefined).is_err());
    }

    #[test]
    fn the_run_line_comes_first_and_only_once() {
        assert!(parse("source home from=a.png\nrun\n").is_err());
        assert!(parse("run\nrun\nsource home from=a.png\n").is_err());
        assert!(parse("run\n").is_err());
    }

    #[test]
    fn comments_and_quotes_are_tokenized() {
        let tokens = tokenize("source home from=\"a # b.png\" # a comment", 1).unwrap();
        assert_eq!(tokens, ["source", "home", "from=a # b.png"]);
    }

    #[test]
    fn round_trips_every_action() {
        let text = r#"
run
source a from=a.png
render a op=full/max/0/gray.png as=b out=b.png
fit b edge=8 upscale as=c out=c.png
crop c box=0,0,2,2 zoom=2 smooth as=d out=d.png
cover d size=4x4 upscale as=e out=e.png
square e size=4 upscale as=f out=f.png
trim f tolerance=4 as=g out=g.png
sheet cols=2 cell=4x4 from=g,a as=h out=h.png
overlay a layer=g x=-1 y=2 as=i out=i.png
stats i
diff i a out=diff.png tolerance=2
assert size i eq=4x4
"#;
        let scenario = parse(text).unwrap();
        let formatted = scenario.format();
        assert_eq!(parse(&formatted).unwrap(), scenario);
    }

    #[test]
    fn empty_required_values_are_rejected() {
        assert!(parse("run\nsource home from=\n").is_err());
        assert!(parse("run\nsource home from=a.png\ncrop home box=0,0,1,1 out=\n").is_err());
    }

    #[test]
    fn format_is_stable_for_a_minimal_scenario() {
        let scenario =
            parse("run\nsource home from=a.png\nfit home edge=24 out=a-24.png\n").unwrap();
        let once = scenario.format();
        let twice = parse(&once).unwrap().format();
        assert_eq!(once, twice);
    }
}
