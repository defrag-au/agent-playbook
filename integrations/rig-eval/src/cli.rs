//! The command line, and the paths it resolves.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use clap::Parser;

use crate::target::Target;

const DEFAULT_MODEL: &str = "deepseek/deepseek-flash";
const DEFAULT_RIG: &str = ".tmp/rig-fork/bin/rig";
const DEFAULT_ENV: &str = ".tmp/rig-home/model.env";
const DEFAULT_SCENARIOS: &str = "integrations/rig-eval/scenarios";
const DEFAULT_OPENCODE: &str = "bunx";
const DEFAULT_OPENCODE_CONFIG: &str = ".tmp/opencode/opencode.json";
const DEFAULT_PI: &str = ".tmp/pi/bin/pi";

/// The command line. Clap owns `--help`, `--version` and the usage errors.
#[derive(Debug, Parser)]
#[command(
    name = "rig-eval",
    version,
    about = "Run the harness comparison over real-work scenarios and score it",
    after_help = "Every scenario is run under every target. The result is a score (checks passed),\n\
                  recall and precision over `path:line` citations, model calls, compactions,\n\
                  tool calls by name, refusals, output bytes and wall time. The raw `--json`\n\
                  stream and a copy of the effect log sit beside `results.json`."
)]
struct Cli {
    /// Run only this scenario; repeatable
    #[arg(long = "scenario", value_name = "NAME")]
    only: Vec<String>,

    /// A target: rig:advertised, rig:slim, rig:gated, rig:permissive, rig:terse, rig:lean, rig:baseline, rig:vanilla, opencode, pi; repeatable
    #[arg(long = "target", value_name = "NAME")]
    targets: Vec<String>,

    /// A custom rig home, as LABEL=PATH; repeatable
    #[arg(long = "home", value_name = "LABEL=PATH")]
    homes: Vec<String>,

    /// The model every run uses
    #[arg(long, default_value = DEFAULT_MODEL, value_name = "VENDOR/MODEL")]
    model: String,

    /// The rig launcher [default: .tmp/rig-fork/bin/rig]
    #[arg(long, value_name = "PATH")]
    rig: Option<PathBuf>,

    /// The opencode provider config [default: .tmp/opencode/opencode.json]
    #[arg(long = "opencode-config", value_name = "PATH")]
    opencode_config: Option<PathBuf>,

    /// The Pi launcher [default: .tmp/pi/bin/pi]
    #[arg(long, value_name = "PATH")]
    pi: Option<PathBuf>,

    /// A file of KEY=VALUE lines holding the model key [default: .tmp/rig-home/model.env]
    #[arg(long = "env", value_name = "PATH")]
    env_file: Option<PathBuf>,

    /// Scenario JSON [default: integrations/rig-eval/scenarios]
    #[arg(long = "scenarios", value_name = "DIR")]
    scenario_dir: Option<PathBuf>,

    /// Where the streams and results go
    #[arg(long, required = true, value_name = "DIR")]
    out: PathBuf,

    /// Re-export an existing results.json as Observable files, without running anything
    #[arg(long, value_name = "DIR")]
    reuse: Option<PathBuf>,

    /// Runs per scenario and target
    #[arg(long, value_name = "N", default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    repeat: u32,

    /// Runs to execute in parallel, each in its own snapshot checkout
    #[arg(long, value_name = "N", default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    jobs: u32,

    /// Kill a run that takes longer than this
    #[arg(long, value_name = "SECONDS")]
    timeout: Option<u64>,

    /// Validate the scenarios and targets, print the plan, run nothing
    #[arg(long)]
    dry_run: bool,
}

/// The command line with every path resolved.
pub struct Args {
    pub root: PathBuf,
    pub scenario_dir: PathBuf,
    pub only: Vec<String>,
    pub targets: Vec<Target>,
    pub model: String,
    pub rig: PathBuf,
    pub opencode_config: PathBuf,
    pub pi: PathBuf,
    pub env_file: PathBuf,
    pub out: PathBuf,
    pub reuse: Option<PathBuf>,
    pub repeat: u32,
    pub jobs: usize,
    pub timeout: Option<Duration>,
    pub dry_run: bool,
}

