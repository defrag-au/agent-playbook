//! Delivering OCFL-stored skills into an agent's skill directory.
//!
//! The store is the artifact; this is the delivery. A manifest declares `skills:` in its
//! `.playbook/project.conf`, `sync` exports each one through `skill-store` and copies the tree into
//! place, and `skills.lock` records exactly what was delivered, and where.
//!
//! Four decisions worth knowing before reading the code:
//!
//! - **The lock is the manifest, per target.** A directory under a skill root that the lock does
//!   not name *for that target* is unmanaged, and sync refuses to touch it rather than deleting
//!   something it did not write. Recording the target is what stops a zed run from claiming a
//!   directory only Claude Code has.
//! - **Two phases.** Every skill is exported first, then every tree is copied. A failing export
//!   therefore leaves the filesystem exactly as it was, rather than half-delivered.
//! - **`check` never writes.** It re-exports each source and compares: the lock against the
//!   envelope, and the delivered tree against the export, byte for byte. That needs no digest
//!   implementation here, which is what keeps this crate dependency-free.
//! - **One manifest, one destination list.** `skills_scope:` decides whether that list is the
//!   repository's own directories or the harness globals; `--target all` expands to every harness
//!   that declares one.
//!
//! `skill-store` is called as a subprocess — `SKILL_STORE` names the binary, default
//! `skill-store` on `PATH`. That is the same boundary `at-recall` draws around `git`: this crate
//! runs exactly one program, and only for the two commands that have to fetch.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::frontmatter::parse_conf;
use crate::load::expand_tilde;
use crate::model::{Project, SkillScope, Target};

/// The lock's filename inside `.playbook/`.
pub const LOCK_FILE: &str = "skills.lock";

/// A development overlay, beside the lock and gitignored by convention.
///
/// It maps a **skill name** to the source to use instead of the declared one, which is what lets
/// an edit be seen by a consumer before a version is published — and a branch be tried without
/// touching the manifest. It is a separate file rather than a key in `project.conf` so a machine
/// path cannot be committed by accident: CI has no such file, and therefore never sees an
/// override.
pub const PATCH_FILE: &str = "skills.patch";

/// One delivered skill, as the lock records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    /// The source exactly as the manifest declared it. It may name a head rather than a version;
    /// `version` is the version that was actually exported.
    pub source: String,
    pub version: String,
    pub tree_digest: String,
    /// The targets this skill was delivered for. Sorted.
    pub targets: Vec<String>,
}

/// One place a run delivers into — `repo/.agents/skills`, `~/.claude/skills`, …
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    /// The target that named it, for the report and for the lock.
    pub target: String,
    /// Absolute. Each skill lands at `<root>/<name>`.
    pub root: PathBuf,
}

/// Build the destination list for a run.
///
/// Empty directory keys are skipped: a model overlay is not a harness, and `all` must not fail
/// because of one. Naming such a target explicitly is an error, and the caller checks that first.
/// Two targets that name the same directory deliver once — a harness-specific target wins over
/// `generic`, whose `harness: any` makes it a baseline rather than a destination.
pub fn destinations(
    targets: &[(String, Target)],
    scope: SkillScope,
    repo: &Path,
) -> Vec<Destination> {
    let mut candidates: Vec<(bool, String, PathBuf)> = targets
        .iter()
        .filter_map(|(name, target)| {
            let dir = match scope {
                SkillScope::Repo => &target.skill_dir,
                SkillScope::Global => &target.skill_global_dir,
            };
            if dir.is_empty() {
                return None;
            }
            let root = match scope {
                SkillScope::Repo => repo.join(dir),
                SkillScope::Global => expand_tilde(dir),
            };
            Some((target.harness == "any", name.clone(), root))
        })
        .collect();
    candidates.sort();

    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    let mut out = Vec::new();
    for (_, target, root) in candidates {
        if seen.insert(root.clone()) {
            out.push(Destination { target, root });
        }
    }
    out
}

/// The skill a source names, from its `#` fragment.
///
/// `playbook` reads only this much of the grammar, because a declaration's *identity* is the
/// skill: that is what a patch targets, and what makes two declarations of the same skill an
/// error before anything is fetched. The rest of the source is opaque here and is resolved by
/// `skill-store`.
pub fn declared_name(source: &str) -> Option<&str> {
    let (_, fragment) = source.split_once('#')?;
    let name = fragment
        .split_once('@')
        .map(|(name, _)| name)
        .unwrap_or(fragment);
    (!name.is_empty()).then_some(name)
}

/// One declared skill, with the source that will actually be exported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    pub name: String,
    /// What the manifest says.
    pub declared: String,
    /// What is used — the declared source, unless a patch overrides it.
    pub effective: String,
    pub patched: bool,
}

