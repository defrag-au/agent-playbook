//! Container isolation: one trial, one container, one mounted snapshot.
//!
//! The image is the boundary. A trial's snapshot is mounted at `/repo`, the corpus is absent from
//! the image and from the mount, and the host's output directory is not reachable at all. That
//! replaces the seatbelt profile and the snapshot's rewritten history as the thing that keeps the
//! gold away from the agent, and it is the same boundary for every arm.
//!
//! The image also carries the harnesses and the toolkit, so a trial does not depend on the host's
//! `PATH`. Each rig home is baked with a warm target directory: `rig` runs `cargo build` before
//! every start, and a cold Bevy build per trial would dominate the measurement.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::process::Invocation;

/// The repository the image was built from: the toolkit, the plugin and the rig homes.
pub const IMAGE_ROOT: &str = "/workspace";
/// Where a trial's snapshot is mounted.
pub const WORKSPACE: &str = "/repo";
/// What the build script writes beside the image's other metadata.
pub const METADATA: &str = "/etc/playbook-eval.json";
/// Where an opencode trial mounts its provider config.
pub const OPENCODE_CONFIG: &str = "/opt/opencode/opencode.json";
/// The toolkit binaries, copied out of the image's release build.
pub const TOOLKIT_BIN: &str = "/opt/playbook/bin";
const RIG_BIN: &str = "/usr/local/bin/rig";
const PI_BIN: &str = "/usr/local/bin/pi";
const BUNX_BIN: &str = "/usr/local/bin/bunx";

/// The image's base `PATH`, without the toolkit. `cargo` must stay first for `rig`, which shells
/// out to it on every start.
const BASE_PATH: &str =
    "/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

/// The `PATH` an arm runs with. `absent` removes the toolkit directory rather than shadowing it,
/// because the image's own `PATH` is otherwise the only route to those binaries.
pub fn path(with_toolkit: bool) -> String {
    if with_toolkit {
        format!("{TOOLKIT_BIN}:{BASE_PATH}")
    } else {
        BASE_PATH.to_owned()
    }
}

/// The image path of an arm's home. It mirrors the host's `<repo>/.tmp/rig-home[-<label>]` depth
/// so a `settings.toml` that names `../../integrations/rig` resolves to the image's checkout.
pub fn home_dir(label: &str) -> String {
    if label == "gated" {
        format!("{IMAGE_ROOT}/.tmp/rig-home")
    } else {
        format!("{IMAGE_ROOT}/.tmp/rig-home-{label}")
    }
}

/// An inspected image: what a trial runs in, and what a manifest records.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Image {
    pub reference: String,
    pub digest: String,
    pub cpus: u32,
    pub memory: String,
}

/// What the image build baked, at [`METADATA`].
///
/// `settings` is the `git hash-object` of each home's `settings.toml`, so a run can refuse an image
/// whose arm is not the arm the host names. An image built from a different repository revision is
/// recorded rather than refused: the image fixes the harness, while the snapshot is the subject.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub head: String,
    pub settings: BTreeMap<String, String>,
}

/// What a run needs from its image. Validation happens once, before the first trial.
pub struct Requirements<'a> {
    /// `(home label, settings blob)` for every rig arm in the run.
    pub homes: Vec<(&'a str, &'a str)>,
    pub rig: bool,
    pub opencode: bool,
    pub pi: bool,
    pub toolkit: bool,
}

/// One trial's mounts and home.
#[derive(Debug, Clone)]
pub struct Trial {
    pub image: Image,
    /// The snapshot, mounted at [`WORKSPACE`].
    pub workspace: PathBuf,
    /// The rig home label baked into the image, when this arm has one.
    pub home: Option<String>,
    /// The host directory mounted at that home's `sessions`, so the effect log outlives the
    /// container.
    pub sessions: Option<PathBuf>,
    /// The host opencode config, mounted read-only.
    pub opencode_config: Option<PathBuf>,
}