#[derive(Debug, Clone)]
pub struct Home {
    pub label: String,
    pub path: PathBuf,
}

impl Args {
    pub fn parse() -> Result<Self> {
        let cli = Cli::parse();
        let root = repo_root()?;
        let scenario_dir = cli
            .scenario_dir
            .map(|path| absolute(&path))
            .unwrap_or_else(|| root.join(DEFAULT_SCENARIOS));

        let mut targets = cli
            .targets
            .iter()
            .map(|spec| parse_target(&root, spec))
            .collect::<Result<Vec<_>>>()?;
        for value in &cli.homes {
            targets.push(Target::Rig(parse_home(value)?));
        }
        if targets.is_empty() {
            targets = default_targets(&root);
        }

        let args = Self {
            root: root.clone(),
            scenario_dir,
            only: cli.only,
            targets,
            model: cli.model,
            rig: cli
                .rig
                .map(|path| absolute(&path))
                .unwrap_or_else(|| root.join(DEFAULT_RIG)),
            opencode_config: cli
                .opencode_config
                .map(|path| absolute(&path))
                .unwrap_or_else(|| root.join(DEFAULT_OPENCODE_CONFIG)),
            pi: cli
                .pi
                .map(|path| absolute(&path))
                .unwrap_or_else(|| root.join(DEFAULT_PI)),
            env_file: cli
                .env_file
                .map(|path| absolute(&path))
                .unwrap_or_else(|| root.join(DEFAULT_ENV)),
            out: absolute(&cli.out),
            reuse: cli.reuse.map(|path| absolute(&path)),
            repeat: cli.repeat,
            jobs: cli.jobs as usize,
            timeout: cli.timeout.map(Duration::from_secs),
            dry_run: cli.dry_run,
        };
        args.validate()?;
        Ok(args)
    }

    /// A dry run may be planned before a launcher or a home exists; a real run may not.
    fn validate(&self) -> Result<()> {
        if self.reuse.is_some() {
            return Ok(());
        }
        if self.dry_run {
            return Ok(());
        }
        // The corpus ships in the checkout. The sandbox is what keeps the agent from reading its
        // own gold, so an in-checkout corpus without one is refused rather than treated as
        // evidence.
        if is_inside(&self.root, &self.scenario_dir) && !crate::sandbox::available() {
            bail!(
                "--scenarios {} is inside the checkout and no sandbox is available; copy the \
                 corpus outside the checkout, or run where `sandbox-exec` is available",
                self.scenario_dir.display()
            );
        }
        for target in &self.targets {
            match target {
                Target::Rig(home) => {
                    let settings = home.path.join("settings.toml");
                    if !settings.is_file() {
                        bail!(
                            "home `{}` has no settings.toml at {}",
                            home.label,
                            settings.display()
                        );
                    }
                }
                Target::Opencode => {
                    if !self.opencode_config.is_file() {
                        bail!(
                            "no opencode provider config at {}",
                            self.opencode_config.display()
                        );
                    }
                    if !on_path(Path::new(DEFAULT_OPENCODE)) {
                        bail!(
                            "no `{DEFAULT_OPENCODE}` on PATH; opencode runs through it, pinned to \
                             opencode-ai@1.19.5"
                        );
                    }
                }
                Target::Pi => {
                    if !self.pi.is_file() {
                        bail!(
                            "no Pi launcher at {}; install it with `npm install -g --prefix .tmp/pi --ignore-scripts @earendil-works/pi-coding-agent`",
                            self.pi.display()
                        );
                    }
                }
            }
        }
        if self.targets.iter().any(Target::is_rig) && !self.rig.is_file() {
            bail!(
                "no rig launcher at {}; build it with `cargo install --path \
                 ~/code/defrag/rig/crates/rig-cli --locked --force --root .tmp/rig-fork`",
                self.rig.display()
            );
        }
        Ok(())
    }
}

