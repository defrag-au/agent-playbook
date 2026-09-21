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
    pub body: String,
}

impl Rule {
    /// The actionable lead: the first paragraph, as one line.
    pub fn lead(&self) -> String {
        let mut out = String::new();
        for line in self.body.lines() {
            if line.trim().is_empty() {
                if !out.is_empty() {
                    break;
                }
                continue;
            }
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(line.trim());
        }
        out
    }

    /// The id without its layer prefix, for display.
    pub fn short_id(&self) -> &str {
        self.id.as_str()
    }
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
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub emphasis: Vec<String>,
    pub addenda: Vec<String>,
    pub default_file: String,
}

/// A harness-specific addendum appended after the rules.
#[derive(Debug, Clone)]
pub struct Addendum {
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
        let rule = Rule {
            id: "x".into(),
            title: "T".into(),
            layer: Layer::Core,
            activation: Activation::Always,
            priority: 50,
            overrides: vec![],
            targets: vec![],
            rel: "rules/core/x.md".into(),
            body: "First line\nwraps.\n\nSecond paragraph.\n".into(),
        };
        assert_eq!(rule.lead(), "First line wraps.");
    }
}
