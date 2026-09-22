//! Finding the playbook root and reading the data tree.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::frontmatter::{parse_conf, split_document, split_list, Split};
use crate::model::{split_sections, Diagnostic, Layer, Project, Rule, Section, Severity, Target};

const PROJECT_KEYS: &[&str] = &[
    "project",
    "path",
    "org",
    "ecosystems",
    "languages",
    "default_target",
];
const TARGET_KEYS: &[&str] = &[
    "target",
    "model",
    "harness",
    "title",
    "include",
    "exclude",
    "exclude_activation",
    "emphasis",
    "addenda",
    "memory",
    "default_file",
    "instruction_files",
];
const RULE_KEYS: &[&str] = &[
    "id",
    "title",
    "layer",
    "activation",
    "priority",
    "overrides",
    "targets",
];

/// How the root was found, so a caller can say which tree it resolved against.
///
/// It matters because a verdict reads the same either way: `check` against the packaged copy and
/// `check` against the checkout you are editing both print `ok`, and only one of them is about the
/// rules you just changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootSource {
    /// `--root <path>`
    Named,
    /// `PLAYBOOK_ROOT`
    Environment,
    /// An ancestor of the working directory: the checkout the caller is standing in.
    WorkingDirectory,
    /// Beside the installed binary, in `share/playbook` — the copy this binary was packaged with.
    Installed,
    /// The directory this binary was compiled in, which is a checkout under `cargo run`.
    BuildDirectory,
}

impl RootSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            RootSource::Named => "--root",
            RootSource::Environment => "PLAYBOOK_ROOT",
            RootSource::WorkingDirectory => "the working directory",
            RootSource::Installed => "the packaged copy",
            RootSource::BuildDirectory => "the build directory",
        }
    }

    /// Whether this is the copy that shipped with the binary rather than a tree the caller is
    /// standing in or named.
    pub fn is_packaged(&self) -> bool {
        matches!(self, RootSource::Installed)
    }
}

/// Locate the playbook root, and say how.
///
/// Order: an explicit `--root`, `PLAYBOOK_ROOT`, the nearest ancestor of the working directory
/// that looks like the playbook, then the data an installed binary carries beside itself, then the
/// directory this binary was built in. The last two are for a binary invoked from somewhere else
/// entirely; the build directory is last because in a packaged build it is a directory that no
/// longer exists, so naming it is the honest failure.
pub fn find_root_with_source(explicit: Option<&Path>) -> Result<(PathBuf, RootSource), String> {
    if let Some(dir) = explicit {
        return validate_root(dir).map(|dir| (dir, RootSource::Named));
    }
    if let Some(dir) = std::env::var_os("PLAYBOOK_ROOT") {
        return validate_root(Path::new(&dir)).map(|dir| (dir, RootSource::Environment));
    }
    if let Ok(cwd) = std::env::current_dir() {
        for candidate in cwd.ancestors() {
            if looks_like_root(candidate) {
                return Ok((candidate.to_path_buf(), RootSource::WorkingDirectory));
            }
        }
    }
    if let Some(dir) = packaged_root() {
        return Ok((dir, RootSource::Installed));
    }
    validate_root(Path::new(env!("CARGO_MANIFEST_DIR")))
        .map(|dir| (dir, RootSource::BuildDirectory))
}

/// The same, without the provenance.
pub fn find_root(explicit: Option<&Path>) -> Result<PathBuf, String> {
    find_root_with_source(explicit).map(|(dir, _)| dir)
}

/// The data tree an installed binary carries beside itself: `<exe dir>/../share/playbook`.
///
/// The compile-time manifest directory is not this — it is the build directory, which `nix build`
/// deletes — so without this a packaged composer cannot answer at all, and `playbook check` from a
/// devshell would be a command that only ever errors. Canonicalised so the path it prints names the
/// store generation rather than a `bin/..` that depends on which symlink you came through.
fn packaged_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let candidate = exe.parent()?.join("../share/playbook");
    looks_like_root(&candidate)
        .then(|| std::fs::canonicalize(&candidate).ok())
        .flatten()
}

fn looks_like_root(dir: &Path) -> bool {
    dir.join("rules").is_dir() && dir.join("models").is_dir() && dir.join("projects").is_dir()
}

