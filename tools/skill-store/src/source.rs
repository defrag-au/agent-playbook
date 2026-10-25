//! A source reference, in the shape of a Nix flake reference.
//!
//! ```text
//! <store>#<skill>[@v<N>]
//!
//! github:defrag-au/container-compose#container-compose@v1
//! github:defrag-au/container-compose?rev=<sha>&dir=.skill-store#container-compose@v1
//! git+https://example.org/skills.git?rev=<sha>#widget-screenshot@v2
//! path:~/code/defrag/skill-store#widget-screenshot@v2
//! ```
//!
//! The `#` fragment is the selector, as it is for a flake's output attribute. `@vN` is ours and
//! names an OCFL version directory exactly; omitting it selects the object's head, which the
//! consumer's lock then freezes.
//!
//! A store may be the repository root or a `.skill-store/` directory inside it. The latter is the
//! default when the root has no OCFL declaration of its own, so a tool repository can carry its
//! skill beside its code without a query parameter.

use std::path::PathBuf;

use crate::fsx::{expand_tilde, safe_relative};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Locator {
    Path(PathBuf),
    Git {
        url: String,
        ref_: Option<String>,
        rev: Option<String>,
        dir: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub raw: String,
    /// The locator as written, with the fragment removed and `github://` normalised.
    pub locator_raw: String,
    pub locator: Locator,
    pub skill: Option<String>,
    pub version: Option<String>,
}

impl Source {
    /// The store as the consumer wrote it, for an export manifest or a lock.
    pub fn store_label(&self) -> String {
        self.locator_raw.clone()
    }
}

pub fn parse(raw: &str) -> Result<Source, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("empty source".to_string());
    }

    let (locator_part, fragment) = match raw.split_once('#') {
        Some((locator, fragment)) => (locator, Some(fragment)),
        None => (raw, None),
    };

    let (skill, version) = match fragment {
        None => (None, None),
        Some(fragment) => {
            if fragment.is_empty() {
                return Err(format!("`{raw}`: the `#` fragment is empty"));
            }
            let (skill, version) = match fragment.split_once('@') {
                Some((skill, version)) => (skill, Some(version)),
                None => (fragment, None),
            };
            validate_skill_name(skill)?;
            if let Some(version) = version {
                validate_version(version)?;
            }
            (Some(skill.to_string()), version.map(ToString::to_string))
        }
    };

    // `github://` is what muscle memory reaches for; a flake ref is `github:`.
    let locator_part = locator_part
        .strip_prefix("github://")
        .map(|rest| format!("github:{rest}"))
        .or_else(|| {
            locator_part
                .strip_prefix("gitlab://")
                .map(|rest| format!("gitlab:{rest}"))
        })
        .unwrap_or_else(|| locator_part.to_string());

    let locator = if let Some(rest) = locator_part.strip_prefix("path:") {
        if rest.is_empty() {
            return Err(format!("`{raw}`: `path:` needs a directory"));
        }
        Locator::Path(expand_tilde(rest))
    } else if let Some(rest) = locator_part.strip_prefix("github:") {
        hosting(rest, "https://github.com", raw)?
    } else if let Some(rest) = locator_part.strip_prefix("gitlab:") {
        hosting(rest, "https://gitlab.com", raw)?
    } else if let Some(rest) = locator_part.strip_prefix("git+") {
        git_url(rest, raw)?
    } else if locator_part.starts_with("https://") || locator_part.starts_with("http://") {
        return Err(format!(
            "`{raw}`: a bare http(s) URL means a tarball in a flake reference; use \
             `git+https://…` for a git-hosted store"
        ));
    } else {
        return Err(format!(
            "`{raw}`: unknown store. Expected `github:<owner>/<repo>`, `gitlab:<owner>/<repo>`, \
             `git+https://…`, `git+ssh://…`, `git+file://…` or `path:<dir>`"
        ));
    };

    Ok(Source {
        raw: raw.to_string(),
        locator_raw: locator_part,
        locator,
        skill,
        version,
    })
}

fn hosting(rest: &str, base: &str, raw: &str) -> Result<Locator, String> {
    let (path_part, query) = split_query(rest);
    let query = parse_query(query, raw)?;
    let mut parts = path_part.split('/');
    let owner = parts.next().unwrap_or("");
    let repo = parts.next().unwrap_or("");
    if owner.is_empty() || repo.is_empty() {
        return Err(format!(
            "`{raw}`: expected `<owner>/<repo>` after the store type"
        ));
    }
    if owner == "." || owner == ".." || repo == "." || repo == ".." {
        return Err(format!("`{raw}`: `{owner}/{repo}` is not a repository"));
    }
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    let path_ref: Vec<&str> = parts.collect();
    let path_ref = if path_ref.is_empty() {
        None
    } else {
        Some(path_ref.join("/"))
    };
    if path_ref.is_some() && query.ref_.is_some() {
        return Err(format!(
            "`{raw}`: the ref is given twice — once in the path and once as `?ref=`"
        ));
    }
    if query.ref_.is_some() && query.rev.is_some() {
        return Err(format!("`{raw}`: give `?ref=` or `?rev=`, not both"));
    }
    let url = if base.ends_with('/') {
        format!("{base}{owner}/{repo}")
    } else {
        format!("{base}/{owner}/{repo}.git")
    };
    Ok(Locator::Git {
        url,
        ref_: query.ref_.or(path_ref),
        rev: query.rev,
        dir: query.dir,
    })
}