impl Image {
    /// Inspect an image reference, recording the digest the trial actually ran.
    pub fn inspect(reference: &str, cpus: u32, memory: &str) -> Result<Self> {
        let output = Command::new("container")
            .args(["image", "inspect", reference])
            .output()
            .with_context(|| {
                "running `container image inspect`; is the container CLI on PATH? (Apple's \
                 `container`, not Docker)"
            })?;
        if !output.status.success() {
            bail!(
                "container image inspect {reference}: {}",
                stderr(&output.stderr)
            );
        }
        let inspected: Vec<Inspected> = serde_json::from_slice(&output.stdout)
            .with_context(|| format!("parsing `container image inspect {reference}`"))?;
        let image = inspected
            .into_iter()
            .next()
            .with_context(|| format!("`{reference}` is not an image"))?;
        let digest = if image.configuration.descriptor.digest.is_empty() {
            format!("sha256:{}", image.id)
        } else {
            image.configuration.descriptor.digest
        };
        Ok(Self {
            reference: reference.to_owned(),
            digest,
            cpus,
            memory: memory.to_owned(),
        })
    }

    /// Check that the image is the one the run's arms describe: every required home is present and
    /// was built from the host's settings, the required harnesses are installed, and the corpus is
    /// not inside the image.
    pub fn preflight(&self, requirements: &Requirements<'_>) -> Result<Metadata> {
        let metadata = self.metadata()?;
        for (label, blob) in &requirements.homes {
            if !metadata.settings.contains_key(*label) {
                bail!(
                    "image `{}` has no rig home `{label}` (it has: {}); rebuild it with \
                     `integrations/rig-eval/container/build.sh --homes \"{}\"`",
                    self.reference,
                    if metadata.settings.is_empty() {
                        "none".to_owned()
                    } else {
                        metadata
                            .settings
                            .keys()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    },
                    label
                );
            }
            match metadata.settings.get(*label) {
                Some(built) if built == blob => {}
                Some(_) => bail!(
                    "image `{}` built `{label}` from different settings; rebuild it, or the arm \
                     is not the one the report will name",
                    self.reference
                ),
                None => unreachable!("the label was checked above"),
            }
        }

        let check = self.output(&["/bin/sh", "-c", CHECK])?;
        let check = String::from_utf8_lossy(&check);
        let missing: Vec<&str> = check
            .lines()
            .filter_map(|line| line.strip_prefix("missing:"))
            .collect();
        for (needed, program) in [
            (requirements.rig, "rig"),
            (requirements.opencode, "bunx"),
            (requirements.pi, "pi"),
            (requirements.toolkit, "at-peek"),
            (true, "cargo"),
        ] {
            if needed && missing.contains(&program) {
                bail!(
                    "image `{}` has no `{program}`; rebuild it with \
                     `integrations/rig-eval/container/build.sh`",
                    self.reference
                );
            }
        }
        if check.lines().any(|line| line == "corpus:present") {
            bail!(
                "image `{}` contains integrations/rig-eval/scenarios; the gold must not be in the \
                 image. Rebuild it with the build script, which removes the corpus",
                self.reference
            );
        }
        Ok(metadata)
    }

    fn metadata(&self) -> Result<Metadata> {
        let text = self.output(&["cat", METADATA]).with_context(|| {
            format!(
                "reading {METADATA} from `{}`; an older image has no container metadata — rebuild \
                 it with integrations/rig-eval/container/build.sh",
                self.reference
            )
        })?;
        serde_json::from_slice(&text)
            .with_context(|| format!("parsing {METADATA} in `{}`", self.reference))
    }

    /// Run a program in a throwaway container and return its stdout. The CLI's `[n/6]` progress
    /// and its SIGWINCH warning go to stderr and are deliberately dropped.
    fn output(&self, args: &[&str]) -> Result<Vec<u8>> {
        let output = Command::new("container")
            .arg("run")
            .arg("--rm")
            .arg(&self.reference)
            .args(args)
            .output()
            .with_context(|| format!("running `container run --rm {}`", self.reference))?;
        if !output.status.success() {
            bail!(
                "`container run --rm {} {}`: {}",
                self.reference,
                args.join(" "),
                stderr(&output.stderr)
            );
        }
        Ok(output.stdout)
    }
}

/// The single probe preflight runs inside the image. Fixed text: no arm's data reaches it.
const CHECK: &str = "\
for b in rig pi bunx cargo; do command -v \"$b\" >/dev/null 2>&1 || echo \"missing:$b\"; done\n\
[ -x /opt/playbook/bin/at-peek ] || echo missing:at-peek\n\
[ -d /workspace/integrations/rig-eval/scenarios ] && echo corpus:present\n\
exit 0\n";