/// `KEY=VALUE` lines. A value is never printed: the summary names the keys only.
pub fn read_env(path: &Path) -> Result<Vec<(String, String)>> {
    let entries = dotenvy::from_path_iter(path)
        .with_context(|| format!("reading {} (write the model key there)", path.display()))?;
    let mut vars = Vec::new();
    for entry in entries {
        let (key, value) = entry.with_context(|| format!("parsing {}", path.display()))?;
        if key.trim().is_empty() || value.trim().is_empty() {
            bail!("{}: `{key}` has no value", path.display());
        }
        vars.push((key, value));
    }
    if vars.is_empty() {
        bail!("{}: no KEY=VALUE lines", path.display());
    }
    Ok(vars)
}

/// The checkout this crate is in: `integrations/rig-eval` walks up twice.
fn repo_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("the crate is not inside a checkout"))
}

fn default_targets(root: &Path) -> Vec<Target> {
    vec![
        rig_target(root, "gated", ".tmp/rig-home"),
        rig_target(root, "baseline", ".tmp/rig-home-baseline"),
    ]
}

fn rig_target(root: &Path, label: &str, path: &str) -> Target {
    Target::Rig(Home {
        label: label.to_owned(),
        path: root.join(path),
    })
}

fn parse_target(root: &Path, spec: &str) -> Result<Target> {
    match spec {
        "rig:advertised" => Ok(rig_target(root, "advertised", ".tmp/rig-home-advertised")),
        "rig:slim" => Ok(rig_target(root, "slim", ".tmp/rig-home-slim")),
        "rig:gated" => Ok(rig_target(root, "gated", ".tmp/rig-home")),
        "rig:permissive" => Ok(rig_target(root, "permissive", ".tmp/rig-home-permissive")),
        "rig:terse" => Ok(rig_target(root, "terse", ".tmp/rig-home-terse")),
        "rig:lean" => Ok(rig_target(root, "lean", ".tmp/rig-home-lean")),
        "rig:baseline" => Ok(rig_target(root, "baseline", ".tmp/rig-home-baseline")),
        "rig:vanilla" => Ok(rig_target(root, "vanilla", ".tmp/rig-home-vanilla")),
        "opencode" => Ok(Target::Opencode),
        "pi" => Ok(Target::Pi),
        other => bail!(
            "unknown target `{other}`; known: rig:advertised, rig:slim, rig:gated, rig:permissive, rig:terse, rig:lean, rig:baseline, rig:vanilla, opencode, pi"
        ),
    }
}

fn parse_home(value: &str) -> Result<Home> {
    let (label, path) = match value.split_once('=') {
        Some((label, path)) => (label.to_owned(), path),
        None => {
            let path = Path::new(value);
            let label = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(value)
                .to_owned();
            (label, value)
        }
    };
    if label.is_empty() {
        bail!("`--home {value}` has no label");
    }
    Ok(Home {
        label,
        path: absolute(Path::new(path)),
    })
}

/// An explicit path is relative to where the runner was invoked; a default is built from the root.
fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
    }
}

fn on_path(program: &Path) -> bool {
    if program.is_absolute() || program.components().count() > 1 {
        return program.is_file();
    }
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|directory| directory.join(program).is_file())
    })
}

fn is_inside(root: &Path, path: &Path) -> bool {
    let (Ok(root), Ok(path)) = (root.canonicalize(), path.canonicalize()) else {
        return false;
    };
    path.starts_with(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_home_is_a_label_and_a_path() {
        let named = parse_home("gated=/tmp/rig-home").expect("a named home");
        assert_eq!(named.label, "gated");
        assert_eq!(named.path, PathBuf::from("/tmp/rig-home"));

        let bare = parse_home("/tmp/rig-home-baseline").expect("a bare home");
        assert_eq!(bare.label, "rig-home-baseline");
    }

    #[test]
    fn an_empty_label_is_refused() {
        assert!(parse_home("=/tmp/rig-home").is_err());
    }

    #[test]
    fn the_named_targets_resolve_to_their_homes() {
        let root = Path::new("/repo");
        let Target::Rig(home) = parse_target(root, "rig:vanilla").expect("a target") else {
            panic!("rig:vanilla is a rig home");
        };
        assert_eq!(home.label, "vanilla");
        assert_eq!(home.path, PathBuf::from("/repo/.tmp/rig-home-vanilla"));
        assert!(matches!(
            parse_target(root, "opencode").expect("a target"),
            Target::Opencode
        ));
    }
}