fn git_url(rest: &str, raw: &str) -> Result<Locator, String> {
    let (url, query) = split_query(rest);
    if !(url.starts_with("https://") || url.starts_with("ssh://") || url.starts_with("file://")) {
        return Err(format!(
            "`{raw}`: a `git+` store must be `https://`, `ssh://` or `file://`"
        ));
    }
    let query = parse_query(query, raw)?;
    if query.ref_.is_some() && query.rev.is_some() {
        return Err(format!("`{raw}`: give `?ref=` or `?rev=`, not both"));
    }
    Ok(Locator::Git {
        url: url.to_string(),
        ref_: query.ref_,
        rev: query.rev,
        dir: query.dir,
    })
}

fn split_query(input: &str) -> (&str, Option<&str>) {
    match input.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (input, None),
    }
}

#[derive(Default)]
struct Query {
    ref_: Option<String>,
    rev: Option<String>,
    dir: Option<String>,
}

fn parse_query(query: Option<&str>, raw: &str) -> Result<Query, String> {
    let Some(query) = query else {
        return Ok(Query::default());
    };
    let mut out = Query::default();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let Some((key, value)) = pair.split_once('=') else {
            return Err(format!("`{raw}`: query parameter `{pair}` has no value"));
        };
        if value.is_empty() {
            return Err(format!("`{raw}`: query parameter `{key}` is empty"));
        }
        match key {
            "ref" => out.ref_ = Some(value.to_string()),
            "rev" => out.rev = Some(value.to_string()),
            "dir" => {
                safe_relative(value)?;
                out.dir = Some(value.to_string());
            }
            other => {
                return Err(format!(
                    "`{raw}`: unknown query parameter `{other}` — expected `ref`, `rev` or `dir`"
                ))
            }
        }
    }
    Ok(out)
}

pub(crate) fn validate_skill_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 64 {
        return Err(format!(
            "`{name}`: a skill name is 1–64 characters, lowercase letters, digits and hyphens"
        ));
    }
    if name.starts_with('-') || name.ends_with('-') || name.contains("--") {
        return Err(format!(
            "`{name}`: a skill name must not start or end with a hyphen or contain `--`"
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(format!(
            "`{name}`: a skill name is lowercase letters, digits and hyphens"
        ));
    }
    Ok(())
}

fn validate_version(version: &str) -> Result<(), String> {
    let Some(number) = version.strip_prefix('v') else {
        return Err(format!(
            "`{version}`: a version is an OCFL version directory, e.g. `v2`"
        ));
    };
    if number.is_empty() || number.starts_with('0') || !number.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!(
            "`{version}`: a version is `v` followed by a positive number, e.g. `v2`"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(source: &str) -> Locator {
        parse(source).expect("source").locator
    }

    #[test]
    fn a_github_source_names_a_skill_and_a_version() {
        let source = parse("github:defrag-au/container-compose#container-compose@v1").unwrap();
        assert_eq!(source.skill.as_deref(), Some("container-compose"));
        assert_eq!(source.version.as_deref(), Some("v1"));
        assert_eq!(
            source.locator,
            Locator::Git {
                url: "https://github.com/defrag-au/container-compose.git".into(),
                ref_: None,
                rev: None,
                dir: None,
            }
        );
    }

    #[test]
    fn github_with_two_slashes_is_normalised() {
        assert_eq!(
            git("github://defrag-au/container-compose"),
            git("github:defrag-au/container-compose")
        );
    }

    #[test]
    fn a_rev_and_a_dir_are_carried_on_the_locator() {
        assert_eq!(
            git("github:defrag-au/container-compose?rev=abc123&dir=.skill-store#x@v2"),
            Locator::Git {
                url: "https://github.com/defrag-au/container-compose.git".into(),
                ref_: None,
                rev: Some("abc123".into()),
                dir: Some(".skill-store".into()),
            }
        );
    }

    #[test]
    fn a_git_url_keeps_its_scheme() {
        assert_eq!(
            git("git+https://example.org/skills.git?ref=main#x"),
            Locator::Git {
                url: "https://example.org/skills.git".into(),
                ref_: Some("main".into()),
                rev: None,
                dir: None,
            }
        );
    }

    #[test]
    fn a_bare_https_url_is_rejected_with_the_reason() {
        let error = parse("https://github.com/a/b#x").unwrap_err();
        assert!(error.contains("git+https"), "{error}");
    }

    #[test]
    fn a_version_must_be_a_version_directory() {
        assert!(parse("github:a/b#x@2").is_err());
        assert!(parse("github:a/b#x@v0").is_err());
        assert!(parse("github:a/b#x@v2").is_ok());
    }

    #[test]
    fn an_unknown_query_parameter_is_an_error() {
        let error = parse("github:a/b?branch=main#x").unwrap_err();
        assert!(error.contains("unknown query parameter"), "{error}");
    }

    #[test]
    fn a_path_is_tilde_expanded() {
        if std::env::var_os("HOME").is_none() {
            return;
        }
        assert_eq!(
            git("path:~/code/x#x"),
            Locator::Path(expand_tilde("~/code/x"))
        );
    }

    #[test]
    fn a_store_without_a_fragment_is_parseable_for_list_and_verify() {
        let source = parse("github:a/b").unwrap();
        assert!(source.skill.is_none());
        assert!(source.version.is_none());
    }
}
