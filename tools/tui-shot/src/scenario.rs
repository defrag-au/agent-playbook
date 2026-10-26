//! The scenario DSL: parse, validate and describe a TUI run.
//!
//! The grammar is deliberately the same shape as `browser-shot`'s: one action per line, positional
//! names first, then `key=value` fields, quoted values where whitespace or `=` is needed. It is
//! parsed before anything is launched, so a scenario that names an undeclared TUI or captures
//! outside its artifacts directory fails while the line is being read.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::keys;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
pub const DEFAULT_SETTLE: Duration = Duration::from_millis(40);
pub const DEFAULT_TERM: &str = "xterm-256color";
pub const DEFAULT_ARTIFACTS: &str = ".tmp/tui-shot";

/// Columns by rows, the order `tmux -x … -y …` uses. A TUI's own `stty size` reports the reverse,
/// so the report always names both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
}

impl Size {
    pub fn new(cols: u16, rows: u16) -> Result<Self> {
        if cols == 0 || rows == 0 {
            bail!("size must be greater than zero");
        }
        // A screen larger than this is almost certainly a typo, and every cell is a cell the
        // emulator has to hold.
        if cols > 500 || rows > 200 {
            bail!("size {cols}x{rows} is larger than the 500x200 cap");
        }
        Ok(Self { cols, rows })
    }
}

impl std::fmt::Display for Size {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}x{}", self.cols, self.rows)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Debug,
    Release,
}

impl Profile {
    pub fn directory(self) -> &'static str {
        match self {
            Profile::Debug => "debug",
            Profile::Release => "release",
        }
    }
}

impl std::fmt::Display for Profile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.directory())
    }
}

