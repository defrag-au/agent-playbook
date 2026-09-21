//! The vocabulary: what a rule, a project and a target are.

use std::fmt;
use std::path::PathBuf;

/// When a rule is included in a resolved set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Activation {
    /// Every model, every language, every repo.
    Always,
    /// Only when the project declares the language.
    Language(String),
    /// Only when the project declares the org.
    Org(String),
    /// Exactly one project.
    Project(String),
    /// Never automatic — a target must name the rule in its `include:`.
    Manual,
}

impl Activation {
    pub fn parse(raw: &str) -> Result<Self, String> {
        let raw = raw.trim();
        match raw {
            "" | "always" => Ok(Activation::Always),
            "manual" => Ok(Activation::Manual),
            _ => {
                if let Some(rest) = raw.strip_prefix("language:") {
                    return non_empty(rest, raw).map(Activation::Language);
                }
                if let Some(rest) = raw.strip_prefix("org:") {
                    return non_empty(rest, raw).map(Activation::Org);
                }
                if let Some(rest) = raw.strip_prefix("project:") {
                    return non_empty(rest, raw).map(Activation::Project);
                }
                Err(format!(
                    "unknown activation `{raw}` — expected always, manual, \
                     language:<name>, org:<name> or project:<name>"
                ))
            }
        }
    }

    /// Does this rule activate for `project`?
    pub fn matches(&self, project: &Project) -> bool {
        match self {
            Activation::Always => true,
            Activation::Manual => false,
            Activation::Language(lang) => project.languages.iter().any(|l| l == lang),
            Activation::Org(org) => project.org.as_deref() == Some(org.as_str()),
            Activation::Project(name) => &project.name == name,
        }
    }

    pub fn as_str(&self) -> String {
        match self {
            Activation::Always => "always".into(),
            Activation::Manual => "manual".into(),
            Activation::Language(l) => format!("language:{l}"),
            Activation::Org(o) => format!("org:{o}"),
            Activation::Project(p) => format!("project:{p}"),
        }
    }
}

fn non_empty(value: &str, whole: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("`{whole}` names nothing"));
    }
    Ok(value.to_string())
}

/// Where a rule sits in the precedence order.
///
/// The layer name in frontmatter is one of `core`, `org`, `project` or
/// `language:<name>` — spelled out rather than bare (`language:rust`, not `rust`) so
/// that a typo fails to parse instead of silently sorting into the wrong place.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    Core,
    Language(String),
    Org,
    Project,
}

impl Layer {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim() {
            "core" => Ok(Layer::Core),
            "org" => Ok(Layer::Org),
            "project" => Ok(Layer::Project),
            other => match other.strip_prefix("language:") {
                Some(name) if !name.trim().is_empty() => {
                    Ok(Layer::Language(name.trim().to_string()))
                }
                _ => Err(format!(
                    "unknown layer `{other}` — expected core, org, project or language:<name>"
                )),
            },
        }
    }

    pub fn rank(&self) -> u8 {
        match self {
            Layer::Core => 10,
            Layer::Language(_) => 20,
            Layer::Org => 30,
            Layer::Project => 40,
        }
    }

    pub fn as_str(&self) -> String {
        match self {
            Layer::Core => "core".into(),
            Layer::Language(l) => format!("language:{l}"),
            Layer::Org => "org".into(),
            Layer::Project => "project".into(),
        }
    }
}

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_str())
    }
}

/// One standing constraint.
///
/// The body is split into two sections, and the split is the point: `directive` is what the
/// compiler emits, `rationale` is why the rule exists and stays at source. The compiled block
/// is optimised for an agent's attention budget; the argument for the rule is optimised for
/// the person deciding whether to keep it.
#[derive(Debug, Clone)]
pub struct Rule {
    pub id: String,
    pub title: String,
    pub layer: Layer,
    pub activation: Activation,
    pub priority: i32,
    pub overrides: Vec<String>,
    pub targets: Vec<String>,
    /// Path relative to the playbook root, e.g. `rules/core/working-first.md`.
    pub rel: String,
    /// The terse direction. Emitted.
    pub directive: String,
    /// The incident, the measurement, the history. Not emitted.
    pub rationale: String,
}

