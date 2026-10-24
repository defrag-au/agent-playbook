//! The verbs, and the resolution they share.
//!
//! A verb returns an [`Outcome`] — a report and the exit code it implies — so "nothing found" is
//! a fact the caller can branch on rather than an empty success. The shared contract lives in
//! `at-core`, because the same shape has to hold in every `at-*` tool.

pub mod build;
pub mod files;
pub mod installed;
pub mod location;
pub mod out;
pub mod search;
pub mod slice;

use std::path::{Path, PathBuf};

use at_core::contract::{self, secret_shaped, Fail, Report, MAX_FILES, MAX_LIMIT};

use crate::project::{Package, Project, Resolved, SourceKind};
use crate::store::{self, Listing, Store};
use crate::walk;
use crate::TOOL;

pub use at_core::contract::{plural, Outcome};

#[derive(Debug)]
pub struct Options {
    pub limit: usize,
    pub limit_clamped_from: Option<usize>,
    pub max_files: usize,
    pub max_files_clamped_from: Option<usize>,
    pub depth: usize,
    pub depth_clamped_from: Option<usize>,
    pub include_secret_paths: bool,
    pub summary: bool,
    pub count: bool,
    pub files_only: bool,
    pub version: Option<String>,
    pub registry: Option<String>,
    pub build: bool,
    pub build_dir: Option<PathBuf>,
    /// Named on the command line, so every exit can echo it. These are canonical where the path
    /// had to exist; `--cargo-home` is checked by [`Store::resolve`].
    pub cargo_home: Option<PathBuf>,
    pub project: Option<PathBuf>,
    pub target_dir: Option<PathBuf>,
    pub cwd: PathBuf,
}

impl Options {
    pub fn store(&self) -> Result<Store, Fail> {
        Store::resolve(self.cargo_home.as_deref())
    }

    /// The project with its lock, for a verb that must know which version the project chose.
    pub fn project(&self) -> Result<Project, Fail> {
        Project::discover(self.project.as_deref(), &self.cwd)
    }

    /// The project without its lock, for `out`.
    pub fn project_root(&self) -> Result<Project, Fail> {
        Project::root(self.project.as_deref(), &self.cwd)
    }

    /// The caller's own invocation with the context flags echoed, so an exit resolves against the
    /// same store and project it came from. A flag is echoed only when the target verb's table
    /// accepts it: `where` may suggest `out`, and `out` does not read a cargo home.
    pub fn again(&self, verb: &str, positionals: &[String], flags: &[String]) -> String {
        let accepts = |flag: &str| {
            crate::catalogue::find(verb)
                .is_some_and(|verb| verb.flags.iter().any(|candidate| candidate.name == flag))
        };
        let mut all = flags.to_vec();
        if let Some(path) = &self.cargo_home {
            if accepts("--cargo-home") {
                all.push(format!("--cargo-home {}", shell_word(path)));
            }
        }
        if let Some(path) = &self.project {
            if accepts("--project") {
                all.push(format!("--project {}", shell_word(path)));
            }
        }
        if let Some(path) = &self.target_dir {
            if accepts("--target-dir") {
                all.push(format!("--target-dir {}", shell_word(path)));
            }
        }
        if let Some(version) = &self.version {
            if accepts("--version") {
                all.push(format!("--version {}", shell_word_text(version)));
            }
        }
        if let Some(registry) = &self.registry {
            if accepts("--registry") {
                all.push(format!("--registry {}", shell_word_text(registry)));
            }
        }
        if self.build && accepts("--build") {
            all.push("--build".to_string());
        }
        if let Some(path) = &self.build_dir {
            if accepts("--build-dir") {
                all.push(format!("--build-dir {}", shell_word(path)));
            }
        }
        if self.include_secret_paths && accepts("--include-secret-paths") {
            all.push("--include-secret-paths".to_string());
        }
        contract::again(TOOL, verb, positionals, &all, None)
    }
}

fn shell_word(path: &Path) -> String {
    contract::shell_word(&path.display().to_string())
}

fn shell_word_text(text: &str) -> String {
    contract::shell_word(text)
}

/// The one target a single-target verb accepts.
pub fn exactly_one<'a>(args: &'a [String], verb: &str, what: &str) -> Result<&'a str, Fail> {
    match args {
        [one] => Ok(one),
        [] => Err(Fail::usage(format!(
            "`{verb}` needs {what} · at-crate {verb} {what}"
        ))),
        _ => Err(Fail::usage(format!("`{verb}` takes one {what} at a time"))),
    }
}

/// A verb that asks about a crate as a whole takes a name, never a path. Saying so here keeps
/// `where`, `installed` and `out` from silently treating `foo/src` as a package name.
pub fn bare_crate<'a>(arg: &'a str, verb: &str) -> Result<&'a str, Fail> {
    if arg.contains('/') || arg.contains(':') {
        return Err(Fail::usage(format!(
            "`{verb}` takes a crate name, not a path · use files, slice or search for a path"
        )));
    }
    Ok(arg)
}