impl Trial {
    /// The rig home this trial's arm runs against, inside the image. It mirrors the host layout's
    /// depth, so a `settings.toml` that names `../../integrations/rig` resolves to the image's
    /// checkout rather than to a rewritten path.
    pub fn rig_home(&self, label: &str) -> PathBuf {
        PathBuf::from(home_dir(label))
    }

    /// The host directory whose `sessions` child is the mount: where the effect log appears.
    pub fn effects_root(&self) -> Option<&Path> {
        self.sessions.as_deref().and_then(Path::parent)
    }

    /// The command that runs `invocation` in this trial's container.
    pub fn command(&self, name: &str, invocation: &Invocation, env_file: &Path) -> Result<Command> {
        let program = self.program(&invocation.program)?;
        let workdir = self.cwd(&invocation.cwd)?;
        let mut command = Command::new("container");
        command
            .arg("run")
            .arg("--rm")
            .arg("--name")
            .arg(name)
            .arg("--cpus")
            .arg(self.image.cpus.to_string())
            .arg("--memory")
            .arg(&self.image.memory)
            .arg("--workdir")
            .arg(&workdir)
            .arg("--volume")
            .arg(format!("{}:{WORKSPACE}", self.workspace.display()))
            .arg("--env-file")
            .arg(env_file)
            // The CLI's own working directory: the mount source, which exists on the host.
            .current_dir(&self.workspace);
        if let Some(home) = &self.home {
            let sessions = self.sessions.as_ref().with_context(|| {
                format!("the `{home}` arm has no host directory for its sessions")
            })?;
            command.arg("--volume").arg(format!(
                "{}:{}/sessions",
                sessions.display(),
                self.rig_home(home).display()
            ));
        }
        if let Some(config) = &self.opencode_config {
            command
                .arg("--volume")
                .arg(format!("{}:{OPENCODE_CONFIG}:ro", config.display()));
        }
        command
            .arg(&self.image.reference)
            .arg(program)
            .args(&invocation.args);
        Ok(command)
    }

    /// A host workdir as the path the container sees.
    fn cwd(&self, host: &Path) -> Result<String> {
        let relative = host.strip_prefix(&self.workspace).with_context(|| {
            format!(
                "{} is not inside the trial snapshot {}",
                host.display(),
                self.workspace.display()
            )
        })?;
        Ok(Path::new(WORKSPACE).join(relative).display().to_string())
    }

    /// A launcher the host names as the path the image installs it at. The set is closed: a new
    /// driver has to say where its binary lives in the image.
    fn program(&self, host: &Path) -> Result<String> {
        match host.file_name().and_then(OsStr::to_str) {
            Some("rig") => Ok(RIG_BIN.to_owned()),
            Some("pi") => Ok(PI_BIN.to_owned()),
            Some("bunx") => Ok(BUNX_BIN.to_owned()),
            Some(other) => bail!(
                "container mode has no path for `{other}`; known launchers are rig, pi and bunx"
            ),
            None => bail!(
                "{} has no file name to resolve in the image",
                host.display()
            ),
        }
    }
}

/// `container image inspect`'s shape: the digest is on the descriptor, with `id` as the fallback
/// for an image built straight into the local store.
#[derive(Debug, Deserialize)]
struct Inspected {
    id: String,
    configuration: Configuration,
}

#[derive(Debug, Deserialize)]
struct Configuration {
    descriptor: Descriptor,
}

#[derive(Debug, Deserialize)]
struct Descriptor {
    #[serde(default)]
    digest: String,
}