impl Rule {
    /// The actionable lead, repeated verbatim by a target's `emphasis:`.
    ///
    /// Directives are written two ways — as a prose lead paragraph, or as a bullet list — and
    /// the lead has to be the same thing in both cases: the first complete instruction. For a
    /// bullet list that is the first bullet plus its continuation lines, not the whole list
    /// run together into a paragraph. (It was the whole list, briefly: a directive opening on
    /// a bullet produced a 400-word single line in the preamble.)
    pub fn lead(&self) -> String {
        let lines: Vec<&str> = self.directive.lines().collect();
        let Some(start) = lines.iter().position(|line| !line.trim().is_empty()) else {
            return String::new();
        };

        let first = lines[start].trim();
        let bullet = first.starts_with("- ") || first.starts_with("* ");
        let first = first
            .strip_prefix("- ")
            .or_else(|| first.strip_prefix("* "))
            .unwrap_or(first);

        let mut out = String::from(first);
        for line in &lines[start + 1..] {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                break;
            }
            let continues = if bullet {
                // Inside a bullet: continuations are indented, including nested bullets.
                line.starts_with("  ") || line.starts_with('\t')
            } else {
                // Inside a prose paragraph: stop at the first bullet.
                !(trimmed.starts_with("- ") || trimmed.starts_with("* "))
            };
            if !continues {
                break;
            }
            out.push(' ');
            out.push_str(trimmed);
        }
        out
    }

    pub fn short_id(&self) -> &str {
        self.id.as_str()
    }
}

/// Split a rule body into its directive and rationale sections.
///
/// `## Directive` runs to `## Rationale` (or the end of the file); anything before
/// `## Directive` is prepended to the directive rather than dropped, so a stray preamble
/// costs verbosity instead of losing content. A body with no `## Directive` at all is
/// returned whole, with `has_directive` false so the loader can warn — the test suite is
/// what makes it a hard failure.
pub struct Sections {
    pub directive: String,
    pub rationale: String,
    pub has_directive: bool,
}

pub fn split_sections(body: &str) -> Sections {
    let mut directive: Vec<&str> = Vec::new();
    let mut rationale: Vec<&str> = Vec::new();
    let mut has_directive = false;
    let mut in_rationale = false;

    for line in body.lines() {
        if is_section(line, "directive") {
            has_directive = true;
            continue;
        }
        if is_section(line, "rationale") {
            in_rationale = true;
            continue;
        }
        if in_rationale {
            rationale.push(line);
        } else {
            directive.push(line);
        }
    }

    Sections {
        directive: trim_blank(&directive),
        rationale: trim_blank(&rationale),
        has_directive,
    }
}

fn is_section(line: &str, name: &str) -> bool {
    let Some(rest) = line.trim().strip_prefix("## ") else {
        return false;
    };
    rest.trim().eq_ignore_ascii_case(name)
}

fn trim_blank(lines: &[&str]) -> String {
    let start = lines.iter().position(|l| !l.trim().is_empty());
    let Some(start) = start else {
        return String::new();
    };
    let end = lines
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .unwrap_or(start);
    lines[start..=end].join("\n")
}

/// A repository the playbook knows about.
#[derive(Debug, Clone, Default)]
pub struct Project {
    pub name: String,
    pub path: String,
    pub org: Option<String>,
    pub languages: Vec<String>,
    pub default_target: Option<String>,
}