/// Read `.playbook/skills.patch`. A missing file is no patch, not an error.
pub fn read_patch(dir: &Path) -> Result<BTreeMap<String, String>, String> {
    let path = dir.join(".playbook").join(PATCH_FILE);
    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };

    let mut patch = BTreeMap::new();
    for (number, line) in source.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let where_ = format!("{}:{}", path.display(), number + 1);
        let Some((name, value)) = line.split_once(':') else {
            return Err(format!("{where_}: expected `name: <source>`"));
        };
        let name = name.trim();
        validate_name(name).map_err(|error| format!("{where_}: {error}"))?;
        let value = value.trim();
        if value.is_empty() {
            return Err(format!("{where_}: `{name}` needs a source"));
        }
        if value.split_whitespace().count() != 1 {
            return Err(format!("{where_}: a source cannot contain whitespace"));
        }
        if patch.insert(name.to_string(), value.to_string()).is_some() {
            return Err(format!("{where_}: `{name}` is patched twice"));
        }
    }
    Ok(patch)
}

/// Pair every declared skill with the source that will be used, applying the patch.
///
/// A patch that matches nothing is an error: it means the file has drifted from the manifest, and
/// silently ignoring it would leave the caller believing a skill was overridden when it was not.
pub fn plan_sources(
    project: &Project,
    patch: &BTreeMap<String, String>,
) -> Result<Vec<Planned>, String> {
    let mut planned = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for source in &project.skills {
        if source.split_whitespace().count() != 1 {
            return Err(format!(
                "`{source}`: a skill source cannot contain whitespace — skills.lock is \
                 line-oriented"
            ));
        }
        let Some(name) = declared_name(source) else {
            return Err(format!(
                "`{source}`: a declared skill source must end with `#<skill>` — that is the name \
                 the lock and any patch refer to"
            ));
        };
        validate_name(name)?;
        if !seen.insert(name.to_string()) {
            return Err(format!(
                "`{name}` is declared twice — two sources cannot deliver the same skill name"
            ));
        }
        planned.push(Planned {
            name: name.to_string(),
            declared: source.clone(),
            effective: patch.get(name).cloned().unwrap_or_else(|| source.clone()),
            patched: patch.contains_key(name),
        });
    }
    for name in patch.keys() {
        if !seen.contains(name) {
            return Err(format!(
                "`{name}` is in .playbook/skills.patch but not declared in `skills:` — a patch \
                 that applies to nothing is a mistake, so it is an error rather than a silent \
                 no-op"
            ));
        }
    }
    Ok(planned)
}

/// `.playbook/skills.lock` — the record of what was delivered, keyed by skill name.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Lock {
    pub entries: BTreeMap<String, Entry>,
}

impl Lock {
    /// Read the lock. A missing file is an empty lock, not an error: a repository that has never
    /// synced has delivered nothing.
    pub fn read(path: &Path) -> Result<Self, String> {
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Lock::default())
            }
            Err(error) => return Err(format!("{}: {error}", path.display())),
        };
        Self::parse(&source).map_err(|error| format!("{}: {error}", path.display()))
    }

    /// `name: <source> <version> <tree digest> <targets>`, one skill per line. Comments and blank
    /// lines are skipped; a duplicate name or a malformed line is an error, because a lock that
    /// says two things is not a lock.
    pub fn parse(source: &str) -> Result<Self, String> {
        let mut entries = BTreeMap::new();
        for (number, line) in source.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let where_ = format!("line {}", number + 1);
            let Some((name, rest)) = line.split_once(':') else {
                return Err(format!(
                    "{where_}: expected `name: <source> <version> <digest> <targets>`"
                ));
            };
            let name = name.trim();
            validate_name(name).map_err(|error| format!("{where_}: {error}"))?;
            let fields: Vec<&str> = rest.split_whitespace().collect();
            let [source, version, tree_digest, targets] = fields.as_slice() else {
                return Err(format!(
                    "{where_}: expected `<source> <version> <tree digest> <targets>`, got {} \
                     field(s)",
                    fields.len()
                ));
            };
            let mut targets: Vec<String> = targets
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect();
            if targets.is_empty() {
                return Err(format!("{where_}: no targets listed"));
            }
            targets.sort();
            targets.dedup();
            let entry = Entry {
                name: name.to_string(),
                source: (*source).to_string(),
                version: (*version).to_string(),
                tree_digest: (*tree_digest).to_string(),
                targets,
            };
            if entries.insert(name.to_string(), entry).is_some() {
                return Err(format!("{where_}: `{name}` appears twice"));
            }
        }
        Ok(Lock { entries })
    }

    pub fn render(&self) -> String {
        let mut out = String::from(
            "# Generated by `playbook skills sync` — do not edit.\n\
             # <skill>: <source> <version> <tree digest> <targets>\n",
        );
        for entry in self.entries.values() {
            out.push_str(&format!(
                "{}: {} {} {} {}\n",
                entry.name,
                entry.source,
                entry.version,
                entry.tree_digest,
                entry.targets.join(",")
            ));
        }
        out
    }
}

/// What happened to one skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// `sync` delivered it.
    Written,
    /// `sync` found the delivered tree already identical.
    Unchanged,
    /// `check` found the lock, the source and the delivered tree in agreement.
    Ok,
    /// The source has moved, or the delivered copy no longer matches the export.
    Stale,
    /// The skill is declared but was never delivered.
    Missing,
    /// The lock names a skill the manifest no longer declares.
    Orphan,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Written => "written",
            Verdict::Unchanged => "unchanged",
            Verdict::Ok => "ok",
            Verdict::Stale => "stale",
            Verdict::Missing => "missing",
            Verdict::Orphan => "orphan",
        }
    }

    fn is_failure(self) -> bool {
        matches!(self, Verdict::Stale | Verdict::Missing | Verdict::Orphan)
    }
}