#[derive(Debug, Clone)]
pub struct RunConfig {
    /// The default bound on a single wait.
    pub timeout: Duration,
    /// How long the screen must be quiet before a capture is taken.
    pub settle: Duration,
    pub artifacts: PathBuf,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_TIMEOUT,
            settle: DEFAULT_SETTLE,
            artifacts: PathBuf::from(DEFAULT_ARTIFACTS),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TuiSpec {
    pub name: String,
    /// A binary target name in this workspace. Never a path and never a command line.
    pub bin: String,
    pub size: Size,
    pub profile: Profile,
    pub args: Vec<String>,
    pub term: String,
    pub timeout: Option<Duration>,
}

impl TuiSpec {
    pub fn effective_timeout(&self, run: &RunConfig) -> Duration {
        self.timeout.unwrap_or(run.timeout)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wait {
    Contains(String),
    Absent(String),
    Idle(Duration),
    Exit,
}

impl std::fmt::Display for Wait {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Wait::Contains(text) => write!(formatter, "contains {text:?}"),
            Wait::Absent(text) => write!(formatter, "absent {text:?}"),
            Wait::Idle(period) => write!(formatter, "idle {}ms", period.as_millis()),
            Wait::Exit => formatter.write_str("exit"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assertion {
    Contains(String),
    Absent(String),
    Size(Size),
}

impl std::fmt::Display for Assertion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Assertion::Contains(text) => write!(formatter, "contains {text:?}"),
            Assertion::Absent(text) => write!(formatter, "absent {text:?}"),
            Assertion::Size(size) => write!(formatter, "size {size}"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Step {
    Wait {
        tui: String,
        waits: Vec<Wait>,
        timeout: Option<Duration>,
    },
    Key {
        tui: String,
        key: String,
    },
    Type {
        tui: String,
        text: String,
    },
    Resize {
        tui: String,
        size: Size,
    },
    Capture {
        tui: String,
        out: PathBuf,
    },
    Assert {
        tui: String,
        check: Assertion,
        message: Option<String>,
    },
}

impl Step {
    pub fn tui(&self) -> &str {
        match self {
            Step::Wait { tui, .. }
            | Step::Key { tui, .. }
            | Step::Type { tui, .. }
            | Step::Resize { tui, .. }
            | Step::Capture { tui, .. }
            | Step::Assert { tui, .. } => tui,
        }
    }

    pub fn action(&self) -> &'static str {
        match self {
            Step::Wait { .. } => "wait",
            Step::Key { .. } => "key",
            Step::Type { .. } => "type",
            Step::Resize { .. } => "resize",
            Step::Capture { .. } => "capture",
            Step::Assert { .. } => "assert",
        }
    }

    pub fn detail(&self) -> String {
        match self {
            Step::Wait { waits, .. } => waits
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" · "),
            Step::Key { key, .. } => key.clone(),
            Step::Type { text, .. } => format!("{text:?}"),
            Step::Resize { size, .. } => size.to_string(),
            Step::Capture { out, .. } => out.display().to_string(),
            Step::Assert { check, message, .. } => match message {
                Some(message) => format!("{check} · {message:?}"),
                None => check.to_string(),
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct Scenario {
    pub run: RunConfig,
    pub tuis: Vec<TuiSpec>,
    pub steps: Vec<Step>,
    /// The 1-based line each declaration came from, parallel to `tuis`.
    pub tui_lines: Vec<usize>,
    /// The 1-based line each step came from, parallel to `steps`.
    pub step_lines: Vec<usize>,
}

impl Scenario {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("read scenario {}", path.display()))?;
        Scenario::parse(&text).with_context(|| format!("scenario {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let mut run = RunConfig::default();
        let mut run_seen = false;
        let mut tuis = Vec::new();
        let mut tui_lines = Vec::new();
        let mut steps = Vec::new();
        let mut step_lines = Vec::new();

        for (index, raw) in text.lines().enumerate() {
            let line = index + 1;
            let parsed = tokenize_line(raw, line)?;
            let Some(first) = parsed.tokens.first() else {
                continue;
            };
            if first.key.is_some() {
                return Err(at(
                    first,
                    "a line must start with run, tui or a step action",
                ));
            }
            let action = first.value.as_str();
            let rest = &parsed.tokens[1..];
            match action {
                "run" => {
                    if run_seen {
                        return Err(at(first, "run may appear only once"));
                    }
                    run = parse_run(rest)?;
                    run_seen = true;
                }
                "tui" => {
                    tuis.push(parse_tui(rest, first)?);
                    tui_lines.push(first.line);
                }
                "wait" => {
                    steps.push(parse_wait(rest, first)?);
                    step_lines.push(first.line);
                }
                "key" => {
                    steps.push(parse_key(rest, first)?);
                    step_lines.push(first.line);
                }
                "type" => {
                    steps.push(parse_type(rest, first)?);
                    step_lines.push(first.line);
                }
                "resize" => {
                    steps.push(parse_resize(rest, first)?);
                    step_lines.push(first.line);
                }
                "capture" => {
                    steps.push(parse_capture(rest, first)?);
                    step_lines.push(first.line);
                }
                "assert" => {
                    steps.push(parse_assert(rest, first)?);
                    step_lines.push(first.line);
                }
                other => {
                    return Err(at(
                        first,
                        format!(
                            "unknown action {other:?}; expected run, tui, wait, key, type, resize, \
                             capture or assert"
                        ),
                    ));
                }
            }
        }

        let scenario = Scenario {
            run,
            tuis,
            steps,
            tui_lines,
            step_lines,
        };
        scenario.validate()?;
        Ok(scenario)
    }

    pub fn validate(&self) -> Result<()> {
        if self.tuis.is_empty() {
            bail!("a scenario must declare at least one `tui`");
        }
        if self.steps.is_empty() {
            bail!("a scenario must contain at least one step");
        }

        let mut names = BTreeSet::new();
        for (spec, line) in self.tuis.iter().zip(&self.tui_lines) {
            if !is_bin_name(&spec.bin) {
                bail!(
                    "line {line}: tui {:?}: `bin` must be a binary target name (letters, digits, \
                     `_` or `-`), not a path or a command",
                    spec.name
                );
            }
            if spec.args.iter().any(|arg| arg.contains('\0')) {
                bail!(
                    "line {line}: tui {:?}: an argument contains a NUL",
                    spec.name
                );
            }
            if !names.insert(spec.name.as_str()) {
                bail!("line {line}: duplicate tui name {:?}", spec.name);
            }
        }

        for (step, line) in self.steps.iter().zip(&self.step_lines) {
            if !names.contains(step.tui()) {
                bail!(
                    "line {line}: {} names undeclared tui {:?}",
                    step.action(),
                    step.tui()
                );
            }
            match step {
                Step::Wait { waits, .. } => {
                    if waits.is_empty() {
                        bail!("line {line}: wait needs at least one condition");
                    }
                    for wait in waits {
                        match wait {
                            Wait::Contains(text) | Wait::Absent(text) if text.is_empty() => {
                                bail!("line {line}: wait text must not be empty");
                            }
                            Wait::Idle(period) if period.is_zero() => {
                                bail!("line {line}: idle must be greater than zero");
                            }
                            _ => {}
                        }
                    }
                }
                Step::Key { key, .. } => {
                    keys::encode(key).with_context(|| format!("line {line}: key {key:?}"))?;
                }
                Step::Capture { out, .. } => {
                    if !safe_relative(out) || out.file_name().is_none() {
                        bail!(
                            "line {line}: capture out {} must be a relative file path inside the \
                             artifacts directory, with no `..`",
                            out.display()
                        );
                    }
                }
                Step::Resize { size, .. } => {
                    Size::new(size.cols, size.rows)
                        .with_context(|| format!("line {line}: resize"))?;
                }
                Step::Assert { check, .. } => match check {
                    Assertion::Contains(text) | Assertion::Absent(text) if text.is_empty() => {
                        bail!("line {line}: assert text must not be empty");
                    }
                    Assertion::Size(size) => {
                        Size::new(size.cols, size.rows)
                            .with_context(|| format!("line {line}: assert size"))?;
                    }
                    _ => {}
                },
                Step::Type { .. } => {}
            }
        }

        if self.run.artifacts.as_os_str().is_empty() {
            bail!("run artifacts must not be empty");
        }
        if self.run.settle.is_zero() {
            bail!("run settle must be greater than zero");
        }
        Ok(())
    }

    pub fn tui(&self, name: &str) -> Option<&TuiSpec> {
        self.tuis.iter().find(|spec| spec.name == name)
    }

    /// Every capture stem, in order, for the check report.
    pub fn captures(&self) -> Vec<&Path> {
        self.steps
            .iter()
            .filter_map(|step| match step {
                Step::Capture { out, .. } => Some(out.as_path()),
                _ => None,
            })
            .collect()
    }
}

fn parse_run(tokens: &[Token]) -> Result<RunConfig> {
    let mut kvs = collect_kvs(tokens)?;
    let mut run = RunConfig::default();
    if let Some(token) = take(&mut kvs, "timeout") {
        run.timeout = parse_duration(&token)?;
    }
    if let Some(token) = take(&mut kvs, "settle") {
        run.settle = parse_duration(&token)?;
    }
    if let Some(token) = take(&mut kvs, "artifacts") {
        run.artifacts = PathBuf::from(token.value);
    }
    reject_unknown(&kvs, "run")?;
    Ok(run)
}

fn parse_tui(tokens: &[Token], first: &Token) -> Result<TuiSpec> {
    let positionals = positionals(tokens);
    if positionals.len() != 1 {
        return Err(at(first, "tui needs one name"));
    }
    let name = positionals[0].value.clone();
    if !is_name(&name) {
        return Err(at(
            positionals[0],
            "a tui name is letters, digits, `_` or `-`",
        ));
    }

    let mut kvs = collect_kvs(tokens)?;
    let bin = required(&mut kvs, "bin", first, "tui")?.value;
    let size = match take(&mut kvs, "size") {
        Some(token) => parse_size(&token)?,
        None => Size {
            cols: 120,
            rows: 40,
        },
    };
    let profile = match take(&mut kvs, "profile") {
        Some(token) => match token.value.as_str() {
            "debug" => Profile::Debug,
            "release" => Profile::Release,
            other => {
                return Err(at(
                    &token,
                    format!("profile {other:?} is not `debug` or `release`"),
                ));
            }
        },
        None => Profile::Debug,
    };
    let args = match take(&mut kvs, "args") {
        Some(token) => shell_words::split(&token.value)
            .with_context(|| format!("line {}:{}: split args", token.line, token.column))?,
        None => Vec::new(),
    };
    let term = take(&mut kvs, "term").map_or_else(|| DEFAULT_TERM.to_owned(), |token| token.value);
    let timeout = take(&mut kvs, "timeout")
        .map(|token| parse_duration(&token))
        .transpose()?;
    reject_unknown(&kvs, "tui")?;

    Ok(TuiSpec {
        name,
        bin,
        size,
        profile,
        args,
        term,
        timeout,
    })
}

fn parse_wait(tokens: &[Token], first: &Token) -> Result<Step> {
    let positionals = positionals(tokens);
    if positionals.len() != 1 {
        return Err(at(first, "wait needs one tui name"));
    }
    let tui = positionals[0].value.clone();

    let mut kvs = collect_kvs(tokens)?;
    let mut waits = Vec::new();
    if let Some(token) = take(&mut kvs, "contains") {
        if token.value.is_empty() {
            return Err(at(&token, "contains must not be empty"));
        }
        waits.push(Wait::Contains(token.value));
    }
    if let Some(token) = take(&mut kvs, "absent") {
        if token.value.is_empty() {
            return Err(at(&token, "absent must not be empty"));
        }
        waits.push(Wait::Absent(token.value));
    }
    if let Some(token) = take(&mut kvs, "idle") {
        waits.push(Wait::Idle(parse_duration(&token)?));
    }
    if let Some(token) = take(&mut kvs, "exit") {
        if parse_bool(&token)? {
            waits.push(Wait::Exit);
        }
    }
    let timeout = take(&mut kvs, "timeout")
        .map(|token| parse_duration(&token))
        .transpose()?;
    reject_unknown(&kvs, "wait")?;

    if waits.is_empty() {
        return Err(at(
            first,
            "wait needs one of contains, absent, idle or exit=true",
        ));
    }

    Ok(Step::Wait {
        tui,
        waits,
        timeout,
    })
}

fn parse_key(tokens: &[Token], first: &Token) -> Result<Step> {
    let positionals = positionals(tokens);
    if positionals.len() != 2 {
        return Err(at(first, "key needs a tui and a key name"));
    }
    let tui = positionals[0].value.clone();
    let key = positionals[1].value.clone();
    let kvs = collect_kvs(tokens)?;
    reject_unknown(&kvs, "key")?;
    keys::encode(&key).with_context(|| format!("line {}:{}", first.line, first.column))?;
    Ok(Step::Key { tui, key })
}

fn parse_type(tokens: &[Token], first: &Token) -> Result<Step> {
    let positionals = positionals(tokens);
    if positionals.len() != 2 {
        return Err(at(first, "type needs a tui and a quoted string"));
    }
    let kvs = collect_kvs(tokens)?;
    reject_unknown(&kvs, "type")?;
    Ok(Step::Type {
        tui: positionals[0].value.clone(),
        text: positionals[1].value.clone(),
    })
}

fn parse_resize(tokens: &[Token], first: &Token) -> Result<Step> {
    let positionals = positionals(tokens);
    if positionals.len() != 2 {
        return Err(at(first, "resize needs a tui and a size such as 80x24"));
    }
    let kvs = collect_kvs(tokens)?;
    reject_unknown(&kvs, "resize")?;
    Ok(Step::Resize {
        tui: positionals[0].value.clone(),
        size: parse_size(positionals[1])?,
    })
}

fn parse_capture(tokens: &[Token], first: &Token) -> Result<Step> {
    let positionals = positionals(tokens);
    if positionals.len() != 1 {
        return Err(at(first, "capture needs one tui name"));
    }
    let mut kvs = collect_kvs(tokens)?;
    let out = required(&mut kvs, "out", first, "capture")?;
    reject_unknown(&kvs, "capture")?;
    Ok(Step::Capture {
        tui: positionals[0].value.clone(),
        out: PathBuf::from(out.value),
    })
}

fn parse_assert(tokens: &[Token], first: &Token) -> Result<Step> {
    let positionals = positionals(tokens);
    if positionals.len() != 1 {
        return Err(at(first, "assert needs one tui name"));
    }
    let mut kvs = collect_kvs(tokens)?;
    let mut checks = 0;
    let mut check = None;
    if let Some(token) = take(&mut kvs, "contains") {
        checks += 1;
        check = Some(Assertion::Contains(token.value));
    }
    if let Some(token) = take(&mut kvs, "absent") {
        checks += 1;
        check = Some(Assertion::Absent(token.value));
    }
    if let Some(token) = take(&mut kvs, "size") {
        checks += 1;
        check = Some(Assertion::Size(parse_size(&token)?));
    }
    let message = take(&mut kvs, "message").map(|token| token.value);
    reject_unknown(&kvs, "assert")?;

    if checks != 1 {
        return Err(at(
            first,
            "assert needs exactly one of contains, absent or size",
        ));
    }

    Ok(Step::Assert {
        tui: positionals[0].value.clone(),
        check: check.expect("checked above"),
        message,
    })
}

/// A binary target name: no path separator, no leading dash, nothing a shell could read as an
/// option.
pub fn is_bin_name(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn is_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// True when a path is relative and cannot leave the directory it is joined to.
pub fn safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

fn parse_size(token: &Token) -> Result<Size> {
    parse_size_value(&token.value).map_err(|error| at(token, error))
}

pub fn parse_size_value(value: &str) -> Result<Size> {
    let (cols, rows) = value
        .split_once('x')
        .or_else(|| value.split_once('X'))
        .ok_or_else(|| anyhow::anyhow!("size {value:?} is not <cols>x<rows>, such as 120x40"))?;
    let cols: u16 = cols
        .parse()
        .with_context(|| format!("size {value:?} columns"))?;
    let rows: u16 = rows
        .parse()
        .with_context(|| format!("size {value:?} rows"))?;
    Size::new(cols, rows)
}

fn parse_duration(token: &Token) -> Result<Duration> {
    parse_duration_value(&token.value).map_err(|error| at(token, error))
}

pub fn parse_duration_value(value: &str) -> Result<Duration> {
    let (number, factor) = if let Some(number) = value.strip_suffix("ms") {
        (number, 1)
    } else if let Some(number) = value.strip_suffix('s') {
        (number, 1000)
    } else {
        (value, 1)
    };
    let number: u64 = number
        .parse()
        .with_context(|| format!("duration {value:?}"))?;
    if number == 0 {
        bail!("a duration must be greater than zero");
    }
    let millis = number
        .checked_mul(factor)
        .ok_or_else(|| anyhow::anyhow!("duration is too large"))?;
    Ok(Duration::from_millis(millis))
}

fn parse_bool(token: &Token) -> Result<bool> {
    match token.value.as_str() {
        "true" | "yes" | "1" => Ok(true),
        "false" | "no" | "0" => Ok(false),
        other => Err(at(token, format!("{other:?} is not true or false"))),
    }
}

fn collect_kvs(tokens: &[Token]) -> Result<BTreeMap<String, Token>> {
    let mut kvs = BTreeMap::new();
    for token in tokens {
        let Some(key) = &token.key else {
            continue;
        };
        if kvs.insert(key.clone(), token.clone()).is_some() {
            return Err(at(token, format!("duplicate field `{key}`")));
        }
    }
    Ok(kvs)
}

fn positionals(tokens: &[Token]) -> Vec<&Token> {
    tokens.iter().filter(|token| token.key.is_none()).collect()
}

fn take(kvs: &mut BTreeMap<String, Token>, key: &str) -> Option<Token> {
    kvs.remove(key)
}

fn required(
    kvs: &mut BTreeMap<String, Token>,
    key: &str,
    first: &Token,
    action: &str,
) -> Result<Token> {
    take(kvs, key).ok_or_else(|| at(first, format!("{action} needs `{key}=…`")))
}

fn reject_unknown(kvs: &BTreeMap<String, Token>, action: &str) -> Result<()> {
    if let Some((key, token)) = kvs.iter().next() {
        return Err(at(token, format!("{action} has no field `{key}`")));
    }
    Ok(())
}

fn at(token: &Token, message: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!("line {}:{}: {message}", token.line, token.column)
}

#[derive(Debug, Clone)]
struct Token {
    value: String,
    key: Option<String>,
    line: usize,
    column: usize,
}

struct LineTokens {
    tokens: Vec<Token>,
}

fn tokenize_line(line: &str, number: usize) -> Result<LineTokens> {
    let chars: Vec<char> = line.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < chars.len() {
        while index < chars.len() && chars[index].is_whitespace() {
            index += 1;
        }
        if index >= chars.len() {
            break;
        }
        if chars[index] == '#' {
            break;
        }

        let column = index + 1;
        if chars[index] == '"' || chars[index] == '\'' {
            let (value, next) = read_quoted(&chars, index, number, column)?;
            tokens.push(Token {
                value,
                key: None,
                line: number,
                column,
            });
            index = next;
            if index < chars.len() && !chars[index].is_whitespace() {
                bail!(
                    "line {number}:{}: expected whitespace after a quoted value",
                    index + 1
                );
            }
            continue;
        }

        let start = index;
        let mut key = None;
        while index < chars.len() && !chars[index].is_whitespace() {
            if chars[index] == '=' {
                let candidate: String = chars[start..index].iter().collect();
                if candidate.is_empty() {
                    bail!(
                        "line {number}:{column}: unexpected '='; quote expressions that contain spaces or '='"
                    );
                }
                key = Some(candidate);
                index += 1;
                break;
            }
            index += 1;
        }

        let value = if let Some(_key) = &key {
            if index < chars.len() && (chars[index] == '"' || chars[index] == '\'') {
                let (value, next) = read_quoted(&chars, index, number, index + 1)?;
                index = next;
                if index < chars.len() && !chars[index].is_whitespace() {
                    bail!(
                        "line {number}:{}: expected whitespace after a quoted value",
                        index + 1
                    );
                }
                value
            } else {
                let start = index;
                while index < chars.len() && !chars[index].is_whitespace() {
                    index += 1;
                }
                chars[start..index].iter().collect()
            }
        } else {
            chars[start..index].iter().collect()
        };

        if key.is_none() && value.is_empty() {
            continue;
        }
        tokens.push(Token {
            value,
            key,
            line: number,
            column,
        });
    }

    Ok(LineTokens { tokens })
}

fn read_quoted(
    chars: &[char],
    start: usize,
    line: usize,
    column: usize,
) -> Result<(String, usize)> {
    let quote = chars[start];
    let mut value = String::new();
    let mut index = start + 1;
    while index < chars.len() {
        let character = chars[index];
        if character == quote {
            return Ok((value, index + 1));
        }
        if character == '\\' {
            index += 1;
            let Some(escaped) = chars.get(index) else {
                break;
            };
            match escaped {
                'n' => value.push('\n'),
                't' => value.push('\t'),
                '\\' => value.push('\\'),
                '"' => value.push('"'),
                '\'' => value.push('\''),
                other => {
                    bail!("line {line}:{}: unknown escape \\{other}", index + 1);
                }
            }
            index += 1;
            continue;
        }
        value.push(character);
        index += 1;
    }
    bail!("line {line}:{column}: unterminated quoted value")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
run timeout=5s settle=60ms artifacts=.tmp/run

tui app bin=factotum-tui size=120x42 args="--base ws://127.0.0.1:7100 --session s-1 --token demo"

wait app contains="factotum"
capture app out=01-arrival

key app i
type app "second question"
key app enter
wait app contains="second question" idle=40ms
capture app out=02-sent

key app 1
wait app contains="deploy"
capture app out=03-permission

resize app 80x24
wait app idle=80ms
capture app out=frames/04-narrow

key app q
wait app exit=true
"#;

    #[test]
    fn parses_the_sample() {
        let scenario = Scenario::parse(SAMPLE).unwrap();
        assert_eq!(scenario.run.timeout, Duration::from_secs(5));
        assert_eq!(scenario.run.settle, Duration::from_millis(60));
        assert_eq!(scenario.run.artifacts, PathBuf::from(".tmp/run"));
        assert_eq!(scenario.tuis.len(), 1);
        let tui = &scenario.tuis[0];
        assert_eq!(tui.name, "app");
        assert_eq!(tui.bin, "factotum-tui");
        assert_eq!(
            tui.size,
            Size {
                cols: 120,
                rows: 42
            }
        );
        assert_eq!(
            tui.args,
            [
                "--base",
                "ws://127.0.0.1:7100",
                "--session",
                "s-1",
                "--token",
                "demo"
            ]
        );
        assert_eq!(scenario.steps.len(), 15);
        assert_eq!(
            scenario.captures(),
            [
                Path::new("01-arrival"),
                Path::new("02-sent"),
                Path::new("03-permission"),
                Path::new("frames/04-narrow"),
            ]
        );
    }

    #[test]
    fn a_line_starts_with_an_action() {
        let error = Scenario::parse("bin=factotum\n").unwrap_err().to_string();
        assert!(error.contains("must start with"), "{error}");
    }

    #[test]
    fn rejects_an_unknown_action() {
        let error = Scenario::parse("tui app bin=x\npoke app\n")
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown action"), "{error}");
    }

    #[test]
    fn rejects_an_undeclared_tui() {
        let error = Scenario::parse("tui app bin=x\nwait other contains=\"x\"\n")
            .unwrap_err()
            .to_string();
        assert!(error.contains("undeclared tui"), "{error}");
    }

    #[test]
    fn rejects_duplicate_names() {
        let error = Scenario::parse("tui app bin=x\ntui app bin=y\nwait app idle=10\n")
            .unwrap_err()
            .to_string();
        assert!(error.contains("duplicate tui name"), "{error}");
    }

    #[test]
    fn rejects_a_capture_that_leaves_the_artifacts_directory() {
        let error = Scenario::parse("tui app bin=x\ncapture app out=../escape\n")
            .unwrap_err()
            .to_string();
        assert!(error.contains("relative file path inside"), "{error}");
    }

    #[test]
    fn rejects_a_bin_that_is_a_path() {
        let error = Scenario::parse("tui app bin=./target/debug/x\nwait app idle=10\n")
            .unwrap_err()
            .to_string();
        assert!(error.contains("binary target name"), "{error}");
    }

    #[test]
    fn rejects_a_duplicate_field() {
        let error = Scenario::parse("tui app bin=x\nwait app contains=\"x\" contains=\"y\"\n")
            .unwrap_err()
            .to_string();
        assert!(error.contains("duplicate field"), "{error}");
    }

    #[test]
    fn rejects_an_empty_wait() {
        let error = Scenario::parse("tui app bin=x\nwait app\n")
            .unwrap_err()
            .to_string();
        assert!(error.contains("needs one of"), "{error}");
    }

    #[test]
    fn parses_quoted_text_with_escapes() {
        let scenario = Scenario::parse("tui app bin=x\ntype app \"line\\n\\\"two\\\"\"\n").unwrap();
        match &scenario.steps[0] {
            Step::Type { text, .. } => assert_eq!(text, "line\n\"two\""),
            other => panic!("unexpected step: {other:?}"),
        }
    }

    #[test]
    fn a_comment_is_not_a_token() {
        let scenario =
            Scenario::parse("tui app bin=x # the program\nwait app idle=10 # settled\n").unwrap();
        assert_eq!(scenario.steps.len(), 1);
    }

    #[test]
    fn durations_accept_suffixes() {
        for (text, millis) in [("40", 40), ("40ms", 40), ("2s", 2000)] {
            let token = Token {
                value: text.to_owned(),
                key: None,
                line: 1,
                column: 1,
            };
            assert_eq!(
                parse_duration(&token).unwrap(),
                Duration::from_millis(millis)
            );
        }
    }

    #[test]
    fn sizes_are_columns_by_rows() {
        let token = Token {
            value: "120x42".to_owned(),
            key: None,
            line: 1,
            column: 1,
        };
        let size = parse_size(&token).unwrap();
        assert_eq!(size.cols, 120);
        assert_eq!(size.rows, 42);
    }
}
