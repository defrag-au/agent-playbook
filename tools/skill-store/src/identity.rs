//! The identity a version records when the caller does not name one.
//!
//! OCFL wants a version's `user` block to be present (W007b) and, when it is, to carry an address
//! (W008) that looks like a URI (W009). The only address a store honestly has is the repository it
//! lives in, so it is read from `origin` and normalised — never guessed, and never a personal email
//! address, which a published inventory would then carry forever.
//!
//! A store outside a git checkout has no address to give. Nothing is recorded then, and the
//! validator's W007b is the honest answer rather than a value invented to silence it.

use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub name: String,
    pub address: String,
}

/// The publishing identity for the store at `store`, read from its repository.
pub fn discover(store: &Path) -> Option<Identity> {
    let address = remote_url(store).and_then(|url| normalise(&url))?;
    let name = git(store, &["config", "user.name"])
        .filter(|name| !name.trim().is_empty())
        .or_else(|| {
            std::env::var("USER")
                .ok()
                .filter(|name| !name.trim().is_empty())
        })
        .unwrap_or_else(|| "skill-store".to_string());
    Some(Identity { name, address })
}

/// `origin`'s URL as git has it stored, before normalisation.
fn remote_url(store: &Path) -> Option<String> {
    git(store, &["remote", "get-url", "origin"])
}

fn git(store: &Path, args: &[&str]) -> Option<String> {
    let path = store.to_str()?;
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// Normalise the forms `origin` is written in to an `https://` URL.
///
/// `git@host:owner/repo.git` is ssh's own shorthand and carries no scheme, so it has to be
/// rewritten: OCFL's W009 wants an address that begins with one, and a reader wants a link they can
/// open. Anything else — a local path, a `file://` remote — has no address a reader could use, and
/// returns `None`.
pub fn normalise(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches('/');
    let url = url.strip_suffix(".git").unwrap_or(url);
    if url.is_empty() {
        return None;
    }
    if let Some(rest) = url.strip_prefix("https://") {
        return Some(format!("https://{rest}"));
    }
    if let Some(rest) = url.strip_prefix("http://") {
        return Some(format!("http://{rest}"));
    }
    if let Some(rest) = url.strip_prefix("ssh://") {
        let rest = strip_user(rest);
        return path_parts(rest).map(|(host, path)| format!("https://{host}/{path}"));
    }
    if let Some(rest) = url.strip_prefix("git://") {
        return path_parts(rest).map(|(host, path)| format!("https://{host}/{path}"));
    }

    // `git@github.com:owner/repo` — the colon is the separator, and there is no scheme.
    let (authority, path) = url.split_once(':')?;
    if path.starts_with('/') || path.contains("://") {
        return None;
    }
    let host = strip_user(authority);
    host_and_path(host, path).map(|(host, path)| format!("https://{host}/{path}"))
}

/// `git@host` or `ssh://git@host` → `host`.
fn strip_user(authority: &str) -> &str {
    authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host)
}

fn path_parts(rest: &str) -> Option<(&str, &str)> {
    let (host, path) = rest.split_once('/')?;
    host_and_path(strip_user(host), path)
}

fn host_and_path<'a>(host: &'a str, path: &'a str) -> Option<(&'a str, &'a str)> {
    let path = path.trim_start_matches('/');
    if host.is_empty() || path.is_empty() || host.contains('/') {
        return None;
    }
    Some((host, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scp_form_becomes_an_https_url() {
        assert_eq!(
            normalise("git@github.com:defrag-au/agent-playbook.git").as_deref(),
            Some("https://github.com/defrag-au/agent-playbook")
        );
        assert_eq!(
            normalise("git@gitlab.example.org:team/store.git").as_deref(),
            Some("https://gitlab.example.org/team/store")
        );
    }

    #[test]
    fn a_url_remote_keeps_its_scheme_and_loses_the_suffix() {
        assert_eq!(
            normalise("https://github.com/defrag-au/container-compose.git/").as_deref(),
            Some("https://github.com/defrag-au/container-compose")
        );
        assert_eq!(
            normalise("ssh://git@github.com/defrag-au/fleet.git").as_deref(),
            Some("https://github.com/defrag-au/fleet")
        );
    }

    #[test]
    fn a_remote_with_no_address_a_reader_could_open_is_refused() {
        assert_eq!(normalise("/Users/damo/code/defrag/agent-playbook"), None);
        assert_eq!(normalise("file:///tmp/store"), None);
        assert_eq!(normalise("  "), None);
    }
}