fn stderr(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("failed to send signal"))
        .collect();
    if lines.is_empty() {
        "no output".to_owned()
    } else {
        lines.join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trial() -> Trial {
        Trial {
            image: Image {
                reference: "playbook-eval/rig-eval:test".to_owned(),
                digest: "sha256:test".to_owned(),
                cpus: 4,
                memory: "8G".to_owned(),
            },
            workspace: PathBuf::from("/tmp/rig-eval-snapshots-1/worker-0"),
            home: Some("gated".to_owned()),
            sessions: Some(PathBuf::from(
                "/tmp/out/homes/trace-redirect-rig-gated-r1/sessions",
            )),
            opencode_config: None,
        }
    }

    fn invocation(program: &str, cwd: &str) -> Invocation {
        Invocation {
            program: PathBuf::from(program),
            args: vec!["-p".to_owned(), "a prompt".to_owned()],
            env: Vec::new(),
            cwd: PathBuf::from(cwd),
        }
    }

    fn args(command: &Command) -> Vec<String> {
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn a_trial_mounts_the_snapshot_and_its_sessions() {
        let args = args(
            &trial()
                .command(
                    "rig-eval-1",
                    &invocation(
                        "/repo/.tmp/rig-fork/bin/rig",
                        "/tmp/rig-eval-snapshots-1/worker-0",
                    ),
                    Path::new("/tmp/rig-eval-1/env"),
                )
                .expect("a command"),
        );
        assert!(args.contains(&"run".to_owned()));
        assert!(args.contains(&"--rm".to_owned()));
        assert!(args.contains(&"--name".to_owned()));
        assert!(args.contains(&"rig-eval-1".to_owned()));
        assert!(args.contains(&"/tmp/rig-eval-snapshots-1/worker-0:/repo".to_owned()));
        assert!(args.contains(&"/tmp/rig-eval-1/env".to_owned()));
        assert!(args.contains(&"--volume".to_owned()));
        assert!(args.contains(
            &"/tmp/out/homes/trace-redirect-rig-gated-r1/sessions:/workspace/.tmp/rig-home/sessions"
                .to_owned()
        ));
        assert!(args.contains(&"/usr/local/bin/rig".to_owned()));
    }

    #[test]
    fn the_workdir_is_inside_the_mount() {
        let args = args(
            &trial()
                .command(
                    "rig-eval-1",
                    &invocation(
                        "/rig",
                        "/tmp/rig-eval-snapshots-1/worker-0/integrations/rig",
                    ),
                    Path::new("/tmp/env"),
                )
                .expect("a command"),
        );
        let index = args
            .iter()
            .position(|arg| arg == "--workdir")
            .expect("a workdir");
        assert_eq!(args[index + 1], "/repo/integrations/rig");
    }

    #[test]
    fn a_workdir_outside_the_snapshot_is_refused() {
        let error = trial()
            .command(
                "rig-eval-1",
                &invocation("/rig", "/somewhere/else"),
                Path::new("/tmp/env"),
            )
            .expect_err("a workdir outside the mount");
        assert!(error.to_string().contains("not inside the trial snapshot"));
    }

    #[test]
    fn the_known_launchers_map_to_their_image_paths() {
        let trial = trial();
        for (host, image) in [
            ("/repo/.tmp/rig-fork/bin/rig", RIG_BIN),
            ("/repo/.tmp/pi/bin/pi", PI_BIN),
            ("bunx", BUNX_BIN),
        ] {
            let command = trial
                .command(
                    "rig-eval-1",
                    &invocation(host, "/tmp/rig-eval-snapshots-1/worker-0"),
                    Path::new("/tmp/env"),
                )
                .expect("a command");
            let args = args(&command);
            assert!(args.contains(&image.to_owned()), "{host} -> {image}");
        }
    }

    #[test]
    fn an_unknown_launcher_is_refused() {
        let error = trial()
            .command(
                "rig-eval-1",
                &invocation("/usr/local/bin/other", "/tmp/rig-eval-snapshots-1/worker-0"),
                Path::new("/tmp/env"),
            )
            .expect_err("an unknown launcher");
        assert!(error.to_string().contains("no path for `other`"));
    }

    #[test]
    fn the_path_names_the_toolkit_only_when_the_arm_has_it() {
        assert!(path(true).starts_with(TOOLKIT_BIN));
        assert!(!path(false).contains(TOOLKIT_BIN));
    }

    #[test]
    fn a_home_keeps_the_host_layouts_depth() {
        assert_eq!(home_dir("gated"), "/workspace/.tmp/rig-home");
        assert_eq!(home_dir("baseline"), "/workspace/.tmp/rig-home-baseline");
    }

    #[test]
    fn an_opencode_config_is_mounted_read_only() {
        let mut trial = trial();
        trial.home = None;
        trial.sessions = None;
        trial.opencode_config = Some(PathBuf::from("/tmp/opencode.json"));
        let args = args(
            &trial
                .command(
                    "rig-eval-1",
                    &invocation("bunx", "/tmp/rig-eval-snapshots-1/worker-0"),
                    Path::new("/tmp/env"),
                )
                .expect("a command"),
        );
        assert!(args.contains(&"/tmp/opencode.json:/opt/opencode/opencode.json:ro".to_owned()));
    }
}