#[derive(Debug, Clone)]
pub struct Line {
    pub name: String,
    /// The target that named the destination, for the report.
    pub target: String,
    /// This skill came from `.playbook/skills.patch`, not from the manifest's source.
    pub patched: bool,
    pub verdict: Verdict,
    pub detail: String,
}

#[derive(Debug, Default)]
pub struct Report {
    pub lines: Vec<Line>,
    pub warnings: Vec<String>,
}

impl Report {
    /// Whether every line is a result the caller should accept. Warnings never fail a run: they
    /// name something left in place, which is a thing to decide rather than an error.
    pub fn ok(&self) -> bool {
        !self.lines.iter().any(|line| line.verdict.is_failure())
    }
}

/// One exported skill, as `skill-store` left it.
struct Envelope {
    entry: Entry,
    files: usize,
    tree: PathBuf,
}

/// A scratch directory that removes itself.
///
/// It has to outlive the copy, not the export: deleting it when `export_all` returned left every
/// envelope pointing at a tree that was already gone.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new() -> Result<Self, String> {
        let root = std::env::temp_dir().join(format!(
            "playbook-skills-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).map_err(|e| format!("{}: {e}", root.display()))?;
        Ok(Scratch { root })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Export every declared skill into a scratch tree, then copy each one into every destination.
///
/// `lock_dir` is where `.playbook/skills.lock` lives: the repository for a repo-scoped run, the
/// manifest's directory for a global one — the lock records what *this manifest* delivered, not
/// what the directory it happens to sit in delivered.
pub fn sync(
    lock_dir: &Path,
    project: &Project,
    destinations: &[Destination],
    offline: bool,
    prune: bool,
) -> Result<Report, String> {
    let lock_path = lock_path(lock_dir);
    let previous = Lock::read(&lock_path)?;
    let patch = read_patch(lock_dir)?;
    let planned = plan_sources(project, &patch)?;
    let (_scratch, exports) = export_all(&planned, offline)?;

    // Refuse before writing anything: an unmanaged directory at a declared name is either a
    // hand-installed skill or a name collision, and both need a decision rather than a overwrite.
    let declared: Vec<String> = planned.iter().map(|plan| plan.name.clone()).collect();
    let mut collisions = Vec::new();
    for destination in destinations {
        collisions.extend(unmanaged_directories(
            &destination.root,
            &declared,
            &previous,
            &destination.target,
        ));
    }
    if !collisions.is_empty() {
        // Say which kind of collision each one is. The first global sync meets hand-installed
        // copies, and "identical" and "stale" call for the same action but not the same
        // confidence: one is a no-op adoption, the other is replacing an out-of-date procedure.
        let listed: Vec<String> = collisions
            .iter()
            .map(|path| {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default();
                let note = exports
                    .iter()
                    .find(|(_, envelope)| envelope.entry.name == name)
                    .and_then(|(_, envelope)| {
                        first_difference(path, &envelope.tree)
                            .ok()
                            .map(|difference| match difference {
                                None => format!(
                                    " (identical to {} — deleting it loses nothing)",
                                    envelope.entry.version
                                ),
                                Some(_) => " (differs from the published version — it is a \
                                             stale copy)"
                                    .to_string(),
                            })
                    })
                    .unwrap_or_default();
                format!("  {}{note}", path.display())
            })
            .collect();
        return Err(format!(
            "refusing to overwrite {} unmanaged skill director{}:\n{}\nMove {} out of the way, \
             or delete {} so the next sync records the delivered version.",
            collisions.len(),
            if collisions.len() == 1 { "y" } else { "ies" },
            listed.join("\n"),
            if collisions.len() == 1 { "it" } else { "them" },
            if collisions.len() == 1 { "it" } else { "them" },
        ));
    }

    let mut report = Report::default();
    let mut lock = Lock::default();
    let delivered: Vec<String> = destinations
        .iter()
        .map(|destination| destination.target.clone())
        .collect();

    for (plan, envelope) in &exports {
        let name = &plan.name;
        for destination in destinations {
            let path = destination.root.join(name);
            let same = path.is_dir() && first_difference(&path, &envelope.tree)?.is_none();
            if same {
                report.lines.push(Line {
                    name: name.clone(),
                    target: destination.target.clone(),
                    patched: plan.patched,
                    verdict: Verdict::Unchanged,
                    detail: format!("{} — {} file(s)", envelope.entry.version, envelope.files),
                });
            } else {
                remove_any(&path)?;
                copy_tree(&envelope.tree, &path)?;
                report.lines.push(Line {
                    name: name.clone(),
                    target: destination.target.clone(),
                    patched: plan.patched,
                    verdict: Verdict::Written,
                    detail: format!(
                        "{} — {} file(s) from {}",
                        envelope.entry.version, envelope.files, envelope.entry.source
                    ),
                });
            }
        }

        if let Some(entry) = entry_after(plan, envelope, &previous, &delivered) {
            lock.entries.insert(name.clone(), entry);
        }
    }

    // A skill that was declared before and is not now. The delivered directories are left alone
    // unless `--prune`, because deleting a directory is the one thing a consumer cannot undo —
    // and the lock keeps describing them, so re-adding the skill later meets managed directories
    // rather than an unmanaged collision.
    for (name, entry) in &previous.entries {
        if lock.entries.contains_key(name) {
            continue;
        }
        if prune {
            for destination in destinations {
                let path = destination.root.join(name);
                remove_any(&path)?;
                report.lines.push(Line {
                    name: name.clone(),
                    target: destination.target.clone(),
                    patched: false,
                    verdict: Verdict::Orphan,
                    detail: "no longer declared — removed".to_string(),
                });
            }
        } else {
            lock.entries.insert(name.clone(), entry.clone());
            report.warnings.push(format!(
                "`{name}` is in skills.lock but no longer declared — its directories are left in \
                 place and the lock keeps recording them; `playbook skills sync --prune` removes \
                 them"
            ));
        }
    }

    let rendered = lock.render();
    let existing = fs::read_to_string(&lock_path).ok();
    // A manifest that declares no skills should not acquire a header-only lock, which would read
    // as "this manifest has a skill story" when it has none.
    let should_write = match &existing {
        Some(existing) => existing != &rendered,
        None => !lock.entries.is_empty(),
    };
    if should_write {
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        fs::write(&lock_path, &rendered).map_err(|e| format!("{}: {e}", lock_path.display()))?;
    }

    note_patch(&mut report, &planned, true);

    Ok(report)
}

/// The lock entry for a skill after this run.
///
/// A patched skill keeps whatever entry it already had: the lock records **published** deliveries,
/// and a working copy is not one. A skill that was only ever patched has no entry at all, which is
/// what makes `check` say "sync" once the patch is removed.
fn entry_after(
    plan: &Planned,
    envelope: &Envelope,
    previous: &Lock,
    delivered: &[String],
) -> Option<Entry> {
    if plan.patched {
        return previous.entries.get(&plan.name).cloned();
    }
    // Union with what the lock already recorded: this run may have covered fewer targets than a
    // previous one, and a directory it did not touch is still ours.
    let mut targets = delivered.to_vec();
    if let Some(previous) = previous.entries.get(&plan.name) {
        targets.extend(previous.targets.iter().cloned());
    }
    targets.sort();
    targets.dedup();
    Some(Entry {
        name: plan.name.clone(),
        source: envelope.entry.source.clone(),
        version: envelope.entry.version.clone(),
        tree_digest: envelope.entry.tree_digest.clone(),
        targets,
    })
}

/// Name the patch, always — a green `check` must never quietly mean "tested against a local
/// working copy".
fn note_patch(report: &mut Report, planned: &[Planned], wrote_a_lock: bool) {
    let patched: Vec<&str> = planned
        .iter()
        .filter(|plan| plan.patched)
        .map(|plan| plan.name.as_str())
        .collect();
    if patched.is_empty() {
        return;
    }
    let lock = if wrote_a_lock {
        ", and skills.lock was left alone for them"
    } else {
        ""
    };
    report.warnings.push(format!(
        "{} skill(s) resolve through .playbook/skills.patch ({}), so the manifest's sources were \
         not used{lock}",
        patched.len(),
        patched.join(", ")
    ));
}
/// Re-export every declared skill and compare it with the lock and every delivered tree.
pub fn check(
    lock_dir: &Path,
    project: &Project,
    destinations: &[Destination],
    offline: bool,
) -> Result<Report, String> {
    let lock = Lock::read(&lock_path(lock_dir))?;
    let patch = read_patch(lock_dir)?;
    let planned = plan_sources(project, &patch)?;
    let (_scratch, exports) = export_all(&planned, offline)?;
    let mut report = Report::default();

    for (plan, envelope) in &exports {
        let name = &plan.name;

        // A patched skill is not in the lock by design, so it is compared with its working copy
        // directly. Everything else has to agree with the lock first.
        if plan.patched {
            for destination in destinations {
                let path = destination.root.join(name);
                if !path.is_dir() {
                    report.lines.push(Line {
                        name: name.clone(),
                        target: destination.target.clone(),
                        patched: true,
                        verdict: Verdict::Missing,
                        detail: format!(
                            "{} does not exist — run `playbook skills sync`",
                            path.display()
                        ),
                    });
                    continue;
                }
                match first_difference(&path, &envelope.tree)? {
                    None => report.lines.push(Line {
                        name: name.clone(),
                        target: destination.target.clone(),
                        patched: true,
                        verdict: Verdict::Ok,
                        detail: format!(
                            "{} — {} file(s), from {}",
                            envelope.entry.version, envelope.files, envelope.entry.source
                        ),
                    }),
                    Some(reason) => report.lines.push(Line {
                        name: name.clone(),
                        target: destination.target.clone(),
                        patched: true,
                        verdict: Verdict::Stale,
                        detail: format!(
                            "the delivered copy differs from the patched working tree: {reason}"
                        ),
                    }),
                }
            }
            continue;
        }

        let Some(recorded) = lock.entries.get(name) else {
            for destination in destinations {
                report.lines.push(Line {
                    name: name.clone(),
                    target: destination.target.clone(),
                    patched: false,
                    verdict: Verdict::Missing,
                    detail: "not in skills.lock — run `playbook skills sync`".to_string(),
                });
            }
            continue;
        };
        if recorded.tree_digest != envelope.entry.tree_digest {
            for destination in destinations {
                report.lines.push(Line {
                    name: name.clone(),
                    target: destination.target.clone(),
                    patched: false,
                    verdict: Verdict::Stale,
                    detail: format!(
                        "the source is at {} ({}), the lock says {} ({})",
                        envelope.entry.version,
                        envelope.entry.tree_digest,
                        recorded.version,
                        recorded.tree_digest
                    ),
                });
            }
            continue;
        }

        for destination in destinations {
            let path = destination.root.join(name);
            if !recorded.targets.contains(&destination.target) {
                report.lines.push(Line {
                    name: name.clone(),
                    target: destination.target.clone(),
                    patched: false,
                    verdict: Verdict::Missing,
                    detail: "skills.lock has no record of delivering it to this target".to_string(),
                });
                continue;
            }
            if !path.is_dir() {
                report.lines.push(Line {
                    name: name.clone(),
                    target: destination.target.clone(),
                    patched: false,
                    verdict: Verdict::Missing,
                    detail: format!("{} does not exist", path.display()),
                });
                continue;
            }
            match first_difference(&path, &envelope.tree)? {
                None => report.lines.push(Line {
                    name: name.clone(),
                    target: destination.target.clone(),
                    patched: false,
                    verdict: Verdict::Ok,
                    detail: format!("{} — {} file(s)", envelope.entry.version, envelope.files),
                }),
                Some(reason) => report.lines.push(Line {
                    name: name.clone(),
                    target: destination.target.clone(),
                    patched: false,
                    verdict: Verdict::Stale,
                    detail: format!(
                        "the delivered copy differs from {}: {reason}",
                        envelope.entry.version
                    ),
                }),
            }
        }
    }

    let planned_names: BTreeSet<&String> = exports.iter().map(|(plan, _)| &plan.name).collect();
    for name in lock.entries.keys() {
        if planned_names.contains(name) {
            continue;
        }
        for destination in destinations {
            report.lines.push(Line {
                name: name.clone(),
                target: destination.target.clone(),
                patched: false,
                verdict: Verdict::Orphan,
                detail: "in skills.lock but no longer declared".to_string(),
            });
        }
    }

    note_patch(&mut report, &planned, false);

    Ok(report)
}

fn lock_path(repo: &Path) -> PathBuf {
    repo.join(".playbook").join(LOCK_FILE)
}

/// Directories at a declared name that the lock does not account for **for this target**.
///
/// This is the one protection against clobbering a hand-installed skill — the case that matters,
/// because the first sync in a real repository meets exactly that. It is per target because a
/// zed run must not be able to claim a directory that only Claude Code has.
fn unmanaged_directories(
    root: &Path,
    declared: &[String],
    previous: &Lock,
    target: &str,
) -> Vec<PathBuf> {
    declared
        .iter()
        .filter(|name| root.join(name.as_str()).exists())
        .filter(|name| {
            !previous
                .entries
                .get(name.as_str())
                .is_some_and(|entry| entry.targets.iter().any(|known| known == target))
        })
        .map(|name| root.join(name))
        .collect()
}

/// Export every declared source, into one scratch directory each.
///
/// The scratch tree lives in the system temporary directory rather than the repository: it is a
/// build intermediate, and a consumer should not have to ignore it. The returned [`Scratch`] owns
/// it and removes it when the caller is done.
fn export_all(
    planned: &[Planned],
    offline: bool,
) -> Result<(Scratch, Vec<(Planned, Envelope)>), String> {
    let scratch = Scratch::new()?;
    let mut exported = Vec::new();
    for (index, plan) in planned.iter().enumerate() {
        let out = scratch.root.join(format!("{index}"));
        let envelope = export(&plan.effective, offline, &out)?;
        if envelope.entry.name != plan.name {
            return Err(format!(
                "`{}` exported `{}`, but the declaration says `{}` — the source and the name it is \
                 declared under have to agree",
                plan.effective, envelope.entry.name, plan.name
            ));
        }
        exported.push((plan.clone(), envelope));
    }
    Ok((scratch, exported))
}

fn export(source: &str, offline: bool, out: &Path) -> Result<Envelope, String> {
    let binary = std::env::var_os("SKILL_STORE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("skill-store"));
    let mut command = Command::new(&binary);
    command.arg("export").arg(source).arg("--out").arg(out);
    if offline {
        command.arg("--offline");
    }
    let output = command.output().map_err(|e| {
        format!(
            "cannot run {}: {e} — `skill-store` is a sibling binary; set SKILL_STORE to its path",
            binary.display()
        )
    })?;
    if !output.status.success() {
        return Err(format!(
            "skill-store export {source}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let conf_path = out.join("export.conf");
    let conf = parse_conf(
        &fs::read_to_string(&conf_path).map_err(|e| format!("{}: {e}", conf_path.display()))?,
    );
    let required = |key: &str| {
        conf.get(key)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                format!(
                    "{}: no `{key}:` in the export envelope — is this a skill-store envelope?",
                    conf_path.display()
                )
            })
    };
    let name = required("name")?;
    validate_name(&name)?;
    let files = required("files")?
        .parse::<usize>()
        .map_err(|error| format!("{}: `files`: {error}", conf_path.display()))?;
    let tree = out.join("skill");
    if !tree.is_dir() {
        return Err(format!(
            "{}: the envelope has no `skill/` directory — `skill-store export` wrote {}",
            out.display(),
            if files == 0 {
                "an empty version".to_string()
            } else {
                format!("{files} file(s) somewhere else")
            }
        ));
    }

    Ok(Envelope {
        entry: Entry {
            name,
            source: source.to_string(),
            version: required("version")?,
            tree_digest: required("tree_digest")?,
            // Filled in by `sync`, which is the only thing that knows where a skill landed.
            targets: Vec::new(),
        },
        files,
        tree,
    })
}

/// A skill name becomes a directory name, so it has to be one that cannot escape the root.
fn validate_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "`{name}` is not a skill name — 1–64 lowercase letters, digits and hyphens, not \
             starting or ending with a hyphen"
        ))
    }
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    let files = walk(from)?;
    if files.is_empty() {
        return Err(format!("{}: the export has no files", from.display()));
    }
    for (relative, path) in files {
        let destination = to.join(&relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        fs::copy(&path, &destination)
            .map_err(|e| format!("{} → {}: {e}", path.display(), destination.display()))?;
    }
    Ok(())
}

/// Remove a delivered tree, whichever kind of entry is sitting at the path.
fn remove_any(path: &Path) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let removed = if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    removed.map_err(|e| format!("{}: {e}", path.display()))
}