/// A consumer of rules — usually a model+harness pair.
#[derive(Debug, Clone, Default)]
pub struct Target {
    pub name: String,
    pub model: String,
    pub harness: String,
    /// The heading for the generated block. Defaults to `Agent rules`.
    pub title: String,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    /// Activation kinds to drop, e.g. `always`.
    ///
    /// For a target whose rules are delivered somewhere else. The personal instructions file
    /// is the worked case: the universal rules ship in each repository's block, so repeating
    /// them in a file that loads for every project is duplication with no reader.
    pub exclude_activation: Vec<String>,
    pub emphasis: Vec<String>,
    pub addenda: Vec<String>,
    /// Files under `memory/`, emitted after the rules.
    pub memory: Vec<String>,
    pub default_file: String,
    /// The harness's instruction-file priority order, most significant first.
    ///
    /// Only meaningful for harnesses that read a single file — see
    /// [`crate::install::shadowing_files`]. Empty means the harness merges every file it
    /// finds, so nothing can shadow anything.
    pub instruction_files: Vec<String>,
}

/// A block of prose appended to the generated output — an addendum, or a memory file.
///
/// One shape for both, because they are the same thing mechanically: a markdown file from a
/// directory, demoted one heading level and appended. They differ only in what they are for
/// and where they sit in the output.
#[derive(Debug, Clone)]
pub struct Section {
    pub name: String,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Error,
            message: message.into(),
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Warning,
            message: message.into(),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        write!(f, "{label}: {}", self.message)
    }
}

/// A rule plus where it came from on disk.
#[derive(Debug, Clone)]
pub struct RuleFile {
    pub path: PathBuf,
    pub rel: String,
    pub source: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> Project {
        Project {
            name: "shared-crates".into(),
            path: "~/code/defrag/shared-crates".into(),
            org: Some("defrag".into()),
            languages: vec!["rust".into()],
            default_target: Some("claude-code".into()),
        }
    }

    #[test]
    fn activation_parses_every_variant() {
        assert_eq!(Activation::parse("always").unwrap(), Activation::Always);
        assert_eq!(Activation::parse("").unwrap(), Activation::Always);
        assert_eq!(Activation::parse("manual").unwrap(), Activation::Manual);
        assert_eq!(
            Activation::parse("language:rust").unwrap(),
            Activation::Language("rust".into())
        );
        assert_eq!(
            Activation::parse("org:defrag").unwrap(),
            Activation::Org("defrag".into())
        );
        assert_eq!(
            Activation::parse("project:archivist").unwrap(),
            Activation::Project("archivist".into())
        );
    }

    #[test]
    fn unknown_activation_is_an_error_not_a_silent_skip() {
        // A typo'd activation means a rule silently never applies, which is the exact
        // failure mode this repo exists to remove.
        assert!(Activation::parse("langauge:rust").is_err());
        assert!(Activation::parse("language:").is_err());
        assert!(Activation::parse("org:").is_err());
    }

    #[test]
    fn activation_matching() {
        let p = project();
        assert!(Activation::Always.matches(&p));
        assert!(!Activation::Manual.matches(&p));
        assert!(Activation::Language("rust".into()).matches(&p));
        assert!(!Activation::Language("go".into()).matches(&p));
        assert!(Activation::Org("defrag".into()).matches(&p));
        assert!(!Activation::Org("hodlcroft".into()).matches(&p));
        assert!(Activation::Project("shared-crates".into()).matches(&p));
        assert!(!Activation::Project("archivist".into()).matches(&p));
    }

    #[test]
    fn a_bare_language_layer_does_not_parse() {
        // `layer: rust` was the earlier spelling. It has to fail loudly, because a bare
        // name is ambiguous between a language and a typo.
        assert!(Layer::parse("rust").is_err());
        assert_eq!(
            Layer::parse("language:rust").unwrap(),
            Layer::Language("rust".into())
        );
    }

    #[test]
    fn layer_ranks_order_core_before_project() {
        assert!(Layer::Core.rank() < Layer::Language("rust".into()).rank());
        assert!(Layer::Language("rust".into()).rank() < Layer::Org.rank());
        assert!(Layer::Org.rank() < Layer::Project.rank());
    }

