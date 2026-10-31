//! `at-cardano run` — a scenario file: several questions, values carried between them, checks.
//!
//! A scenario is the same closed grammar as the command line, written down. A `target` line
//! names a catalogue verb, so a scenario cannot ask a question the binary could not be asked
//! directly; it cannot name a host, add a header or reach `/submittx`. What it adds is that an
//! answer can be read — `save txs.last.tx_hash as prev` — and used in the next step's target.
//! That is the thing a shell script does badly: it carries a value by parsing output with
//! `jq`, or by retyping the hash into the next command.
//!
//! The grammar and every field are documented in `tools/at-cardano/SCENARIO.md`.

mod execute;
mod extract;
mod parse;

use std::collections::HashMap;
use std::time::Duration;

use crate::koios::Network;
use crate::verbs::Options;

pub use execute::run;
pub use parse::parse;

/// A scenario is a text file. The cap is generous for the grammar and small enough that a
/// mistyped path cannot become a read of something large.
pub const MAX_BYTES: u64 = 256 * 1024;
/// Steps a file may contain. A scenario is a short sequence of questions, not a program.
pub const MAX_STEPS: usize = 64;
/// The whole run's bound, checked before each step. One request is already bounded by the
/// HTTP client's own 60-second timeout.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
pub const MAX_TIMEOUT: Duration = Duration::from_secs(600);
/// Content lines a run may print across every answer, before it says what it withheld.
pub const MAX_PRINTED_LINES: usize = 200;

pub struct Scenario {
    pub network: Option<Network>,
    pub keyless: bool,
    pub timeout: Duration,
    pub steps: Vec<Step>,
}

pub enum Step {
    Target(Target),
    Save(Save),
    Assert(Assert),
    Echo(Echo),
}

/// One question: a name to bind the answer to, a catalogue verb, its positionals and fields.
pub struct Target {
    pub line: usize,
    pub name: String,
    pub verb: &'static crate::catalogue::Verb,
    pub positionals: Vec<Template>,
    pub fields: Vec<Field>,
}

/// One value read out of an earlier answer.
pub struct Save {
    pub line: usize,
    pub source: String,
    pub row: Row,
    pub column: String,
    pub bind: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    First,
    Last,
    Index(usize),
}

pub struct Assert {
    pub line: usize,
    pub source: String,
    pub checks: Vec<Check>,
    pub message: Option<Template>,
}

pub enum Check {
    Count(Compare, usize),
    Contains(Template),
    Absent(Template),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Compare {
    Eq,
    Gt,
    Lt,
    Ge,
    Le,
}

impl Compare {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "=" => Some(Self::Eq),
            ">" => Some(Self::Gt),
            "<" => Some(Self::Lt),
            ">=" => Some(Self::Ge),
            "<=" => Some(Self::Le),
            _ => None,
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::Gt => ">",
            Self::Lt => "<",
            Self::Ge => ">=",
            Self::Le => "<=",
        }
    }

    pub fn holds(self, actual: usize, expected: usize) -> bool {
        match self {
            Self::Eq => actual == expected,
            Self::Gt => actual > expected,
            Self::Lt => actual < expected,
            Self::Ge => actual >= expected,
            Self::Le => actual <= expected,
        }
    }
}

pub struct Echo {
    pub line: usize,
    pub text: Template,
}

/// A value with `${name}` holes, resolved from the saves that ran before it. Parsed once, so a
/// reference to a name no `save` ever binds is an error while the line is read.
#[derive(Clone)]
pub struct Template {
    parts: Vec<Part>,
    /// The text as written, for a failure message that shows what was asked for.
    pub source: String,
}

#[derive(Clone)]
pub(crate) enum Part {
    Literal(String),
    Ref(String),
}

impl Template {
    pub(crate) fn new(parts: Vec<Part>, source: String) -> Self {
        Self { parts, source }
    }

    pub fn is_literal(&self) -> bool {
        self.parts
            .iter()
            .all(|part| matches!(part, Part::Literal(_)))
    }

    pub fn literal(&self) -> String {
        let mut out = String::new();
        for part in &self.parts {
            if let Part::Literal(text) = part {
                out.push_str(text);
            }
        }
        out
    }

    pub fn fill(&self, bindings: &HashMap<String, String>) -> Result<String, String> {
        let mut out = String::new();
        for part in &self.parts {
            match part {
                Part::Literal(text) => out.push_str(text),
                Part::Ref(name) => match bindings.get(name) {
                    Some(value) => out.push_str(value),
                    None => {
                        return Err(format!(
                        "`${{{name}}}` is not bound · the save that binds it did not run or did \
                             not succeed"
                    ))
                    }
                },
            }
        }
        Ok(out)
    }
}