/// The first way two trees disagree, or `None` when they are identical.
///
/// Byte comparison rather than a digest: the composer has no hashing dependency, and comparing
/// the files is both exact and enough to name what changed. A read failure is an **error**, not
/// `None` — an unreadable tree must not report as an identical one.
fn first_difference(left: &Path, right: &Path) -> Result<Option<String>, String> {
    let left_files = walk(left)?;
    let right_files = walk(right)?;
    let left_paths: Vec<&String> = left_files.iter().map(|(path, _)| path).collect();
    let right_paths: Vec<&String> = right_files.iter().map(|(path, _)| path).collect();
    if left_paths != right_paths {
        let left_set: BTreeSet<&String> = left_paths.iter().copied().collect();
        let right_set: BTreeSet<&String> = right_paths.iter().copied().collect();
        if let Some(missing) = right_set.difference(&left_set).next() {
            return Ok(Some(format!("`{missing}` is missing")));
        }
        if let Some(extra) = left_set.difference(&right_set).next() {
            return Ok(Some(format!("`{extra}` is not in the export")));
        }
        // Same set, different order cannot happen: both lists are sorted.
        return Ok(Some("the file lists differ".to_string()));
    }
    for (relative, left_path) in &left_files {
        let right_path = right.join(relative);
        let left_bytes =
            fs::read(left_path).map_err(|e| format!("{}: {e}", left_path.display()))?;
        let right_bytes =
            fs::read(&right_path).map_err(|e| format!("{}: {e}", right_path.display()))?;
        if left_bytes != right_bytes {
            return Ok(Some(format!("`{relative}` differs")));
        }
    }
    Ok(None)
}

