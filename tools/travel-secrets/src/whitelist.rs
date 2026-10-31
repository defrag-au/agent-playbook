//! The whitelist: a flat `key: value` file whose `pass:` entry names directories or individual
//! entries that are allowed on the road.
//!
//! It is deliberately machine-independent: no host, no recipient, no output path. The same file
//! can be used for any travel device and any destination key.

use std::fs;
use std::path::Path;

use at_core::contract::Fail;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Whitelist {
    pub entries: Vec<String>,
}

impl Whitelist {
    pub fn read(path: &Path) -> Result<Whitelist, Fail> {
        let raw = fs::read_to_string(path).map_err(|error| {
            Fail::environment(format!("cannot read {}: {error}", path.display()))
        })?;
        Whitelist::parse(path, &raw)
    }

    fn parse(path: &Path, raw: &str) -> Result<Whitelist, Fail> {
        let mut pass: Option<String> = None;

        for (index, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                return Err(Fail::usage(format!(
                    "{}:{}: expected `key: value`",
                    path.display(),
                    index + 1
                )));
            };
            if key.trim() != "pass" {
                continue;
            }
            if pass.is_some() {
                return Err(Fail::usage(format!(
                    "{}:{}: duplicate `pass:` key",
                    path.display(),
                    index + 1
                )));
            }
            pass = Some(value.trim().to_string());
        }

        let raw = pass.ok_or_else(|| {
            Fail::usage(format!(
                "{}: missing `pass:` whitelist · expected `pass: area,area/entry`",
                path.display()
            ))
        })?;

        let mut entries = Vec::new();
        for part in raw.split(',') {
            let part = part.trim();
            if part.is_empty() {
                return Err(Fail::usage(format!(
                    "{}: empty entry in the `pass:` whitelist",
                    path.display()
                )));
            }
            entries.push(part.to_string());
        }

        if entries.is_empty() {
            return Err(Fail::usage(format!(
                "{}: the `pass:` whitelist is empty",
                path.display()
            )));
        }

        Ok(Whitelist { entries })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whitelist_splits_entries_on_commas() {
        let parsed = Whitelist::parse(
            Path::new("/fleet/travel/credentials.md"),
            "pass: github, cloudflare/api ,travel\n",
        )
        .expect("whitelist");
        assert_eq!(parsed.entries, ["github", "cloudflare/api", "travel"]);
    }

    #[test]
    fn comments_and_other_keys_are_ignored() {
        let parsed = Whitelist::parse(
            Path::new("/fleet/travel/credentials.md"),
            "# comment\nnotes: travel\npass: github\n",
        )
        .expect("whitelist");
        assert_eq!(parsed.entries, ["github"]);
    }

    #[test]
    fn a_missing_pass_key_is_an_error() {
        let error = Whitelist::parse(Path::new("/fleet/travel/credentials.md"), "notes: only\n")
            .expect_err("missing");
        assert!(error.message.contains("missing `pass:`"));
    }

    #[test]
    fn a_duplicate_pass_key_is_an_error() {
        let error = Whitelist::parse(
            Path::new("/fleet/travel/credentials.md"),
            "pass: github\npass: travel\n",
        )
        .expect_err("duplicate");
        assert!(error.message.contains("duplicate `pass:`"));
    }

    #[test]
    fn an_empty_entry_is_an_error() {
        let error = Whitelist::parse(
            Path::new("/fleet/travel/credentials.md"),
            "pass: github,,travel\n",
        )
        .expect_err("empty");
        assert!(error.message.contains("empty entry"));
    }

    #[test]
    fn an_empty_whitelist_is_an_error() {
        let error = Whitelist::parse(Path::new("/fleet/travel/credentials.md"), "pass:\n")
            .expect_err("empty");
        assert!(error.message.contains("empty"));
    }
}
