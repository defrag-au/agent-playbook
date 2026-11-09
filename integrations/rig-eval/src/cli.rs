//! The command line, and the paths it resolves.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use clap::Parser;

const DEFAULT_MODEL: &str = "deepseek/deepseek-flash";
const DEFAULT_RIG: &str = ".tmp/rig-fork/bin/rig";
const DEFAULT_ENV: &str = ".tmp/rig-home/model.env";
const DEFAULT_SCENARIOS: &str = "integrations/rig-eval/scenarios";

/// The command line. Clap owns `--help`, `--version` and the usage errors.
#[derive(Debug, Parser)]
#[command(
    name = "rig-eval",
    version,
    about = "Run the playbook's rig homes over real-work scenarios and score them",
    after_help = "Every scenario is run under every home. The result is a score (checks passed),\n\
                  recall and precision over `path:line` citations, model calls, compactions,\n\
                  tool calls by name, refusals, output bytes and wall time. The raw `--json`\n\
                  stream and a copy of the effect log sit beside `results.json`."
)]
struct Cli {
    /// Run only this scenario; repeatable
    #[arg(long = "scenario", value_name = "NAME")]
    only: Vec<String>,

    /// A RIG_HOME to run under, as LABEL=PATH; repeatable
    #[arg(long = "home", value_name = "LABEL=PATH")]
    homes: Vec<String>,

    /// The model every run uses
    #[arg(long, default_value = DEFAULT_MODEL, value_name = "VENDOR/MODEL")]
    model: String,

    /// The rig launcher [default: .tmp/rig-fork/bin/rig]
    #[arg(long, value_name = "PATH")]
    rig: Option<PathBuf>,

    /// A file of KEY=VALUE lines holding the model key [default: .tmp/rig-home/model.env]
    #[arg(long = "env", value_name = "PATH")]
    env_file: Option<PathBuf>,

    /// Scenario JSON [default: integrations/rig-eval/scenarios]
    #[arg(long = "scenarios", value_name = "DIR")]
    scenario_dir: Option<PathBuf>,

    /// Where the streams and results go
    #[arg(long, required = true, value_name = "DIR")]
    out: PathBuf,

    /// Runs per scenario and home
    #[arg(long, value_name = "N", default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    repeat: u32,

    /// Kill a run that takes longer than this
    #[arg(long, value_name = "SECONDS")]
    timeout: Option<u64>,

    /// Validate the scenarios and homes, print the plan, run nothing
    #[arg(long)]
    dry_run: bool,
}

/// The command line with every path resolved.
pub struct Args {
    pub root: PathBuf,
    pub scenario_dir: PathBuf,
    pub only: Vec<String>,
    pub homes: Vec<Home>,
    pub model: String,
    pub rig: PathBuf,
    pub env_file: PathBuf,
    pub out: PathBuf,
    pub repeat: u32,
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

        let homes = if cli.homes.is_empty() {
            default_homes(&root)
        } else {
            cli.homes
                .iter()
                .map(|value| parse_home(value))
                .collect::<Result<Vec<_>>>()?
        };
        let rig = cli
            .rig
            .map(|path| absolute(&path))
            .unwrap_or_else(|| root.join(DEFAULT_RIG));
        let env_file = cli
            .env_file
            .map(|path| absolute(&path))
            .unwrap_or_else(|| root.join(DEFAULT_ENV));

        let args = Self {
            root,
            scenario_dir,
            only: cli.only,
            homes,
            model: cli.model,
            rig,
            env_file,
            out: absolute(&cli.out),
            repeat: cli.repeat,
            timeout: cli.timeout.map(Duration::from_secs),
            dry_run: cli.dry_run,
        };
        args.validate()?;
        Ok(args)
    }

    /// A dry run may be planned before the launcher or a home exists; a real run may not.
    fn validate(&self) -> Result<()> {
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
        for home in &self.homes {
            let settings = home.path.join("settings.toml");
            if !settings.is_file() {
                bail!(
                    "home `{}` has no settings.toml at {}",
                    home.label,
                    settings.display()
                );
            }
        }
        if !self.rig.is_file() {
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

fn default_homes(root: &Path) -> Vec<Home> {
    vec![
        Home {
            label: "gated".to_owned(),
            path: root.join(".tmp/rig-home"),
        },
        Home {
            label: "baseline".to_owned(),
            path: root.join(".tmp/rig-home-baseline"),
        },
    ]
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
}