/// Refuse a secret-shaped file name unless the caller forced it. The same guard rail `at-peek`
/// applies, because crate fixtures are not a safer place for a token than a worktree.
pub fn check_secret(path: &Path, opts: &Options, display: &str) -> Result<(), Fail> {
    if opts.include_secret_paths {
        return Ok(());
    }
    let name = walk::file_name(path);
    if let Some(rule) = secret_shaped(&name) {
        return Err(Fail::refused(format!(
            "{display} is refused: {rule} · --include-secret-paths reads it anyway"
        )));
    }
    Ok(())
}

/// The resolved crate for a query, from the project's lock first and the store's single copy
/// second. Ambiguity is always an error naming the choices; this function never picks one.
pub fn resolve(
    query: &str,
    opts: &Options,
    store: &Store,
    project: &Project,
    listing: &Listing,
) -> Result<Resolved, Fail> {
    let candidates = project
        .lock
        .as_ref()
        .map(|lock| lock.matching(query))
        .unwrap_or_default();

    if let Some(version) = opts.version.as_deref() {
        if let Some(package) = candidates.iter().find(|package| package.version == version) {
            return from_package(package, opts, store, listing)
                .map(|resolved| with_spelling(resolved, query));
        }
        let installed = store.find(listing, query, Some(version), opts.registry.as_deref())?;
        return Ok(with_spelling(
            Resolved::from_installed(&installed, query),
            query,
        ));
    }

    match candidates.as_slice() {
        [] => {
            let installed = store.find(listing, query, None, opts.registry.as_deref())?;
            Ok(with_spelling(
                Resolved::from_installed(&installed, query),
                query,
            ))
        }
        [package] => {
            let resolved = from_package(package, opts, store, listing)?;
            Ok(with_spelling(resolved, query))
        }
        many => {
            let lock = project
                .lock
                .as_ref()
                .map(|lock| lock.path.display().to_string())
                .unwrap_or_default();
            let choices: Vec<String> = many
                .iter()
                .map(|package| format!("{} · {}", package.version, origin(package)))
                .collect();
            Err(Fail::usage(format!(
                "{query} appears {} times in {lock} · --version <ver> picks one\n  {}",
                many.len(),
                choices.join("\n  ")
            )))
        }
    }
}

fn with_spelling(mut resolved: Resolved, query: &str) -> Resolved {
    resolved.spelling = (query != resolved.name).then(|| resolved.name.clone());
    resolved
}

fn origin(package: &Package) -> String {
    match SourceKind::of(package.source.as_deref()) {
        SourceKind::Registry => "registry".to_string(),
        SourceKind::Git { .. } => "git".to_string(),
        SourceKind::Path => "path".to_string(),
        SourceKind::Unknown(source) => source,
    }
}

fn from_package(
    package: &Package,
    opts: &Options,
    store: &Store,
    listing: &Listing,
) -> Result<Resolved, Fail> {
    let kind = SourceKind::of(package.source.as_deref());
    let dir = match &kind {
        SourceKind::Registry => {
            let installed = store.find(
                listing,
                &package.name,
                Some(&package.version),
                opts.registry.as_deref(),
            )?;
            Some(installed.dir)
        }
        SourceKind::Git { url, rev } => {
            let candidates = store.git_checkout_candidates(url, rev.as_deref());
            match candidates.as_slice() {
                [] => None,
                [one] => Some(one.clone()),
                many => {
                    let paths: Vec<String> =
                        many.iter().map(|path| path.display().to_string()).collect();
                    return Err(Fail::usage(format!(
                        "{url} has {} extracted checkouts at rev {} · this tool will not pick one\n  {}",
                        many.len(),
                        rev.as_deref().unwrap_or("unknown"),
                        paths.join("\n  ")
                    )));
                }
            }
        }
        SourceKind::Path | SourceKind::Unknown(_) => None,
    };

    Ok(Resolved {
        name: package.name.clone(),
        version: package.version.clone(),
        kind,
        dir,
        from_lock: true,
        spelling: None,
    })
}

/// `1.2 kB`, `3.4 MB` — sizes a reader can compare without counting digits.
pub fn size(len: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    if len < KB {
        format!("{len} B")
    } else if len < MB {
        format!("{:.1} kB", len as f64 / KB as f64)
    } else {
        format!("{:.1} MB", len as f64 / MB as f64)
    }
}

/// The header every verb starts from, with the clamped-limit announcements already in place.
pub fn report_for(opts: &Options, verb: &str, root: &str, target: &str) -> Report {
    let mut report = Report::new();
    announce_clamps(&mut report, opts);
    report.header(TOOL, verb, root, target);
    report
}

/// The clamps travel with every answer, so a caller can see the limit it asked for was not the
/// limit it got.
pub fn announce_clamps(report: &mut Report, opts: &Options) {
    if let Some(asked) = opts.limit_clamped_from {
        report.bound(format!("--limit {asked} clamped to {MAX_LIMIT}"));
    }
    if let Some(asked) = opts.max_files_clamped_from {
        report.bound(format!("--max-files {asked} clamped to {MAX_FILES}"));
    }
    if let Some(asked) = opts.depth_clamped_from {
        report.bound(format!("--depth {asked} clamped to 6"));
    }
}

/// Kept here so `installed` and `where` use the same spelling rule as `store`.
pub fn normalised(name: &str) -> String {
    store::normalised(name)
}
