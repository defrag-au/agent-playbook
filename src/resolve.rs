//! The resolution engine.
//!
//! Collect low → high, drop what a higher layer supersedes, sort, then amplify for the
//! target. See `docs/precedence.md` for the model this implements.

use std::path::{Path, PathBuf};

use crate::load::{has_errors, load_addendum, load_project, load_rules, load_target};
use crate::model::{Activation, Addendum, Diagnostic, Layer, Project, Rule, Target};

pub struct Resolved {
    /// The playbook root this was resolved from. Carried so the rendered block can name
    /// it: the generated file lives in another repository, where a relative link into
    /// the playbook would be dead on arrival.
    pub root: PathBuf,
    pub project: Project,
    pub target: Target,
    /// Sorted: layer rank, then priority descending, then id.
    pub rules: Vec<Rule>,
    /// Ids removed because a higher layer named them in `overrides:`.
    pub superseded: Vec<String>,
    /// In overlay order, looked up from the resolved set.
    pub emphasis: Vec<Rule>,
    pub addenda: Vec<Addendum>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Resolved {
    pub fn has_errors(&self) -> bool {
        has_errors(&self.diagnostics)
    }
}

pub fn resolve(
    root: &Path,
    project_name: &str,
    target_name: Option<&str>,
) -> Result<Resolved, String> {
    let project = load_project(root, project_name)?;
    let target_name = match target_name {
        Some(t) => t.to_string(),
        None => project
            .default_target
            .clone()
            .ok_or_else(|| format!("no --target given and {project_name} has no default_target"))?,
    };
    let target = load_target(root, &target_name)?;

    let (all_rules, mut diagnostics) = load_rules(root);

    // --- include ------------------------------------------------------------
    let mut kept: Vec<Rule> = Vec::new();
    for rule in all_rules {
        if !rule.targets.is_empty() && !rule.targets.iter().any(|t| t == &target.name) {
            continue;
        }
        let included = match &rule.activation {
            Activation::Manual => target
                .include
                .iter()
                .any(|i| i == &rule.id || rule.rel.starts_with(i.as_str())),
            other => other.matches(&project),
        };
        if !included {
            continue;
        }
        if excluded(&rule, &target) {
            continue;
        }
        kept.push(rule);
    }

    // --- overrides ----------------------------------------------------------
    let mut superseded: Vec<String> = Vec::new();
    for rule in &kept {
        for overridden in &rule.overrides {
            let Some(victim) = kept.iter().find(|r| &r.id == overridden) else {
                diagnostics.push(Diagnostic::warning(format!(
                    "{}: overrides `{overridden}`, which is not in this rule set",
                    rule.rel
                )));
                continue;
            };
            // A rule may only supersede one in a strictly lower layer. Anything else is
            // an overlay quietly weakening a constraint, which is the thing the whole
            // precedence model exists to prevent.
            if victim.layer.rank() >= rule.layer.rank() {
                diagnostics.push(Diagnostic::error(format!(
                    "{}: overrides `{}` at layer {} — a rule may only override a rule in a \
                     lower layer ({} is {})",
                    rule.rel, victim.id, victim.layer, rule.rel, rule.layer
                )));
                continue;
            }
            // Mutual override resolves to an empty set, which is never intended.
            if victim.overrides.contains(&rule.id) {
                diagnostics.push(Diagnostic::error(format!(
                    "{} and {} override each other — neither would survive",
                    rule.rel, victim.rel
                )));
                continue;
            }
            if !superseded.contains(&victim.id) {
                superseded.push(victim.id.clone());
            }
        }
    }
    superseded.sort();
    kept.retain(|r| !superseded.contains(&r.id));

    // --- sort ---------------------------------------------------------------
    kept.sort_by(|a, b| {
        a.layer
            .rank()
            .cmp(&b.layer.rank())
            .then(b.priority.cmp(&a.priority))
            .then(a.id.cmp(&b.id))
    });

    // --- emphasis -----------------------------------------------------------
    let mut emphasis = Vec::new();
    for id in &target.emphasis {
        match kept.iter().find(|r| &r.id == id) {
            Some(rule) => emphasis.push(rule.clone()),
            None => diagnostics.push(Diagnostic::error(format!(
                "target `{}` emphasises `{id}`, which is not in this rule set",
                target.name
            ))),
        }
    }

    // --- addenda ------------------------------------------------------------
    let mut addenda = Vec::new();
    for name in &target.addenda {
        match load_addendum(root, &target.name, name) {
            Ok(a) => addenda.push(a),
            Err(e) => diagnostics.push(Diagnostic::error(format!(
                "target `{}`: addendum {name}: {e}",
                target.name
            ))),
        }
    }

    if kept.is_empty() {
        diagnostics.push(Diagnostic::warning(format!(
            "project `{project_name}` + target `{target_name}` resolves to zero rules"
        )));
    }

    Ok(Resolved {
        root: root.to_path_buf(),
        project,
        target,
        rules: kept,
        superseded,
        emphasis,
        addenda,
        diagnostics,
    })
}

fn excluded(rule: &Rule, target: &Target) -> bool {
    target
        .exclude
        .iter()
        .any(|e| e == &rule.id || rule.rel.starts_with(e.as_str()))
}

/// Is the rule's layer a language layer for a language the project declares?
pub fn layer_is_language(layer: &Layer) -> bool {
    matches!(layer, Layer::Language(_))
}
