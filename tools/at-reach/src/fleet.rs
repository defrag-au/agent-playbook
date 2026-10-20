//! The private fleet tree: machine profiles, root resolution and loading.
//!
//! A profile is flat `key: value` text. It is deliberately the same shape as a rule's
//! frontmatter so an agent already knows how to read one, and so this tool needs no parser
//! dependency. The tree is data, not configuration for this repository: it lives elsewhere and
//! is named explicitly by `--fleet-root`, `AGENT_FLEET_ROOT`, or one of the two conventions.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use at_core::contract::Fail;

/// One machine profile.
#[derive(Debug, Clone)]
pub struct Profile {
    pub id: String,
    pub role: String,
    pub os: String,
    pub arch: String,
    pub nix_system: Option<String>,
    pub ssh: String,
    pub tailscale: Option<String>,
    pub tailscale_ip: Option<String>,
    pub tailscale_tag: Option<String>,
    pub shell: Option<String>,
    pub sudo: String,
    pub repo_root: Option<PathBuf>,
    pub repos: Vec<String>,
    pub rebuild_repo: Option<String>,
    pub rebuild: String,
    pub writes: String,
    pub notes: Option<String>,
}

impl Profile {
    fn load(path: &Path) -> Result<Profile, Fail> {
        let raw = fs::read_to_string(path).map_err(|e| {
            Fail::environment(format!("cannot read profile {}: {e}", path.display()))
        })?;
        Profile::parse(path, &raw)
    }

    fn parse(path: &Path, raw: &str) -> Result<Profile, Fail> {
        let mut values: BTreeMap<String, String> = BTreeMap::new();

        for (index, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                return Err(Fail::environment(format!(
                    "{}:{}: expected `key: value`",
                    path.display(),
                    index + 1
                )));
            };
            let key = key.trim();
            let value = value.trim();
            if key.is_empty() {
                return Err(Fail::environment(format!(
                    "{}:{}: empty key",
                    path.display(),
                    index + 1
                )));
            }
            if values.insert(key.to_string(), value.to_string()).is_some() {
                return Err(Fail::environment(format!(
                    "{}:{}: duplicate key `{key}`",
                    path.display(),
                    index + 1
                )));
            }
        }

        let required = |key: &str| -> Result<String, Fail> {
            values.get(key).cloned().ok_or_else(|| {
                Fail::environment(format!("{}: missing required key `{key}`", path.display()))
            })
        };

        let repos = values
            .get("repos")
            .map(|list| {
                list.split(',')
                    .map(str::trim)
                    .filter(|repo| !repo.is_empty())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();

        Ok(Profile {
            id: required("id")?,
            role: required("role")?,
            os: required("os")?,
            arch: required("arch")?,
            nix_system: values.get("nix_system").cloned(),
            ssh: required("ssh")?,
            tailscale: values.get("tailscale").cloned(),
            tailscale_ip: values.get("tailscale_ip").cloned(),
            tailscale_tag: values.get("tailscale_tag").cloned(),
            shell: values.get("shell").cloned(),
            sudo: required("sudo")?,
            repo_root: values.get("repo_root").map(PathBuf::from),
            repos,
            rebuild_repo: values.get("rebuild_repo").cloned(),
            rebuild: required("rebuild")?,
            writes: required("writes")?,
            notes: values.get("notes").cloned(),
        })
    }

    /// The repo paths this profile names, resolved against `repo_root` when they are relative.
    pub fn repo_paths(&self) -> Result<Vec<PathBuf>, Fail> {
        let mut paths = Vec::with_capacity(self.repos.len());
        for repo in &self.repos {
            let candidate = Path::new(repo);
            if candidate.is_absolute() {
                paths.push(candidate.to_path_buf());
            } else if let Some(root) = &self.repo_root {
                paths.push(root.join(candidate));
            } else {
                return Err(Fail::environment(format!(
                    "{}: repo `{repo}` is relative but `repo_root` is missing",
                    self.id
                )));
            }
        }
        Ok(paths)
    }
}

/// A loaded fleet tree.
pub struct Fleet {
    root: PathBuf,
    source: &'static str,
    profiles: Vec<Profile>,
}

impl Fleet {
    /// Resolve the tree in the documented order, then load it.
    pub fn resolve(explicit: Option<&Path>) -> Result<Fleet, Fail> {
        if let Some(path) = explicit {
            return Fleet::at(path, "--fleet-root");
        }

        match std::env::var("AGENT_FLEET_ROOT") {
            Ok(value) if !value.trim().is_empty() => {
                return Fleet::at(Path::new(&value), "AGENT_FLEET_ROOT");
            }
            Ok(_) => {
                return Err(Fail::environment(
                    "AGENT_FLEET_ROOT is set but empty · unset it or name a fleet root",
                ));
            }
            Err(_) => {}
        }

        if let Ok(cwd) = std::env::current_dir() {
            if cwd.join("machines").is_dir() {
                return Fleet::at(&cwd, "current directory");
            }
            let candidate = cwd.join("fleet");
            if candidate.join("machines").is_dir() {
                return Fleet::at(&candidate, "./fleet");
            }
        }

        if let Ok(home) = std::env::var("HOME") {
            let candidate = Path::new(&home).join(".config/agent-playbook/fleet");
            if candidate.join("machines").is_dir() {
                return Fleet::at(&candidate, "~/.config/agent-playbook/fleet");
            }
        }

        Err(Fail::environment(
            "no fleet root · pass --fleet-root <path>, set AGENT_FLEET_ROOT, or run from a \
             repository with ./fleet",
        ))
    }

