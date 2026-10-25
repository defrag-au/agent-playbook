//! `playbook` — the command line over the resolution engine.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use playbook::init;
use playbook::install::{self, Outcome};
use playbook::load::{
    census, claiming_project, expand_tilde, find_manifest, find_root, find_root_with_source,
    included_by_some_target, list_projects, list_targets, load_project, load_target, RootSource,
};
use playbook::model::{Activation, Diagnostic};
use playbook::render;
use playbook::resolve;

const USAGE: &str = "\
playbook — resolve and render agent rules for a project + target

usage: playbook <command> [options]

commands
  compose    print the managed block
  list       print the resolution table for a project + target
  install    write the block into a repository's agent file
  check      exit 1 if a repository's block is stale
  init       scaffold a repository's .playbook/ manifest and rules directory
  root       print the rule tree being resolved against, and how it was found
  projects   list known registry projects and targets
  rules      list every rule, and which projects activate it

options
  --project <name>   legacy registry project under projects/; for init, the name to write
  --manifest <path>  a `.playbook/project.conf`, or the directory containing one
  --target <name>    target under models/ (default: the project's default_target)
  --repo <path>      repository root (install, check, init; default: the working directory)
  --file <name>      filename within the repo (default: the target's default_file)
  --out <file>       write to a file instead of stdout (compose)
  --root <dir>       playbook root (default: found from the working directory, then
                     beside the installed binary — `playbook root` says which)
  -h, --help         this

examples
  playbook list
  playbook install --target zed
  playbook check                     # from inside the repository: neither is needed
  playbook check --manifest ~/code/DamonOehlman/fleet/personal/.playbook/project.conf --repo ~/.config/zed
  playbook init --project montager
  playbook list --project shared-crates   # legacy registry entry
  playbook root
";

#[derive(Default)]
struct Opts {
    project: Option<String>,
    manifest: Option<String>,
    target: Option<String>,
    repo: Option<String>,
    file: Option<String>,
    out: Option<String>,
    root: Option<String>,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() || args[0] == "-h" || args[0] == "--help" || args[0] == "help" {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    let command = args[0].clone();
    let opts = match parse_opts(&args[1..]) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {e}");
            eprint!("\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    let result = match command.as_str() {
        "compose" => cmd_compose(&opts),
        "list" => cmd_list(&opts),
        "install" => cmd_install(&opts, false),
        "check" => cmd_install(&opts, true),
        "init" => cmd_init(&opts),
        "root" => cmd_root(&opts),
        "projects" => cmd_projects(&opts),
        "rules" => cmd_rules(&opts),
        other => Err(format!("unknown command `{other}`")),
    };

    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}

fn parse_opts(args: &[String]) -> Result<Opts, String> {
    const VALUE_OPTS: &[&str] = &[
        "--project",
        "--manifest",
        "--target",
        "--repo",
        "--file",
        "--out",
        "--root",
    ];

    let mut opts = Opts::default();
    let mut i = 0;
    while i < args.len() {
        let key = args[i].as_str();
        if !VALUE_OPTS.contains(&key) {
            return Err(format!("unknown option `{key}`"));
        }
        let value = args
            .get(i + 1)
            .cloned()
            .ok_or_else(|| format!("`{key}` needs a value"))?;
        match key {
            "--project" => opts.project = Some(value),
            "--manifest" => opts.manifest = Some(value),
            "--target" => opts.target = Some(value),
            "--repo" => opts.repo = Some(value),
            "--file" => opts.file = Some(value),
            "--out" => opts.out = Some(value),
            "--root" => opts.root = Some(value),
            _ => unreachable!("checked against VALUE_OPTS"),
        }
        i += 2;
    }
    Ok(opts)
}

fn root_of(opts: &Opts) -> Result<PathBuf, String> {
    find_root(opts.root.as_deref().map(Path::new))
}

/// The root and where it came from, for the two commands that report a verdict about a repository.
fn root_of_with_source(opts: &Opts) -> Result<(PathBuf, RootSource), String> {
    find_root_with_source(opts.root.as_deref().map(Path::new))
}

/// Say which tree a verdict came from, when it is not the one the caller is standing in.
///
/// `check` against the packaged copy and `check` against the checkout you just edited print the
/// same `ok`, and only one of them is about your change. Said as a fact and not as an instruction:
/// the packaged copy is the right tree for every repository that is not the playbook itself, so a
/// line telling that case to go and find a checkout reads as a complaint about a verdict that was
/// correct. Silence when the root came from `--root`, `PLAYBOOK_ROOT` or the working directory:
/// the caller named it or is in it. `playbook root` answers the same question on demand, at length.
fn note_root(root: &Path, source: RootSource) {
    if source.is_packaged() {
        println!("# rules: {} · {}", root.display(), source.as_str());
    }
}

fn report(diagnostics: &[Diagnostic]) {
    for d in diagnostics {
        eprintln!("{d}");
    }
}

/// Which project a command is about.
enum ProjectChoice {
    /// A legacy `projects/<name>/project.conf` entry.
    Registry(String),
    /// A repository's own `.playbook/project.conf`.
    Repo(PathBuf),
}

/// The directory a repository-scoped command is anchored at.
///
/// `--repo` defaults to the working directory. An explicit `--repo` is never walked away from: it
/// is the directory the caller named.
fn anchor(opts: &Opts) -> Result<PathBuf, String> {
    let path = match &opts.repo {
        Some(path) => expand_tilde(path),
        None => std::env::current_dir()
            .map_err(|e| format!("cannot read the working directory: {e}"))?,
    };
    if !path.is_dir() {
        return Err(format!("no such directory: {}", path.display()));
    }
    Ok(path)
}

/// The project a command is about.
///
/// An explicit `--manifest` or `--project` wins over discovery. Otherwise a
/// `.playbook/project.conf` at or above the anchor is used, and a legacy registry entry is found by
/// the `path:` it claims. `--manifest` is how a manifest can live outside the directory it writes
/// into (the personal file in `fleet`).
fn choose_project(root: &Path, opts: &Opts, anchor: &Path) -> Result<ProjectChoice, String> {
    if let Some(raw) = &opts.manifest {
        let path = expand_tilde(raw);
        let manifest = if path.is_dir() {
            path.join(".playbook").join("project.conf")
        } else {
            let is_manifest = path.file_name().is_some_and(|name| name == "project.conf")
                && path
                    .parent()
                    .and_then(Path::file_name)
                    .is_some_and(|name| name == ".playbook");
            if !is_manifest {
                return Err(format!(
                    "`--manifest` must name <repo>/.playbook/project.conf or the repository \
                     directory, got {}",
                    path.display()
                ));
            }
            path
        };
        if !manifest.is_file() {
            return Err(format!(
                "{} is not a `.playbook/project.conf` (or a directory containing one)",
                manifest.display()
            ));
        }
        let repo = manifest
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| format!("{} is not inside a repository", manifest.display()))?;
        return Ok(ProjectChoice::Repo(repo.to_path_buf()));
    }

    if let Some(name) = &opts.project {
        return Ok(ProjectChoice::Registry(name.clone()));
    }

    if let Some((repo, _)) = find_manifest(anchor) {
        return Ok(ProjectChoice::Repo(repo));
    }

    if let Some((project, _)) = claiming_project(root, anchor)? {
        return Ok(ProjectChoice::Registry(project.name));
    }

    Err(format!(
        "no `.playbook/project.conf` at or above {} — run `playbook init`, or pass `--manifest \
         <path>` (or `--project <name>` for a legacy registry entry)",
        anchor.display()
    ))
}

fn resolve_choice(
    root: &Path,
    choice: &ProjectChoice,
    target: Option<&str>,
) -> Result<resolve::Resolved, String> {
    match choice {
        ProjectChoice::Registry(name) => resolve::resolve(root, name, target),
        ProjectChoice::Repo(repo) => resolve::resolve_repo(root, repo, target),
    }
}

/// Resolve, report, and stop if anything was fatal.
fn resolved_or_exit(
    root: &Path,
    choice: &ProjectChoice,
    target: Option<&str>,
) -> Result<Option<resolve::Resolved>, String> {
    let resolved = resolve_choice(root, choice, target)?;
    report(&resolved.diagnostics);
    if resolved.has_errors() {
        return Ok(None);
    }
    Ok(Some(resolved))
}

/// The repository whose instruction file a command writes or checks.
///
/// An explicit `--repo` is the answer. Otherwise a repository manifest supplies its own root; a
/// legacy registry entry is matched by its `path:` and, failing that, falls back to the anchor.
fn output_repo(
    root: &Path,
    opts: &Opts,
    choice: &ProjectChoice,
    anchor: PathBuf,
) -> Result<PathBuf, String> {
    if let Some(path) = &opts.repo {
        return Ok(expand_tilde(path));
    }
    match choice {
        ProjectChoice::Repo(repo) => Ok(repo.clone()),
        ProjectChoice::Registry(_) => Ok(claiming_project(root, &anchor)?
            .map(|(_, dir)| dir)
            .unwrap_or(anchor)),
    }
}

fn cmd_compose(opts: &Opts) -> Result<ExitCode, String> {
    let root = root_of(opts)?;
    let anchor = anchor(opts)?;
    let choice = choose_project(&root, opts, &anchor)?;
    let Some(resolved) = resolved_or_exit(&root, &choice, opts.target.as_deref())? else {
        return Ok(ExitCode::from(1));
    };
    let block = render::render(&resolved);

    match &opts.out {
        Some(path) => {
            let path = expand_tilde(path);
            fs::write(&path, &block).map_err(|e| format!("{}: {e}", path.display()))?;
            eprintln!("wrote {}", path.display());
        }
        None => print!("{block}"),
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_list(opts: &Opts) -> Result<ExitCode, String> {
    let root = root_of(opts)?;
    let anchor = anchor(opts)?;
    let choice = choose_project(&root, opts, &anchor)?;
    let Some(resolved) = resolved_or_exit(&root, &choice, opts.target.as_deref())? else {
        return Ok(ExitCode::from(1));
    };

    println!(
        "project: {}   org: {}   languages: {}",
        resolved.project.name,
        resolved.project.org.as_deref().unwrap_or("-"),
        if resolved.project.languages.is_empty() {
            "-".to_string()
        } else {
            resolved.project.languages.join(", ")
        }
    );
    println!(
        "target:  {}   model: {}   harness: {}",
        resolved.target.name, resolved.target.model, resolved.target.harness
    );
    println!(
        "include: {}   exclude: {}   exclude_activation: {}   emphasis: {}",
        or_dash(&resolved.target.include),
        or_dash(&resolved.target.exclude),
        or_dash(&resolved.target.exclude_activation),
        or_dash(&resolved.target.emphasis)
    );
    println!(
        "memory: {}   addenda: {}",
        or_dash(&resolved.target.memory),
        or_dash(&resolved.target.addenda)
    );

    let width = resolved
        .rules
        .iter()
        .map(|r| r.rel.len())
        .max()
        .unwrap_or(4)
        .max(4);

    println!();
    println!(
        "  {:<3} {:<15} {:>4}  {:<width$}",
        "#", "layer", "pri", "rule"
    );
    println!("  {}", "-".repeat(width + 26));
    for (n, rule) in resolved.rules.iter().enumerate() {
        println!(
            "  {:<3} {:<15} {:>4}  {:<width$}",
            n + 1,
            rule.layer.as_str(),
            rule.priority,
            rule.rel
        );
    }
    println!("\n{} rules", resolved.rules.len());
    if !resolved.memory.is_empty() {
        println!("memory sections: {}", resolved.memory.len());
    }

    if !resolved.superseded.is_empty() {
        println!("superseded by a higher layer:");
        for id in &resolved.superseded {
            println!("  {id}");
        }
    }
    if !resolved.emphasis.is_empty() {
        println!("emphasised in the preamble:");
        for rule in &resolved.emphasis {
            println!("  {}", rule.id);
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn or_dash(list: &[String]) -> String {
    if list.is_empty() {
        "-".to_string()
    } else {
        list.join(", ")
    }
}

fn cmd_install(opts: &Opts, check: bool) -> Result<ExitCode, String> {
    let (root, source) = root_of_with_source(opts)?;
    note_root(&root, source);
    let anchor = anchor(opts)?;
    let choice = choose_project(&root, opts, &anchor)?;
    let repo = output_repo(&root, opts, &choice, anchor)?;

    let Some(resolved) = resolved_or_exit(&root, &choice, opts.target.as_deref())? else {
        return Ok(ExitCode::from(1));
    };

    let file = opts
        .file
        .clone()
        .unwrap_or_else(|| resolved.target.default_file.clone());

    // A harness that reads only the first matching instruction file will silently ignore the
    // one we write if something outranks it. Say so before writing anything.
    for shadow in install::shadowing_files(&repo, &file, &resolved.target.instruction_files) {
        eprintln!(
            "warning: {} outranks {} in {}'s instruction-file order — the managed block will \
             NOT be read. Remove it, or install into it instead with `--file {shadow}`.",
            shadow, file, resolved.target.name
        );
    }

    let block = render::render(&resolved);

    match install::write(&repo, &file, &block, check)? {
        Outcome::Created(path) => {
            println!("created {}", path.display());
            Ok(ExitCode::SUCCESS)
        }
        Outcome::Updated(path) => {
            println!("updated {}", path.display());
            Ok(ExitCode::SUCCESS)
        }
        Outcome::Unchanged(path) => {
            println!("unchanged {}", path.display());
            Ok(ExitCode::SUCCESS)
        }
        Outcome::UpToDate(path) => {
            println!(
                "ok: {} matches project={} target={}",
                path.display(),
                resolved.project.name,
                resolved.target.name
            );
            Ok(ExitCode::SUCCESS)
        }
        Outcome::Stale(path, diff) => {
            eprintln!("stale: {} differs from the composed block", path.display());
            eprint!("{diff}");
            let command = match &choice {
                ProjectChoice::Registry(name) => format!(
                    "playbook install --project {name} --target {} --repo {}",
                    resolved.target.name,
                    repo.display()
                ),
                ProjectChoice::Repo(_) => format!(
                    "playbook install --target {} --repo {}",
                    resolved.target.name,
                    repo.display()
                ),
            };
            eprintln!("\nrun: {command}");
            Ok(ExitCode::from(1))
        }
        Outcome::Missing(path) => {
            eprintln!("stale: {} does not exist", path.display());
            Ok(ExitCode::from(1))
        }
        Outcome::NoBlock(path) => {
            eprintln!("stale: no agent-playbook block in {}", path.display());
            Ok(ExitCode::from(1))
        }
    }
}

/// Scaffold a repository's `.playbook/` manifest and rules directory.
fn cmd_init(opts: &Opts) -> Result<ExitCode, String> {
    if opts.manifest.is_some() {
        return Err("`init` creates a manifest; `--manifest` is not applicable".to_string());
    }
    let repo = anchor(opts)?;
    let name = match &opts.project {
        Some(name) => name.clone(),
        None => repo
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("cannot derive a project name from {}", repo.display()))?
            .to_string(),
    };
    let target = opts.target.as_deref().unwrap_or("claude-code");

    let created = init::scaffold(&repo, &name, target)?;
    for path in &created {
        println!("created {}", path.display());
    }
    println!(
        "\nnext: edit {}/.playbook/project.conf, then run `playbook install --target {target}`",
        repo.display()
    );
    Ok(ExitCode::SUCCESS)
}

/// Which rule tree is being resolved against, and how it was found.
///
/// The composer is installed in every devshell now, so "which copy am I using" is a question
/// that comes up whenever a rule change does not seem to take effect — and the answer is not
/// visible in any other command's output.
fn cmd_root(opts: &Opts) -> Result<ExitCode, String> {
    let (root, source) = root_of_with_source(opts)?;
    println!("{} · {}", root.display(), source.as_str());
    Ok(ExitCode::SUCCESS)
}

fn cmd_projects(opts: &Opts) -> Result<ExitCode, String> {
    let root = root_of(opts)?;

    println!("projects:");
    for name in list_projects(&root) {
        let project = load_project(&root, &name)?;
        println!(
            "  {:<22} {:<12} {}",
            name,
            project.org.unwrap_or_else(|| "-".into()),
            project.path
        );
    }

    println!("\ntargets:");
    for name in list_targets(&root) {
        let target = load_target(&root, &name)?;
        println!(
            "  {:<22} model={:<14} harness={:<14} file={}",
            name, target.model, target.harness, target.default_file
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// Every rule, and which projects it reaches. A rule that activates nowhere is the exact
/// failure the eight mis-filed skills had — present, plausible, and never loaded.
fn cmd_rules(opts: &Opts) -> Result<ExitCode, String> {
    let root = root_of(opts)?;
    let mut rows = census(&root)?;
    rows.sort_by(|a, b| {
        a.rule
            .layer
            .rank()
            .cmp(&b.rule.layer.rank())
            .then(a.rule.id.cmp(&b.rule.id))
    });

    let rule_width = rows.iter().map(|r| r.rule.rel.len()).max().unwrap_or(4);
    let layer_width = rows
        .iter()
        .map(|r| r.rule.layer.as_str().len())
        .max()
        .unwrap_or(5)
        .max(5);
    let act_width = rows
        .iter()
        .map(|r| r.rule.activation.as_str().len())
        .max()
        .unwrap_or(10)
        .max(10);

    println!(
        "  {:<layer_width$}  {:<act_width$}  {:<rule_width$}  projects",
        "layer", "activation", "rule"
    );
    println!("  {}", "-".repeat(layer_width + act_width + rule_width + 8));

    let mut orphans = 0;
    for row in &rows {
        let reach = if !row.projects.is_empty() {
            row.projects.join(", ")
        } else if row.rule.activation == Activation::Manual {
            if included_by_some_target(&root, &row.rule) {
                "manual (included by a target)".to_string()
            } else {
                orphans += 1;
                "UNREACHABLE — manual, and no target includes it".to_string()
            }
        } else {
            orphans += 1;
            "ORPHAN — no project activates this rule".to_string()
        };

        println!(
            "  {:<layer_width$}  {:<act_width$}  {:<rule_width$}  {}",
            row.rule.layer.as_str(),
            row.rule.activation.as_str(),
            row.rule.rel,
            reach
        );
    }

    println!("\n{} rules, {} unreachable", rows.len(), orphans);
    Ok(ExitCode::SUCCESS)
}