/// A per-target field. The name is resolved against the verb's own flags while the line is
/// read, so a field the verb does not take is a parse error and there is no second table to
/// drift from the first.
pub enum Field {
    Limit(Template),
    Offset(Template),
    Order(Template),
    Select(Template),
    Epoch(Template),
    Body(Template),
    Query(Template),
    History(bool),
    All(bool),
    Summary(bool),
    NoExtended(bool),
    NoInputs(bool),
    NoAssets(bool),
    Scripts(bool),
    Post(bool),
}

impl Field {
    /// The switch a bare token or `name=true` builds. `None` for a name that is not a switch
    /// the scenario knows; the parser reports it against the verb.
    pub fn switch(name: &str, value: bool) -> Option<Self> {
        Some(match name {
            "history" => Self::History(value),
            "all" => Self::All(value),
            "summary" => Self::Summary(value),
            "scripts" => Self::Scripts(value),
            "post" => Self::Post(value),
            "no-extended" => Self::NoExtended(value),
            "no-inputs" => Self::NoInputs(value),
            "no-assets" => Self::NoAssets(value),
            _ => return None,
        })
    }

    /// The `name=value` form. `None` for a name the scenario does not carry; the parser reports
    /// it against the verb.
    pub fn valued(name: &str, value: Template) -> Option<Self> {
        Some(match name {
            "limit" => Self::Limit(value),
            "offset" => Self::Offset(value),
            "order" => Self::Order(value),
            "select" => Self::Select(value),
            "epoch" => Self::Epoch(value),
            "body" => Self::Body(value),
            "query" => Self::Query(value),
            _ => return None,
        })
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Limit(_) => "limit",
            Self::Offset(_) => "offset",
            Self::Order(_) => "order",
            Self::Select(_) => "select",
            Self::Epoch(_) => "epoch",
            Self::Body(_) => "body",
            Self::Query(_) => "query",
            Self::History(_) => "history",
            Self::All(_) => "all",
            Self::Summary(_) => "summary",
            Self::NoExtended(_) => "no-extended",
            Self::NoInputs(_) => "no-inputs",
            Self::NoAssets(_) => "no-assets",
            Self::Scripts(_) => "scripts",
            Self::Post(_) => "post",
        }
    }

    pub fn apply(
        &self,
        options: &mut Options,
        bindings: &HashMap<String, String>,
    ) -> Result<(), String> {
        match self {
            Self::Limit(template) => {
                let asked = number(self.name(), template, bindings)?;
                if asked == 0 {
                    return Err(
                        "limit 0 asks for no rows · the smallest useful limit is 1".to_string()
                    );
                }
                let ceiling = crate::cli::MAX_ROWS;
                options.limit = asked.min(ceiling);
                options.limit_clamped_from = (asked > ceiling).then_some(asked);
                options.limit_explicit = true;
            }
            Self::Offset(template) => {
                options.offset = number(self.name(), template, bindings)?;
                options.offset_explicit = true;
            }
            Self::Epoch(template) => {
                let raw = template.fill(bindings)?;
                options.epoch = Some(
                    raw.parse()
                        .map_err(|_| format!("epoch {raw} is not a number"))?,
                );
            }
            Self::Order(template) => {
                let raw = template.fill(bindings)?;
                crate::cli::valid_order(&raw).map_err(|fail| fail.message)?;
                options.order = Some(raw);
            }
            Self::Select(template) => {
                let raw = template.fill(bindings)?;
                crate::cli::valid_select(&raw).map_err(|fail| fail.message)?;
                options.select = Some(raw);
            }
            Self::Body(template) => options.body = Some(template.fill(bindings)?),
            Self::Query(template) => {
                let raw = template.fill(bindings)?;
                options
                    .query
                    .push(crate::cli::parse_query(&raw).map_err(|fail| fail.message)?);
            }
            Self::History(value) => options.history = *value,
            Self::All(value) => options.all = *value,
            Self::Summary(value) => options.summary = *value,
            Self::NoExtended(value) => options.extended = !value,
            Self::NoInputs(value) => options.inputs = !value,
            Self::NoAssets(value) => options.assets = !value,
            Self::Scripts(value) => options.scripts = *value,
            Self::Post(value) => options.post = *value,
        }
        Ok(())
    }
}

fn number(
    what: &str,
    template: &Template,
    bindings: &HashMap<String, String>,
) -> Result<usize, String> {
    let raw = template.fill(bindings)?;
    raw.parse()
        .map_err(|_| format!("{what} {raw} is not a number"))
}