    #[test]
    fn lead_takes_only_the_first_paragraph() {
        let rule = rule_with_directive("First line\nwraps.\n\nSecond paragraph.\n");
        assert_eq!(rule.lead(), "First line wraps.");
    }

    fn rule_with_directive(directive: &str) -> Rule {
        Rule {
            id: "x".into(),
            title: "T".into(),
            layer: Layer::Core,
            activation: Activation::Always,
            priority: 50,
            overrides: vec![],
            targets: vec![],
            rel: "rules/core/x.md".into(),
            directive: directive.into(),
            rationale: String::new(),
        }
    }

    #[test]
    fn lead_takes_the_first_bullet_and_its_continuations() {
        // The regression: a directive opening on a bullet used to collapse the entire list
        // into one line, because "first paragraph" found no blank line to stop at.
        let rule = rule_with_directive(
            "- First instruction,\n  which wraps.\n  - a nested detail\n- Second instruction.\n- Third.\n",
        );
        assert_eq!(
            rule.lead(),
            "First instruction, which wraps. - a nested detail"
        );
    }

    #[test]
    fn lead_stops_at_the_second_bullet() {
        let rule = rule_with_directive("- One.\n- Two.\n- Three.\n");
        assert_eq!(rule.lead(), "One.");
    }

    #[test]
    fn lead_stops_at_a_bullet_after_a_prose_lead() {
        // A rule may open with a sentence and then list — the lead is the sentence.
        let rule = rule_with_directive("Do the thing.\n\n- detail one\n- detail two\n");
        assert_eq!(rule.lead(), "Do the thing.");
    }

    #[test]
    fn lead_tolerates_a_leading_blank_line_and_a_star_bullet() {
        let rule = rule_with_directive("\n\n* Starred instruction.\n* Another.\n");
        assert_eq!(rule.lead(), "Starred instruction.");
    }

    #[test]
    fn lead_of_an_empty_directive_is_empty() {
        assert_eq!(rule_with_directive("\n\n").lead(), "");
    }

    #[test]
    fn sections_split_at_the_rationale_heading() {
        let body = "## Directive\n\nDo the thing.\n\n## Rationale\n\nBecause it broke.\n";
        let s = split_sections(body);
        assert!(s.has_directive);
        assert_eq!(s.directive, "Do the thing.");
        assert_eq!(s.rationale, "Because it broke.");
    }

    #[test]
    fn rationale_is_optional() {
        let s = split_sections("## Directive\n\nDo the thing.\n");
        assert!(s.has_directive);
        assert_eq!(s.directive, "Do the thing.");
        assert!(s.rationale.is_empty());
    }

    #[test]
    fn a_body_with_no_directive_is_returned_whole_and_flagged() {
        // The compiler warns on this; the test suite is what makes it fail. Nothing is
        // dropped either way — a rule that loses its text silently is worse than a long one.
        let s = split_sections("Just prose, no headings.\n\nMore prose.\n");
        assert!(!s.has_directive);
        assert!(s.directive.contains("Just prose"));
        assert!(s.directive.contains("More prose"));
    }

    #[test]
    fn content_before_the_directive_heading_is_kept_not_dropped() {
        let s = split_sections("A stray preamble.\n\n## Directive\n\nDo the thing.\n");
        assert!(s.has_directive);
        assert!(s.directive.contains("A stray preamble."));
        assert!(s.directive.contains("Do the thing."));
    }

    #[test]
    fn sub_headings_inside_a_directive_stay_in_the_directive() {
        // A directive may carry its own structure — the traps list does. Only `## Rationale`
        // ends it, so `###` and even `##` sub-headings do not silently truncate the rule.
        let body = "## Directive\n\nLead.\n\n### A trap\n\nDetails.\n\n## Rationale\n\nWhy.\n";
        let s = split_sections(body);
        assert!(s.directive.contains("### A trap"));
        assert!(s.directive.contains("Details."));
        assert_eq!(s.rationale, "Why.");
    }
}
