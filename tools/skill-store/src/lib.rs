//! `skill-store` — an OCFL store for agent skills.
//!
//! The store is the artifact: one OCFL object per skill, one immutable version per release.
//! The export is the delivery: a plain directory an agent can read, plus a flat manifest that a
//! target-aware consumer can record in its lock. See README.md.

pub mod digest;
pub mod export;
pub mod fsx;
pub mod git;
pub mod identity;
pub mod inventory;
pub mod publish;
pub mod skill;
pub mod source;
pub mod status;
pub mod store;
pub mod verify;

use std::path::Path;

use crate::store::Store;

const USAGE: &str = "\
skill-store — publish and export OCFL-stored agent skills

usage: skill-store <command> [options]

commands
  export <source> --out <dir>          export one version as an envelope (skill/ + export.conf)
                                       a `path:` to a plain skill directory exports as `working`
  list <source>                        list the objects in a store, with head and description
  verify <source> [<skill>]            check inventories, digests and content paths
  status --store <path> --worktree <dir>  compare a store with the directories it publishes
  publish <dir> --store <path>         publish a plain skill directory as a new version
  publish-changed --store <path> --worktree <dir>  publish every skill that has moved on
  source <source>                      print how a source resolves (for debugging)

source
  <store>#<skill>[@v<N>]
  github:<owner>/<repo>[/<ref>][?rev=<sha>&dir=<subdir>]
  gitlab:<owner>/<repo>[/<ref>][?rev=<sha>&dir=<subdir>]
  git+https://<host>/<path>[?ref=<ref>&rev=<sha>&dir=<subdir>]
  path:<dir>

  A store is either the directory with `0=ocfl_1.1` or a `.skill-store/` directory inside it.
  Omitting `@vN` selects the object's head; a consumer's lock should freeze it.

  Each version records who published it, read from the repository the store lives in: the name
  from `git config user.name` and the address from `origin`, normalised to a URL. A store outside
  a git checkout has no address to record and writes no user block at all.

options
  --out <dir>        export destination (required for export)
  --store <path>     storage root to publish into (publish, publish-changed, status)
  --worktree <dir>   directory holding one subdirectory per skill (status, publish-changed)
  --id <uri>         object id (default: urn:skill:<name>)
  --message <text>   version message
  --user <name>      version user (default: `git config user.name`)
  --address <url>    version user address (default: the store repository's `origin`)
  --version <vN>     verify or export one version
  --skill <name>     verify one object
  --init             create the storage root if it does not exist
  --offline          use the git cache without fetching
  -h, --help         this
";

#[derive(Default)]
struct Opts {
    out: Option<String>,
    store: Option<String>,
    worktree: Option<String>,
    id: Option<String>,
    message: Option<String>,
    user: Option<String>,
    address: Option<String>,
    skill: Option<String>,
    version: Option<String>,
    offline: bool,
    init: bool,
    positional: Vec<String>,
}

pub fn run(args: &[String]) -> i32 {
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" || args[0] == "help" {
        print!("{USAGE}");
        return 0;
    }
    let command = args[0].clone();
    let opts = match parse(&args[1..]) {
        Ok(opts) => opts,
        Err(error) => return usage(&error),
    };
    match command.as_str() {
        "export" => cmd_export(&opts),
        "list" => cmd_list(&opts),
        "verify" => cmd_verify(&opts),
        "status" => cmd_status(&opts),
        "publish" => cmd_publish(&opts),
        "publish-changed" => cmd_publish_changed(&opts),
        "source" => cmd_source(&opts),
        other => usage(&format!("unknown command `{other}`")),
    }
}

fn parse(args: &[String]) -> Result<Opts, String> {
    const VALUE: &[&str] = &[
        "--out",
        "--store",
        "--worktree",
        "--id",
        "--message",
        "--user",
        "--address",
        "--skill",
        "--version",
    ];
    let mut opts = Opts::default();
    let mut index = 0;
    while index < args.len() {
        let key = args[index].as_str();
        match key {
            "-h" | "--help" => return Err("help".to_string()),
            "--offline" => opts.offline = true,
            "--init" => opts.init = true,
            other if VALUE.contains(&other) => {
                let value = args
                    .get(index + 1)
                    .cloned()
                    .ok_or_else(|| format!("`{other}` needs a value"))?;
                match other {
                    "--out" => opts.out = Some(value),
                    "--store" => opts.store = Some(value),
                    "--worktree" => opts.worktree = Some(value),
                    "--id" => opts.id = Some(value),
                    "--message" => opts.message = Some(value),
                    "--user" => opts.user = Some(value),
                    "--address" => opts.address = Some(value),
                    "--skill" => opts.skill = Some(value),
                    "--version" => opts.version = Some(value),
                    _ => unreachable!(),
                }
                index += 1;
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option `{other}`"));
            }
            positional => opts.positional.push(positional.to_string()),
        }
        index += 1;
    }
    Ok(opts)
}

