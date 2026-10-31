//! Where the Koios key comes from, and how the answer describes it without printing it.
//!
//! The key is a secret, so it is read once into memory and never rendered. The source is shown
//! as a sentence — `KOIOS_API_KEY`, `~/.config/koios/api_key`, or keyless — and a key file
//! other accounts can read is reported as a caveat rather than silently accepted.

use std::path::{Path, PathBuf};

use crate::contract::Fail;

pub struct Auth {
    key: Option<String>,
    source: Source,
    caveat: Option<String>,
}

enum Source {
    Env,
    File(PathBuf),
    Missing(PathBuf),
    Ignored,
}

impl Auth {
    /// The bearer token, if one was configured. Never rendered by any caller.
    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    pub fn describe(&self) -> String {
        match &self.source {
            Source::Env => "key from KOIOS_API_KEY".to_string(),
            Source::File(path) => format!("key from {}", display_path(path)),
            Source::Missing(path) => format!("keyless (no {})", display_path(path)),
            Source::Ignored => "keyless (--keyless)".to_string(),
        }
    }

    pub fn caveat(&self) -> Option<&str> {
        self.caveat.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn for_test(key: Option<&str>, caveat: Option<&str>) -> Self {
        Self {
            key: key.map(str::to_string),
            source: Source::Env,
            caveat: caveat.map(str::to_string),
        }
    }
}

/// Read `KOIOS_API_KEY`, then `$XDG_CONFIG_HOME/koios/api_key` (or
/// `~/.config/koios/api_key`), then fall back to the free tier.
pub fn load(keyless: bool) -> Result<Auth, Fail> {
    if keyless {
        return Ok(Auth {
            key: None,
            source: Source::Ignored,
            caveat: None,
        });
    }

    if let Ok(value) = std::env::var("KOIOS_API_KEY") {
        let value = value.trim();
        if !value.is_empty() {
            return Ok(Auth {
                key: Some(value.to_string()),
                source: Source::Env,
                caveat: None,
            });
        }
    }

    let Some(path) = config_path() else {
        return Ok(Auth {
            key: None,
            source: Source::Ignored,
            caveat: Some(
                "HOME and XDG_CONFIG_HOME are both unset; running on the keyless tier".to_string(),
            ),
        });
    };

    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let key = text.trim();
            if key.is_empty() {
                return Ok(Auth {
                    key: None,
                    source: Source::Missing(path),
                    caveat: Some("the key file is empty; running on the keyless tier".to_string()),
                });
            }
            Ok(Auth {
                key: Some(key.to_string()),
                source: Source::File(path.clone()),
                caveat: permission_caveat(&path),
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Auth {
            key: None,
            source: Source::Missing(path),
            caveat: None,
        }),
        Err(error) => Err(Fail::environment(format!(
            "cannot read {}: {error} · fix the file or run with --keyless",
            display_path(&path)
        ))),
    }
}

pub fn config_path() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(dir).join("koios/api_key"));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config/koios/api_key"))
}

fn display_path(path: &Path) -> String {
    if let Some(home) = std::env::var_os("HOME") {
        if let Ok(rest) = path.strip_prefix(&home) {
            return format!("~/{}", rest.display());
        }
    }
    path.display().to_string()
}

#[cfg(unix)]
fn permission_caveat(path: &Path) -> Option<String> {
    use std::os::unix::fs::PermissionsExt;

    let mode = std::fs::metadata(path).ok()?.permissions().mode() & 0o777;
    if mode & 0o077 == 0 {
        return None;
    }
    Some(format!(
        "key file {} is mode {mode:03o}; chmod 600 it so other accounts cannot read it",
        display_path(path)
    ))
}

#[cfg(not(unix))]
fn permission_caveat(_path: &Path) -> Option<String> {
    None
}