    /// Load a fleet tree at a known path. The root must contain `machines/`.
    pub fn at(root: &Path, source: &'static str) -> Result<Fleet, Fail> {
        let root = fs::canonicalize(root).map_err(|e| {
            Fail::environment(format!("cannot resolve fleet root {}: {e}", root.display()))
        })?;
        if !root.is_dir() {
            return Err(Fail::environment(format!(
                "fleet root {} is not a directory",
                root.display()
            )));
        }
        let machines = root.join("machines");
        if !machines.is_dir() {
            return Err(Fail::environment(format!(
                "fleet root {} has no machines/ directory",
                root.display()
            )));
        }

        let entries = fs::read_dir(&machines)
            .map_err(|e| Fail::environment(format!("cannot read {}: {e}", machines.display())))?;

        let mut paths = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| {
                Fail::environment(format!("cannot read {}: {e}", machines.display()))
            })?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "md") {
                paths.push(path);
            }
        }
        paths.sort();

        let mut profiles = Vec::with_capacity(paths.len());
        let mut seen: BTreeMap<String, PathBuf> = BTreeMap::new();
        for path in paths {
            let profile = Profile::load(&path)?;
            if let Some(previous) = seen.insert(profile.id.clone(), path.clone()) {
                return Err(Fail::environment(format!(
                    "duplicate id `{}` in {} and {}",
                    profile.id,
                    previous.display(),
                    path.display()
                )));
            }
            profiles.push(profile);
        }
        profiles.sort_by(|a, b| a.id.cmp(&b.id));

        Ok(Fleet {
            root,
            source,
            profiles,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn source(&self) -> &'static str {
        self.source
    }

    pub fn profiles(&self) -> &[Profile] {
        &self.profiles
    }

    pub fn find(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|profile| profile.id == id)
    }

    pub fn ids(&self) -> String {
        self.profiles
            .iter()
            .map(|profile| profile.id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    const PROFILE: &str = "\
id: example-host
role: client
os: linux
arch: x86_64
nix_system: x86_64-linux
ssh: example-host
tailscale: example-host
tailscale_ip: 100.64.0.10
tailscale_tag: tag:client
shell: /bin/zsh
sudo: required
repo_root: /home/user/code
repos: example-org/nix-config,example-org/shared-crates
rebuild_repo: example-org/nix-config
rebuild: home-manager switch --flake .#user@example-host
writes: none
notes: Example only.
";

    fn profile(raw: &str) -> Profile {
        Profile::parse(Path::new("/fleet/machines/example.md"), raw).expect("a valid profile")
    }

    #[test]
    fn a_profile_parses_into_its_fields() {
        let parsed = profile(PROFILE);
        assert_eq!(parsed.id, "example-host");
        assert_eq!(parsed.role, "client");
        assert_eq!(
            parsed.repos,
            ["example-org/nix-config", "example-org/shared-crates"]
        );
        assert_eq!(parsed.sudo, "required");
    }

    #[test]
    fn repo_paths_are_relative_to_the_repo_root() {
        let parsed = profile(PROFILE);
        let paths = parsed.repo_paths().expect("repo paths");
        assert_eq!(
            paths,
            [
                PathBuf::from("/home/user/code/example-org/nix-config"),
                PathBuf::from("/home/user/code/example-org/shared-crates"),
            ]
        );
    }

    #[test]
    fn an_absolute_repo_stays_absolute() {
        let raw = PROFILE.replace(
            "repos: example-org/nix-config,example-org/shared-crates",
            "repos: /srv/repos/compositor",
        );
        let parsed = profile(&raw);
        assert_eq!(
            parsed.repo_paths().expect("repo paths"),
            [PathBuf::from("/srv/repos/compositor")]
        );
    }

    #[test]
    fn a_duplicate_key_is_an_error() {
        let raw = format!("{PROFILE}role: admin\n");
        let error = Profile::parse(Path::new("/fleet/machines/example.md"), &raw)
            .expect_err("duplicate key");
        assert!(error.message.contains("duplicate key `role`"));
    }

    #[test]
    fn a_missing_required_key_is_an_error() {
        let raw = PROFILE.replace("ssh: example-host\n", "");
        let error =
            Profile::parse(Path::new("/fleet/machines/example.md"), &raw).expect_err("missing ssh");
        assert!(error.message.contains("missing required key `ssh`"));
    }

    #[test]
    fn a_relative_repo_without_a_root_is_an_error() {
        let raw = PROFILE.replace("repo_root: /home/user/code\n", "");
        let parsed = profile(&raw);
        let error = parsed.repo_paths().expect_err("missing repo_root");
        assert!(error.message.contains("repo_root"));
    }

    #[test]
    fn the_public_example_fleet_loads() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/fleet");
        if !root.exists() {
            return;
        }
        let fleet = Fleet::at(&root, "examples/fleet").expect("the example fleet");
        assert_eq!(fleet.profiles().len(), 1);
        assert!(fleet.find("example-host").is_some());
    }

    #[test]
    fn a_fleet_loads_profiles_in_id_order() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("at-reach-{}-{stamp}", std::process::id()));
        let machines = root.join("machines");
        fs::create_dir_all(&machines).expect("machines dir");
        fs::write(
            machines.join("b.md"),
            PROFILE.replace("example-host", "b-host"),
        )
        .expect("profile b");
        fs::write(
            machines.join("a.md"),
            PROFILE.replace("example-host", "a-host"),
        )
        .expect("profile a");

        let fleet = Fleet::at(&root, "--fleet-root").expect("a fleet");
        assert_eq!(fleet.ids(), "a-host, b-host");
        assert_eq!(fleet.source(), "--fleet-root");
        fs::remove_dir_all(&root).expect("cleanup");
    }
}