fn usage(error: &str) -> i32 {
    if error != "help" {
        eprintln!("error: {error}");
    }
    eprint!("\n{USAGE}");
    if error == "help" {
        0
    } else {
        2
    }
}

fn operation(error: String) -> i32 {
    eprintln!("error: {error}");
    1
}

fn one_positional(opts: &Opts, command: &str) -> Result<String, String> {
    match opts.positional.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(format!("`{command}` needs a source")),
        _ => Err(format!("`{command}` takes one source")),
    }
}

fn open(source: &str, offline: bool) -> Result<(source::Source, Store), String> {
    let parsed = source::parse(source)?;
    let store = Store::open(&parsed, offline)?;
    Ok((parsed, store))
}

fn cmd_export(opts: &Opts) -> i32 {
    let source = match one_positional(opts, "export") {
        Ok(source) => source,
        Err(error) => return usage(&error),
    };
    let Some(out) = &opts.out else {
        return usage("`export` needs `--out <dir>`");
    };
    let out = Path::new(out);
    let parsed = match source::parse(&source) {
        Ok(parsed) => parsed,
        Err(error) => return operation(error),
    };

    // A `path:` that is not a store but holds a `SKILL.md` is a working copy — what a development
    // override points at. Everything else has to be a published version.
    if let source::Locator::Path(dir) = &parsed.locator {
        if store::find_root(dir).is_none() && dir.join("SKILL.md").is_file() {
            return match export::export_working(&source, dir, parsed.skill.as_deref(), out) {
                Ok(report) => {
                    println!(
                        "exported {} {} {} → {}/skill ({} files, {})",
                        report.name,
                        report.id,
                        report.version,
                        out.display(),
                        report.files,
                        report.tree_digest
                    );
                    0
                }
                Err(error) => operation(error),
            };
        }
    }

    let store = match Store::open(&parsed, opts.offline) {
        Ok(store) => store,
        Err(error) => return operation(error),
    };
    let Some(skill) = &parsed.skill else {
        return usage("`export` needs a skill: `<store>#<skill>[@v<N>]`");
    };
    match export::export(&store, skill, parsed.version.as_deref(), out) {
        Ok(report) => {
            println!(
                "exported {} {} {} → {}/skill ({} files, {})",
                report.name,
                report.id,
                report.version,
                out.display(),
                report.files,
                report.tree_digest
            );
            0
        }
        Err(error) => operation(error),
    }
}

fn cmd_list(opts: &Opts) -> i32 {
    let source = match one_positional(opts, "list") {
        Ok(source) => source,
        Err(error) => return usage(&error),
    };
    let (_, store) = match open(&source, opts.offline) {
        Ok(pair) => pair,
        Err(error) => return operation(error),
    };
    let objects = match store.objects() {
        Ok(objects) => objects,
        Err(error) => return operation(error),
    };
    if objects.is_empty() {
        println!("{}: no objects", store.root.display());
        return 0;
    }
    for object in &objects {
        let inventory = &object.inventory;
        let versions: Vec<String> = inventory.versions.keys().cloned().collect();
        let description = inventory
            .state_for(&inventory.head)
            .ok()
            .and_then(|state| state.get("SKILL.md").cloned())
            .and_then(|digest| inventory.manifest.get(&digest).cloned())
            .and_then(|paths| paths.first().cloned())
            .and_then(|path| std::fs::read_to_string(object.root.join(path)).ok())
            .and_then(|body| skill::description(&body));
        println!(
            "{}  head {}  versions {}  {}",
            inventory.id,
            inventory.head,
            versions.join(","),
            description.unwrap_or_else(|| "-".to_string())
        );
        println!("  {}", object.root.display());
    }
    0
}

fn cmd_verify(opts: &Opts) -> i32 {
    let Some(source) = opts.positional.first().cloned() else {
        return usage("`verify` needs a source");
    };
    if opts.positional.len() > 2 {
        return usage("`verify` takes a source and at most one skill name");
    }
    let skill = opts
        .skill
        .clone()
        .or_else(|| opts.positional.get(1).cloned());
    let (_, store) = match open(&source, opts.offline) {
        Ok(pair) => pair,
        Err(error) => return operation(error),
    };
    match verify::verify(&store, skill.as_deref(), opts.version.as_deref()) {
        Ok(report) if report.errors.is_empty() => {
            println!(
                "ok: {} object(s), {} file(s) checked",
                report.objects, report.files
            );
            0
        }
        Ok(report) => {
            for error in &report.errors {
                eprintln!("{error}");
            }
            eprintln!(
                "\n{} error(s) across {} object(s)",
                report.errors.len(),
                report.objects
            );
            1
        }
        Err(error) => operation(error),
    }
}