fn validate_root(dir: &Path) -> Result<PathBuf, String> {
    if !looks_like_root(dir) {
        return Err(format!(
            "{} is not a playbook root (expected rules/, models/ and projects/)",
            dir.display()
        ));
    }
    Ok(dir.to_path_buf())
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Directory names under `dir`, sorted. Missing directories are an empty list.
pub fn subdirs(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    names
}

pub fn list_projects(root: &Path) -> Vec<String> {
    subdirs(&root.join("projects"))
}

pub fn list_targets(root: &Path) -> Vec<String> {
    subdirs(&root.join("models"))
}

pub fn load_project(root: &Path, name: &str) -> Result<Project, String> {
    let path = root.join("projects").join(name).join("project.conf");
    if !path.is_file() {
        return Err(format!(
            "no such project: {name} (expected {})",
            path.display()
        ));
    }
    let conf = parse_conf(&read(&path)?);
    let mut diagnostics = Vec::new();
    warn_unknown_keys(&conf, PROJECT_KEYS, &path, &mut diagnostics);

    Ok(Project {
        name: name.to_string(),
        path: conf.get("path").cloned().unwrap_or_default(),
        org: conf.get("org").cloned().filter(|s| !s.is_empty()),
        ecosystems: conf
            .get("ecosystems")
            .map(|v| split_list(v))
            .unwrap_or_default(),
        languages: conf
            .get("languages")
            .map(|v| split_list(v))
            .unwrap_or_default(),
        default_target: conf
            .get("default_target")
            .cloned()
            .filter(|s| !s.is_empty()),
    })
}

/// `~/x` to `$HOME/x`. A project names its checkout the way a person writes it, which is with a
/// tilde.
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

/// The project that claims `dir` — or the nearest ancestor of it that a project claims — with the
/// directory it claimed.
///
/// Discovery runs from the repository rather than from a list of names: the caller is standing in
/// the checkout, and `path:` is the one thing linking a project to it. Walking upwards is what makes
/// a command run from a subdirectory resolve, which matters because a shell is usually in one — and
/// the directory returned is the repository root, not wherever the command happened to be run.
pub fn claiming_project(root: &Path, dir: &Path) -> Result<Option<(Project, PathBuf)>, String> {
    let dir = real(dir);
    for ancestor in dir.ancestors() {
        for name in list_projects(root) {
            let project = load_project(root, &name)?;
            if project.path.is_empty() {
                continue;
            }
            if real(&expand_tilde(&project.path)) == ancestor {
                return Ok(Some((project, ancestor.to_path_buf())));
            }
        }
    }
    Ok(None)
}

/// Canonical where the path exists, verbatim where it does not.
///
/// A project whose checkout is not on this machine — or whose `path:` points at a directory the
/// caller has not created yet — must not make the others unfindable, and comparing a symlinked
/// path against the real one would do exactly that.
fn real(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub fn load_target(root: &Path, name: &str) -> Result<Target, String> {
    let path = root.join("models").join(name).join("overlay.conf");
    if !path.is_file() {
        return Err(format!(
            "no such target: {name} (expected {})",
            path.display()
        ));
    }
    let conf = parse_conf(&read(&path)?);
    let mut diagnostics = Vec::new();
    warn_unknown_keys(&conf, TARGET_KEYS, &path, &mut diagnostics);

    let list = |key: &str| conf.get(key).map(|v| split_list(v)).unwrap_or_default();

    Ok(Target {
        name: name.to_string(),
        model: conf.get("model").cloned().unwrap_or_default(),
        harness: conf.get("harness").cloned().unwrap_or_default(),
        title: conf
            .get("title")
            .cloned()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Agent rules".to_string()),
        include: list("include"),
        exclude: list("exclude"),
        exclude_activation: list("exclude_activation"),
        emphasis: list("emphasis"),
        addenda: list("addenda"),
        memory: list("memory"),
        default_file: conf
            .get("default_file")
            .cloned()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "AGENTS.md".to_string()),
        instruction_files: list("instruction_files"),
    })
}

fn warn_unknown_keys(
    conf: &BTreeMap<String, String>,
    known: &[&str],
    path: &Path,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for key in conf.keys() {
        if !known.contains(&key.as_str()) {
            diagnostics.push(Diagnostic::warning(format!(
                "{}: unknown key `{key}` — a typo here reads as a missing setting",
                path.display()
            )));
        }
    }
}

/// Read every rule under `rules/` and `projects/*/rules/`.
pub fn load_rules(root: &Path) -> (Vec<Rule>, Vec<Diagnostic>) {
    let mut files = Vec::new();
    collect_markdown(&root.join("rules"), &mut files);
    for project in list_projects(root) {
        collect_markdown(
            &root.join("projects").join(&project).join("rules"),
            &mut files,
        );
    }
    files.sort();

    let mut rules = Vec::new();
    let mut diagnostics = Vec::new();

    for path in files {
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .into_owned();
        // Both arms carry diagnostics: a rule can parse successfully *and* have a problem
        // worth reporting (an unknown key, an empty body). Discarding them on the Ok arm is
        // how a typo'd key becomes a silently missing setting.
        let (rule, diags) = parse_rule(&rel, &path);
        diagnostics.extend(diags);
        if let Some(rule) = rule {
            rules.push(rule);
        }
    }

    // A duplicate id makes `overrides:` and `emphasis:` ambiguous, so it is fatal.
    let mut seen: BTreeMap<&str, &str> = BTreeMap::new();
    for rule in &rules {
        if let Some(previous) = seen.insert(rule.id.as_str(), rule.rel.as_str()) {
            diagnostics.push(Diagnostic::error(format!(
                "duplicate rule id `{}` — declared in both {previous} and {}",
                rule.id, rule.rel
            )));
        }
    }

    (rules, diagnostics)
}

fn collect_markdown(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect_markdown(&path, out);
        } else if path.extension().map(|e| e == "md").unwrap_or(false) {
            if path.file_name().map(|n| n == "README.md").unwrap_or(false) {
                continue;
            }
            out.push(path);
        }
    }
}

