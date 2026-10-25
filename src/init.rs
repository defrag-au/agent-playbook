//! Scaffolding a repository's `.playbook/` manifest.
//!
//! `init` only ever creates; it refuses to overwrite a manifest, because the manifest is the
//! repository's declaration of its own identity and replacing it silently would change which
//! shared rules apply.

use std::fs;
use std::path::{Path, PathBuf};

const RULES_README: &str = "\
# Project rules

Rules in this directory apply to this repository only. The composer reads them after the shared
tree, and a rule here must declare:

```conf
layer: project
activation: project:<name>
```

`<name>` is the `project:` value in `../project.conf`. The check is deliberate: a rule copied from
another repository, or a manifest renamed without its rules, fails loudly instead of never loading.

Copy `templates/rule.md` from agent-playbook for the full frontmatter. If a rule becomes true of a
second repository, move it up to the shared tree rather than copying it here.
";

/// Create `<repo>/.playbook/project.conf` and `<repo>/.playbook/rules/`.
///
/// Returns the paths created, so the caller can print them. Refuses to run if a manifest already
/// exists.
pub fn scaffold(repo: &Path, project: &str, target: &str) -> Result<Vec<PathBuf>, String> {
    if project.trim().is_empty() {
        return Err("`project:` cannot be empty".to_string());
    }
    if !repo.is_dir() {
        return Err(format!("no such directory: {}", repo.display()));
    }

    let dir = repo.join(".playbook");
    let manifest = dir.join("project.conf");
    if manifest.exists() {
        return Err(format!(
            "{} already exists — refusing to overwrite it",
            manifest.display()
        ));
    }

    let rules = dir.join("rules");
    fs::create_dir_all(&rules).map_err(|e| format!("{}: {e}", rules.display()))?;

    let body = format!(
        "project: {project}\n\
         org:\n\
         ecosystems:\n\
         renders:\n\
         languages:\n\
         default_target: {target}\n"
    );
    fs::write(&manifest, body).map_err(|e| format!("{}: {e}", manifest.display()))?;

    let readme = rules.join("README.md");
    fs::write(&readme, RULES_README).map_err(|e| format!("{}: {e}", readme.display()))?;

    Ok(vec![manifest, rules, readme])
}