/// Every regular file under `root`, as `(relative path with '/', absolute path)`, sorted.
///
/// Symlinks are refused rather than followed, matching the store: a delivered tree that contains
/// one is not the tree that was exported.
fn walk(root: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let mut out = Vec::new();
    walk_into(root, root, &mut out, 0)?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn walk_into(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(String, PathBuf)>,
    depth: usize,
) -> Result<(), String> {
    if depth > 32 {
        return Err(format!(
            "{}: nesting is deeper than 32 levels",
            dir.display()
        ));
    }
    let entries = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    paths.sort();
    for path in paths {
        let metadata =
            fs::symlink_metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "{}: a symlink in a skill tree is not portable — replace it with a file or a \
                 directory",
                path.display()
            ));
        }
        if metadata.is_dir() {
            walk_into(root, &path, out, depth + 1)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|e| format!("{}: {e}", path.display()))?
                .to_string_lossy()
                .replace('\\', "/");
            out.push((relative, path));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "playbook-skills-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    fn entry(name: &str, digest: &str) -> Entry {
        Entry {
            name: name.to_string(),
            source: format!("path:~/store#{name}@v1"),
            version: "v1".to_string(),
            tree_digest: digest.to_string(),
            targets: vec!["zed".to_string()],
        }
    }

    #[test]
    fn the_lock_round_trips() {
        let mut lock = Lock::default();
        lock.entries.insert(
            "inspect-code".to_string(),
            entry("inspect-code", "sha256:aa"),
        );
        lock.entries.insert(
            "widget-screenshot".to_string(),
            entry("widget-screenshot", "sha256:bb"),
        );
        assert_eq!(Lock::parse(&lock.render()).expect("parse"), lock);
    }

    #[test]
    fn a_missing_lock_is_empty_rather_than_an_error() {
        let dir = scratch("no-lock");
        assert!(Lock::read(&dir.join("skills.lock"))
            .expect("read")
            .entries
            .is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_malformed_lock_line_names_its_number() {
        let error = Lock::parse("inspect-code: path:x#inspect-code@v1\n").expect_err("short");
        assert!(error.contains("line 1"), "{error}");
        assert!(error.contains("field(s)"), "{error}");
    }

    #[test]
    fn a_duplicate_name_in_the_lock_is_refused() {
        let body = "a: path:x#a@v1 v1 sha256:aa zed\na: path:y#a@v1 v1 sha256:bb zed\n";
        let error = Lock::parse(body).expect_err("duplicate");
        assert!(error.contains("appears twice"), "{error}");
    }

    #[test]
    fn lock_targets_are_sorted_and_deduplicated() {
        let lock = Lock::parse("a: path:x#a@v1 v1 sha256:aa zed,claude-code,zed\n").expect("parse");
        assert_eq!(
            lock.entries["a"].targets,
            vec!["claude-code".to_string(), "zed".to_string()]
        );
    }

    #[test]
    fn a_name_that_could_escape_the_skill_root_is_refused() {
        assert!(validate_name("..").is_err());
        assert!(validate_name("a/b").is_err());
        assert!(validate_name("Upper").is_err());
        assert!(validate_name("ok-name").is_ok());
    }

    #[test]
    fn an_undeclared_directory_at_a_declared_name_is_a_collision() {
        let dir = scratch("collision");
        let root = dir.join("skills");
        fs::create_dir_all(root.join("managed")).expect("managed dir");
        fs::create_dir_all(root.join("hand-installed")).expect("hand-installed dir");

        let mut lock = Lock::default();
        lock.entries
            .insert("managed".to_string(), entry("managed", "sha256:aa"));
        let declared = vec![
            "managed".to_string(),
            "hand-installed".to_string(),
            "not-there".to_string(),
        ];

        let collisions = unmanaged_directories(&root, &declared, &lock, "zed");
        assert_eq!(collisions, vec![root.join("hand-installed")]);

        // A directory delivered to a *different* target is not evidence that this one is ours.
        let collisions = unmanaged_directories(&root, &declared, &lock, "claude-code");
        assert_eq!(
            collisions,
            vec![root.join("managed"), root.join("hand-installed")]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    fn target(name: &str, harness: &str, dir: &str, global: &str) -> (String, Target) {
        (
            name.to_string(),
            Target {
                name: name.to_string(),
                harness: harness.to_string(),
                skill_dir: dir.to_string(),
                skill_global_dir: global.to_string(),
                ..Default::default()
            },
        )
    }

    #[test]
    fn repo_destinations_are_the_targets_own_directories() {
        let targets = vec![
            target("zed", "zed", ".agents/skills", "~/global-zed"),
            target("deepseek-flash", "any", "", ""),
        ];
        let plan = destinations(&targets, SkillScope::Repo, Path::new("/repo"));
        assert_eq!(
            plan,
            vec![Destination {
                target: "zed".to_string(),
                root: PathBuf::from("/repo/.agents/skills"),
            }]
        );
    }

    #[test]
    fn a_harness_specific_target_wins_the_directory_it_shares_with_generic() {
        // `generic` and `zed` both name `.agents/skills`; delivering twice would be wasted work
        // and would make the report name a target the caller never asked for.
        let targets = vec![
            target("generic", "any", ".agents/skills", "~/global"),
            target("zed", "zed", ".agents/skills", "~/global"),
        ];
        let plan = destinations(&targets, SkillScope::Repo, Path::new("/repo"));
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].target, "zed");
    }

    #[test]
    fn global_destinations_expand_the_tilde() {
        if std::env::var_os("HOME").is_none() {
            return;
        }
        let targets = vec![target("claude-code", "claude-code", "", "~/.claude/skills")];
        let plan = destinations(&targets, SkillScope::Global, Path::new("/repo"));
        assert_eq!(plan.len(), 1);
        assert!(plan[0].root.ends_with(".claude/skills"));
        assert!(plan[0].root.is_absolute());
        assert_eq!(plan[0].target, "claude-code");
    }

    fn project_with(skills: &[&str]) -> Project {
        Project {
            skills: skills.iter().map(|source| source.to_string()).collect(),
            ..Default::default()
        }
    }

    fn envelope(name: &str, source: &str, version: &str, digest: &str) -> Envelope {
        Envelope {
            entry: Entry {
                name: name.to_string(),
                source: source.to_string(),
                version: version.to_string(),
                tree_digest: digest.to_string(),
                targets: Vec::new(),
            },
            files: 1,
            tree: PathBuf::from("/nonexistent"),
        }
    }

    #[test]
    fn a_skill_name_comes_from_the_fragment() {
        assert_eq!(
            declared_name("github:a/b#inspect-code@v1"),
            Some("inspect-code")
        );
        assert_eq!(declared_name("path:~/x#inspect-code"), Some("inspect-code"));
        assert_eq!(declared_name("github:a/b"), None);
        assert_eq!(declared_name("github:a/b#"), None);
    }

    #[test]
    fn a_patch_replaces_the_source_for_its_name() {
        let project = project_with(&["github:a/b#inspect-code@v1"]);
        let mut patch = BTreeMap::new();
        patch.insert(
            "inspect-code".to_string(),
            "path:~/wip/inspect-code".to_string(),
        );

        let planned = plan_sources(&project, &patch).expect("plan");
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].declared, "github:a/b#inspect-code@v1");
        assert_eq!(planned[0].effective, "path:~/wip/inspect-code");
        assert!(planned[0].patched);
    }

    #[test]
    fn a_patch_that_matches_nothing_is_an_error() {
        let project = project_with(&["github:a/b#inspect-code@v1"]);
        let mut patch = BTreeMap::new();
        patch.insert("capture-rule".to_string(), "path:~/wip".to_string());

        let error = plan_sources(&project, &patch).expect_err("unused patch");
        assert!(error.contains("not declared"), "{error}");
    }

    #[test]
    fn a_declaration_without_a_selector_is_an_error() {
        let project = project_with(&["github:a/b"]);
        let error = plan_sources(&project, &BTreeMap::new()).expect_err("no selector");
        assert!(error.contains("#<skill>"), "{error}");
    }

    #[test]
    fn the_same_skill_declared_twice_is_an_error() {
        let project = project_with(&["github:a/b#x@v1", "github:c/d#x@v2"]);
        let error = plan_sources(&project, &BTreeMap::new()).expect_err("duplicate");
        assert!(error.contains("declared twice"), "{error}");
    }

    #[test]
    fn a_patched_skill_does_not_rewrite_its_lock_entry() {
        let previous_entry = entry("inspect-code", "sha256:aa");
        let mut previous = Lock::default();
        previous
            .entries
            .insert("inspect-code".to_string(), previous_entry.clone());
        let plan = Planned {
            name: "inspect-code".to_string(),
            declared: "github:a/b#inspect-code@v1".to_string(),
            effective: "path:~/wip/inspect-code".to_string(),
            patched: true,
        };

        let after = entry_after(
            &plan,
            &envelope("inspect-code", &plan.effective, "working", "sha256:bb"),
            &previous,
            &["zed".to_string()],
        );
        assert_eq!(
            after,
            Some(previous_entry),
            "a working copy is not a published delivery, so the lock entry has to survive unchanged"
        );
    }

    #[test]
    fn a_skill_that_was_only_ever_patched_has_no_lock_entry() {
        let plan = Planned {
            name: "inspect-code".to_string(),
            declared: "github:a/b#inspect-code@v1".to_string(),
            effective: "path:~/wip/inspect-code".to_string(),
            patched: true,
        };
        let after = entry_after(
            &plan,
            &envelope("inspect-code", &plan.effective, "working", "sha256:bb"),
            &Lock::default(),
            &["zed".to_string()],
        );
        assert_eq!(after, None);
    }

    #[test]
    fn an_unpatched_run_keeps_the_targets_it_did_not_touch() {
        let mut previous = Lock::default();
        let mut earlier = entry("inspect-code", "sha256:aa");
        earlier.targets = vec!["claude-code".to_string()];
        previous.entries.insert("inspect-code".to_string(), earlier);

        let plan = Planned {
            name: "inspect-code".to_string(),
            declared: "github:a/b#inspect-code@v1".to_string(),
            effective: "github:a/b#inspect-code@v1".to_string(),
            patched: false,
        };
        let after = entry_after(
            &plan,
            &envelope("inspect-code", &plan.effective, "v1", "sha256:cc"),
            &previous,
            &["zed".to_string()],
        )
        .expect("entry");

        assert_eq!(
            after.targets,
            vec!["claude-code".to_string(), "zed".to_string()]
        );
        assert_eq!(after.tree_digest, "sha256:cc");
    }

    #[test]
    fn a_tree_comparison_names_the_changed_file() {
        let dir = scratch("compare");
        let left = dir.join("left");
        let right = dir.join("right");
        for root in [&left, &right] {
            fs::create_dir_all(root.join("references")).expect("tree");
            fs::write(root.join("SKILL.md"), "same\n").expect("skill");
            fs::write(root.join("references/a.md"), "same\n").expect("reference");
        }
        assert!(first_difference(&left, &right).expect("compare").is_none());

        fs::write(right.join("references/a.md"), "changed\n").expect("edit");
        assert_eq!(
            first_difference(&left, &right).expect("compare").as_deref(),
            Some("`references/a.md` differs")
        );

        fs::write(right.join("references/a.md"), "same\n").expect("restore");
        // An extra file in the delivered copy is the edit a reader is most likely to make.
        fs::write(left.join("references/b.md"), "new\n").expect("extra");
        assert_eq!(
            first_difference(&left, &right).expect("compare").as_deref(),
            Some("`references/b.md` is not in the export")
        );

        // And the other direction: the export has a file the delivered copy lost.
        fs::remove_file(left.join("references/b.md")).expect("remove extra");
        fs::write(left.join("references/a.md"), "same\n").expect("keep");
        fs::write(right.join("references/b.md"), "new\n").expect("missing");
        assert_eq!(
            first_difference(&left, &right).expect("compare").as_deref(),
            Some("`references/b.md` is missing")
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