fn required_field(
    rel: &str,
    split: &Split<'_>,
    key: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    let value = split.get(key);
    if value.is_empty() {
        diagnostics.push(Diagnostic::error(format!(
            "{rel}: `{key}:` is required and is missing or empty"
        )));
        None
    } else {
        Some(value.to_string())
    }
}

fn parse_rule(rel: &str, path: &Path) -> (Option<Rule>, Vec<Diagnostic>) {
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return (
                None,
                vec![Diagnostic::error(format!("{rel}: cannot read ({e})"))],
            )
        }
    };
    let split = split_document(&source);
    let mut diagnostics = Vec::new();

    for key in split.fields.keys() {
        if !RULE_KEYS.contains(&key.as_str()) {
            diagnostics.push(Diagnostic::warning(format!("{rel}: unknown key `{key}`")));
        }
    }

    let id = required_field(rel, &split, "id", &mut diagnostics);
    let title = required_field(rel, &split, "title", &mut diagnostics);

    let layer = required_field(rel, &split, "layer", &mut diagnostics).and_then(|raw| {
        match Layer::parse(&raw) {
            Ok(layer) => Some(layer),
            Err(e) => {
                diagnostics.push(Diagnostic::error(format!("{rel}: {e}")));
                None
            }
        }
    });

    let activation = required_field(rel, &split, "activation", &mut diagnostics).and_then(|raw| {
        match crate::model::Activation::parse(&raw) {
            Ok(activation) => Some(activation),
            Err(e) => {
                diagnostics.push(Diagnostic::error(format!("{rel}: {e}")));
                None
            }
        }
    });

    let priority = match split.get("priority") {
        "" => Some(50),
        raw => match raw.parse::<i32>() {
            Ok(priority) => Some(priority),
            Err(_) => {
                diagnostics.push(Diagnostic::error(format!(
                    "{rel}: priority `{raw}` is not an integer"
                )));
                None
            }
        },
    };

    let sections = split_sections(split.body);
    if sections.directive.is_empty() {
        diagnostics.push(Diagnostic::error(format!(
            "{rel}: rule has no directive — a heading with no text cannot constrain anything"
        )));
    }

    // A missing `## Directive` is *not* reported here. Format conformance is a test-suite
    // concern in this repository (see `every_rule_has_a_directive_section`), not a runtime
    // one: emitting one warning per unmigrated rule on every invocation buries the warnings
    // that are actually about this run, and a wall of warnings is how you teach someone to
    // ignore them.

    match (id, title, layer, activation, priority) {
        (Some(id), Some(title), Some(layer), Some(activation), Some(priority))
            if !sections.directive.is_empty() =>
        {
            (
                Some(Rule {
                    id,
                    title,
                    layer,
                    activation,
                    priority,
                    overrides: split.list("overrides"),
                    targets: split.list("targets"),
                    rel: rel.to_string(),
                    directive: sections.directive,
                    rationale: sections.rationale,
                }),
                diagnostics,
            )
        }
        _ => (None, diagnostics),
    }
}

pub fn load_addendum(root: &Path, target: &str, name: &str) -> Result<Section, String> {
    let path = root.join("models").join(target).join("addenda").join(name);
    let body = read(&path)?;
    Ok(Section {
        name: name.to_string(),
        body,
    })
}

/// Read a file from the `memory/` layer.
pub fn load_memory(root: &Path, name: &str) -> Result<Section, String> {
    let path = root.join("memory").join(name);
    let body = read(&path)?;
    Ok(Section {
        name: name.to_string(),
        body,
    })
}

/// Everything the `rules` subcommand needs to report orphans.
pub struct RuleCensus {
    pub rule: Rule,
    /// Projects for which this rule activates.
    pub projects: Vec<String>,
}

pub fn census(root: &Path) -> Result<Vec<RuleCensus>, String> {
    let (rules, _) = load_rules(root);
    let mut out = Vec::new();
    for rule in rules {
        let mut projects = Vec::new();
        for name in list_projects(root) {
            let project = load_project(root, &name)?;
            if rule.activation.matches(&project) {
                projects.push(name);
            }
        }
        out.push(RuleCensus { rule, projects });
    }
    Ok(out)
}

/// True when the rule is reachable from some target's `include:` list.
pub fn included_by_some_target(root: &Path, rule: &Rule) -> bool {
    list_targets(root).into_iter().any(|name| {
        load_target(root, &name)
            .map(|t| {
                t.include
                    .iter()
                    .any(|i| i == &rule.id || rule.rel.starts_with(i.as_str()))
            })
            .unwrap_or(false)
    })
}

/// Report a severity so `main` can decide the exit code.
pub fn has_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics.iter().any(|d| d.severity == Severity::Error)
}
