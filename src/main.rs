//! `playbook` — the command line over the resolution engine.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use playbook::frontmatter::split_list;
use playbook::init;
use playbook::install::{self, Outcome};
use playbook::load::{
    census, expand_tilde, find_manifest, find_root, find_root_with_source, list_targets,
    load_manifest, load_target, RootSource,
};
use playbook::model::{Activation, Diagnostic, Project, SkillScope, Target};
use playbook::render;
use playbook::resolve;
use playbook::skills;

const USAGE: &str = "\
playbook — resolve and render agent rules for a project + target

usage: playbook <command> [options]

commands
  compose    print the managed block
  list       print the resolution table for a repository + target
  install    write the block into a repository's agent file
  check      exit 1 if a repository's block is stale
  init       scaffold a repository's .playbook/ manifest and rules directory
  skills     deliver the skills a repository declares: `skills sync`, `skills check`
  root       print the rule tree being resolved against, and how it was found
  rules      list every shared rule, and the activation that reaches it

options
  --name <name>      project name to write (init; default: the directory name)
  --manifest <path>  a `.playbook/project.conf`, or the directory containing one
  --target <name>    target under models/ (default: the manifest's default_target).
                     `skills` takes a comma-separated list, or `all` for every harness that
                     declares a skill directory
  --repo <path>      repository root (install, check, init; default: the working directory)
  --file <name>      filename within the repo (default: the target's default_file)
  --out <file>       write to a file instead of stdout (compose)
  --offline          skills: use the skill-store cache without fetching
  --prune            skills sync: remove a delivered skill no longer declared
  --root <dir>       playbook root (default: found from the working directory, then
                     beside the installed binary — `playbook root` says which)
  -h, --help         this

examples
  playbook list
  playbook install --target zed
  playbook check                     # from inside the repository: neither is needed
  playbook check --manifest ~/code/DamonOehlman/fleet/personal/.playbook/project.conf --repo ~/.config/zed
  playbook init --name montager
  playbook skills sync --target claude-code
  playbook skills check --target claude-code   # what CI would run
  playbook skills sync --target all            # both harnesses at once
  playbook root
";

#[derive(Default)]
struct Opts {
    name: Option<String>,
    manifest: Option<String>,
    target: Option<String>,
    repo: Option<String>,
    file: Option<String>,
    out: Option<String>,
    root: Option<String>,
    offline: bool,
    prune: bool,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() || args[0] == "-h" || args[0] == "--help" || args[0] == "help" {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    let command = args[0].clone();
    if command == "skills" {
        return match cmd_skills(&args[1..]) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::from(2)
            }
        };
    }
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
        "--name",
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
        if key == "--offline" {
            opts.offline = true;
            i += 1;
            continue;
        }
        if key == "--prune" {
            opts.prune = true;
            i += 1;
            continue;
        }
        if !VALUE_OPTS.contains(&key) {
            return Err(format!("unknown option `{key}`"));
        }
        let value = args
            .get(i + 1)
            .cloned()
            .ok_or_else(|| format!("`{key}` needs a value"))?;
        match key {
            "--name" => opts.name = Some(value),
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

/// The repository whose `.playbook/project.conf` a command resolves.
///
/// `--manifest` names a manifest explicitly, which is how one can live outside the directory it
/// writes into (the personal file in `fleet`). Otherwise the nearest manifest at or above the
/// anchor is used.
fn choose_manifest(opts: &Opts, anchor: &Path) -> Result<PathBuf, String> {
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
        return Ok(repo.to_path_buf());
    }

    if let Some((repo, _)) = find_manifest(anchor) {
        return Ok(repo);
    }

    Err(format!(
        "no `.playbook/project.conf` at or above {} — run `playbook init`, or pass `--manifest \
         <path>`",
        anchor.display()
    ))
}

/// Resolve, report, and stop if anything was fatal.
fn resolved_or_exit(
    root: &Path,
    repo: &Path,
    target: Option<&str>,
) -> Result<Option<resolve::Resolved>, String> {
    let resolved = resolve::resolve_repo(root, repo, target)?;
    report(&resolved.diagnostics);
    if resolved.has_errors() {
        return Ok(None);
    }
    Ok(Some(resolved))
}

/// The repository whose instruction file a command writes or checks.
///
/// An explicit `--repo` is the answer. Otherwise the manifest's own directory is used, which is
/// the common case; the personal target passes both because its manifest lives in `fleet`.
fn output_repo(opts: &Opts, manifest_repo: PathBuf) -> PathBuf {
    opts.repo
        .as_deref()
        .map(expand_tilde)
        .unwrap_or(manifest_repo)
}

fn cmd_compose(opts: &Opts) -> Result<ExitCode, String> {
    let root = root_of(opts)?;
    let anchor = anchor(opts)?;
    let manifest_repo = choose_manifest(opts, &anchor)?;
    let Some(resolved) = resolved_or_exit(&root, &manifest_repo, opts.target.as_deref())? else {
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
    let manifest_repo = choose_manifest(opts, &anchor)?;
    let Some(resolved) = resolved_or_exit(&root, &manifest_repo, opts.target.as_deref())? else {
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
    let manifest_repo = choose_manifest(opts, &anchor)?;
    let repo = output_repo(opts, manifest_repo.clone());

    let Some(resolved) = resolved_or_exit(&root, &manifest_repo, opts.target.as_deref())? else {
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
            // The personal file's manifest lives outside the directory its block is written to,
            // so a hint that named only `--repo` would fail to find a manifest at all.
            let mut command = format!("playbook install --target {}", resolved.target.name);
            if let Some(manifest) = &opts.manifest {
                command.push_str(&format!(" --manifest {manifest}"));
            }
            command.push_str(&format!(" --repo {}", repo.display()));
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

/// `playbook skills sync|check` — deliver the skills a repository declares.
///
/// Deliberately not the same resolution `install` performs: the rule tree is not read at all, so a
/// repository whose rules fail to resolve can still receive its skills. The manifest supplies the
/// sources and the target supplies the directory.
fn cmd_skills(args: &[String]) -> Result<ExitCode, String> {
    let Some(verb) = args.first() else {
        return Err(
            "`skills` needs a verb — `playbook skills sync` or `playbook skills check`".to_string(),
        );
    };
    let opts = parse_opts(&args[1..])?;
    match verb.as_str() {
        "sync" => run_skills(&opts, false),
        "check" => run_skills(&opts, true),
        other => Err(format!(
            "unknown `skills` verb `{other}` — expected `sync` or `check`"
        )),
    }
}

fn run_skills(opts: &Opts, check: bool) -> Result<ExitCode, String> {
    let (root, source) = root_of_with_source(opts)?;
    note_root(&root, source);
    let anchor = anchor(opts)?;
    let manifest_repo = choose_manifest(opts, &anchor)?;

    let (project, diagnostics) = load_manifest(&manifest_repo)?;
    report(&diagnostics);

    // A global scope delivers outside any repository, so `--repo` names nothing it could write
    // into — say so rather than accepting a flag that has no effect.
    let scope = project.skills_scope;
    let repo = match scope {
        SkillScope::Repo => Some(output_repo(opts, manifest_repo.clone())),
        SkillScope::Global => {
            if opts.repo.is_some() {
                eprintln!(
                    "note: `--repo` is ignored — `skills_scope: global` delivers into the harness \
                     directories, and the lock stays beside the manifest"
                );
            }
            None
        }
    };
    let lock_dir = repo.clone().unwrap_or_else(|| manifest_repo.clone());

    let requested = requested_target(&root, &project, opts.target.as_deref())?;
    let (targets, target_diagnostics) =
        resolve_skill_targets(&root, scope, &split_list(&requested), opts.target.is_some())?;
    report(&target_diagnostics);
    let destinations = skills::destinations(
        &targets,
        scope,
        repo.as_deref().unwrap_or_else(|| Path::new(".")),
    );

    let result = if check {
        skills::check(&lock_dir, &project, &destinations, opts.offline)?
    } else {
        skills::sync(&lock_dir, &project, &destinations, opts.offline, opts.prune)?
    };

    // Grouped by destination, because the directory is the thing the reader acts on and the
    // alternative is repeating it on every line.
    for destination in &destinations {
        let lines: Vec<&skills::Line> = result
            .lines
            .iter()
            .filter(|line| line.target == destination.target)
            .collect();
        if lines.is_empty() {
            continue;
        }
        println!("{} — {}", destination.target, destination.root.display());
        for line in lines {
            println!(
                "  {}: {}{} — {}",
                line.verdict.as_str(),
                line.name,
                if line.patched { " (patched)" } else { "" },
                line.detail
            );
        }
    }
    for warning in &result.warnings {
        eprintln!("warning: {warning}");
    }
    if result.lines.is_empty() && result.warnings.is_empty() {
        println!(
            "no skills declared in {}",
            manifest_repo.join(".playbook/project.conf").display()
        );
    }

    if check && !result.ok() {
        // The manifest may live outside the directory its trees are delivered into, and an
        // offline check is only reproducible offline — both have to survive into the hint.
        let mut command = format!("playbook skills sync --target {requested}");
        if let Some(manifest) = &opts.manifest {
            command.push_str(&format!(" --manifest {manifest}"));
        }
        if let Some(repo) = &repo {
            command.push_str(&format!(" --repo {}", repo.display()));
        }
        if opts.offline {
            command.push_str(" --offline");
        }
        eprintln!("\nrun: {command}");
        return Ok(ExitCode::from(1));
    }

    Ok(ExitCode::SUCCESS)
}

/// Which targets a run delivers for.
///
/// `--target` wins. Otherwise the manifest's `default_target` — **unless** that target has no
/// directory for this scope. A personal catalogue's `default_target: zed-personal` names the
/// instructions file, not a skill directory, so the honest default there is every harness rather
/// than an error that tells the caller to add a key to an overlay that should not have one.
///
/// The fallback is global-only on purpose: a repository run silently expanding from one harness to
/// all of them is a surprise, and the error names the key that fixes it.
fn requested_target(
    root: &Path,
    project: &Project,
    explicit: Option<&str>,
) -> Result<String, String> {
    if let Some(target) = explicit {
        return Ok(target.to_string());
    }
    let default = project
        .default_target
        .clone()
        .unwrap_or_else(|| "zed".to_string());
    // The overlay's own diagnostics are dropped here on purpose: this only peeks at the two skill
    // directories, and `resolve_skill_targets` loads the same target again (directly, or through
    // `all`) and is where they are reported.
    let (target, _) = load_target(root, &default)?;
    let dir = match project.skills_scope {
        SkillScope::Repo => &target.skill_dir,
        SkillScope::Global => &target.skill_global_dir,
    };
    if dir.is_empty() && project.skills_scope == SkillScope::Global {
        return Ok("all".to_string());
    }
    Ok(default)
}

/// Load the targets a run delivers for.
///
/// `all` expands to every model directory and skips those that do not declare a skill directory
/// for this scope — that is what makes `--target all` one command for both harnesses. A target
/// named explicitly must deliver skills, or the caller has said something they did not mean.
/// The targets a skills run delivers for, with what is wrong with any of their overlays.
type SkillTargets = (Vec<(String, Target)>, Vec<Diagnostic>);

fn resolve_skill_targets(
    root: &Path,
    scope: SkillScope,
    names: &[String],
    explicit: bool,
) -> Result<SkillTargets, String> {
    let all = names.iter().any(|name| name == "all");
    let wanted = if all {
        list_targets(root)
    } else {
        names.to_vec()
    };
    let key = match scope {
        SkillScope::Repo => "skill_dir",
        SkillScope::Global => "skill_global_dir",
    };

    let mut out = Vec::new();
    let mut diagnostics = Vec::new();
    for name in wanted {
        let (target, target_diagnostics) = load_target(root, &name)?;
        diagnostics.extend(target_diagnostics);
        let dir = match scope {
            SkillScope::Repo => &target.skill_dir,
            SkillScope::Global => &target.skill_global_dir,
        };
        if dir.is_empty() {
            if all {
                continue;
            }
            // A manifest whose default target is an instructions target (`zed-personal`) is not
            // misconfigured — it just has no skill directory, and the fix is a flag, not a key.
            return Err(if explicit {
                format!(
                    "target `{name}` does not deliver {} skills — add `{key}:` to {}",
                    scope.as_str(),
                    root.join("models")
                        .join(&name)
                        .join("overlay.conf")
                        .display()
                )
            } else {
                format!(
                    "`{name}` is this manifest's default_target and has no {} skill directory — \
                     pass `--target all`, or a comma-separated list of harnesses",
                    scope.as_str()
                )
            });
        }
        out.push((name, target));
    }

    if out.is_empty() {
        return Err(format!(
            "no target declares a {} skill directory (`{key}:`) — `playbook skills` has nowhere \
             to deliver",
            scope.as_str()
        ));
    }
    Ok((out, diagnostics))
}

/// Scaffold a repository's `.playbook/` manifest and rules directory.
fn cmd_init(opts: &Opts) -> Result<ExitCode, String> {
    if opts.manifest.is_some() {
        return Err("`init` creates a manifest; `--manifest` is not applicable".to_string());
    }
    let repo = anchor(opts)?;
    let name = match &opts.name {
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

/// Every shared rule, and the activation that reaches it. A rule that activates nowhere is the
/// exact failure the eight mis-filed skills had — present, plausible, and never loaded.
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
        "  {:<layer_width$}  {:<act_width$}  {:<rule_width$}  reaches",
        "layer", "activation", "rule"
    );
    println!("  {}", "-".repeat(layer_width + act_width + rule_width + 8));

    let mut orphans = 0;
    for row in &rows {
        let reach = if !row.reach.is_empty() {
            row.reach.clone()
        } else if row.rule.activation == Activation::Manual {
            orphans += 1;
            "UNREACHABLE — manual, and no target includes it".to_string()
        } else {
            orphans += 1;
            "MISPLACED — a `project:` rule belongs in that repository's `.playbook/rules/`"
                .to_string()
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