/// `status` — is the published head still what the authoring directory says?
///
/// Read-only, so it is the command a pull request runs. Both directions count as drift: a
/// directory nobody published, and an object nobody can edit.
fn cmd_status(opts: &Opts) -> i32 {
    let Some(store) = &opts.store else {
        return usage("`status` needs `--store <path>`");
    };
    let Some(worktree) = &opts.worktree else {
        return usage("`status` needs `--worktree <dir>`");
    };
    let store = fsx::expand_tilde(store);
    let worktree = fsx::expand_tilde(worktree);

    let readings = match status::compare(&store, &worktree) {
        Ok(readings) => readings,
        Err(error) => return operation(error),
    };
    if readings.is_empty() {
        println!(
            "no objects in {} and no skills in {}",
            store.display(),
            worktree.display()
        );
        return 0;
    }

    let mut drift = Vec::new();
    for reading in &readings {
        match &reading.state {
            status::State::Current => {
                println!("current     {} {}", reading.name, reading.head);
            }
            status::State::Ahead(difference) => {
                println!(
                    "ahead       {} {} — {}",
                    reading.name,
                    reading.head,
                    difference.describe()
                );
                drift.push(reading.name.clone());
            }
            status::State::Missing => {
                println!("missing     {} — no object", reading.name);
                drift.push(reading.name.clone());
            }
            status::State::StoreOnly => {
                println!(
                    "store-only  {} {} — no directory",
                    reading.name, reading.head
                );
                drift.push(reading.name.clone());
            }
        }
    }

    if drift.is_empty() {
        println!("\nok: {} skill(s) in sync", readings.len());
        return 0;
    }

    eprintln!(
        "\n{} of {} skill(s) out of sync: {}",
        drift.len(),
        readings.len(),
        drift.join(", ")
    );
    eprintln!(
        "ahead or missing → skill-store publish-changed --store {} --worktree {}",
        store.display(),
        worktree.display()
    );
    eprintln!("store-only → add the directory it is published from, or drop the object");
    1
}

fn cmd_publish(opts: &Opts) -> i32 {
    let from = match one_positional(opts, "publish") {
        Ok(from) => from,
        Err(error) => return usage(&error),
    };
    let Some(store) = &opts.store else {
        return usage("`publish` needs `--store <path>`");
    };
    let store = fsx::expand_tilde(store);
    let options = publish::Options {
        id: opts.id.as_deref(),
        message: opts.message.as_deref(),
        user: opts.user.as_deref(),
        address: opts.address.as_deref(),
        init: opts.init,
    };
    match publish::publish(Path::new(&from), &store, &options) {
        Ok(report) => {
            println!(
                "published {} {} (+{} ~{} -{} ={})",
                report.id,
                report.version,
                report.added,
                report.changed,
                report.removed,
                report.unchanged
            );
            0
        }
        Err(error) => operation(error),
    }
}

fn cmd_publish_changed(opts: &Opts) -> i32 {
    if !opts.positional.is_empty() {
        return usage("`publish-changed` takes no positional arguments — it reads `--worktree`");
    }
    let (Some(store), Some(worktree)) = (&opts.store, &opts.worktree) else {
        return usage("`publish-changed` needs `--store <path>` and `--worktree <dir>`");
    };
    let store = fsx::expand_tilde(store);
    let worktree = fsx::expand_tilde(worktree);
    let options = publish::Options {
        id: None,
        message: opts.message.as_deref(),
        user: opts.user.as_deref(),
        address: opts.address.as_deref(),
        init: opts.init,
    };
    match publish::publish_changed(&store, &worktree, &options) {
        Ok(changed) => {
            for report in &changed.published {
                println!(
                    "published {} {} (+{} ~{} -{} ={})",
                    report.id,
                    report.version,
                    report.added,
                    report.changed,
                    report.removed,
                    report.unchanged
                );
            }
            if changed.published.is_empty() {
                println!("nothing to publish");
            }
            for name in &changed.blocked {
                eprintln!(
                    "{name}: an object with no authoring directory — nobody can edit it; add \
                     {}/{name} or drop the object",
                    worktree.display()
                );
            }
            if changed.blocked.is_empty() {
                0
            } else {
                1
            }
        }
        Err(error) => operation(error),
    }
}

fn cmd_source(opts: &Opts) -> i32 {
    let source = match one_positional(opts, "source") {
        Ok(source) => source,
        Err(error) => return usage(&error),
    };
    match source::parse(&source) {
        Ok(parsed) => {
            println!("store:   {}", parsed.store_label());
            println!("skill:   {}", parsed.skill.as_deref().unwrap_or("-"));
            println!("version: {}", parsed.version.as_deref().unwrap_or("head"));
            match parsed.locator {
                source::Locator::Path(path) => println!("path:    {}", path.display()),
                source::Locator::Git {
                    url,
                    ref_,
                    rev,
                    dir,
                } => {
                    println!("url:     {url}");
                    println!("ref:     {}", ref_.as_deref().unwrap_or("-"));
                    println!("rev:     {}", rev.as_deref().unwrap_or("-"));
                    println!("dir:     {}", dir.as_deref().unwrap_or("-"));
                }
            }
            0
        }
        Err(error) => operation(error),
    }
}
