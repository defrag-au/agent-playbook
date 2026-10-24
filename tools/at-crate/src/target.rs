//! Crate targets: `<crate>[/<path>][:<range>]`, and containment inside the resolved crate.
//!
//! The first path component is the crate name. Everything after it is relative to that crate's
//! root, so `--version` has one place to apply and an absolute path has no way to sneak into the
//! second component. The range syntax is `at-core`'s, shared with `at-peek`: 1-based, inclusive,
//! `40+20` for twenty lines from 40.

use std::fs;
use std::path::{Path, PathBuf};

use at_core::contract::Fail;
use at_core::paths::{self, Range, Target};

#[derive(Clone, Debug)]
pub struct CrateTarget {
    pub crate_name: String,
    pub inner: String,
    pub range: Option<Range>,
    /// Exactly as the caller typed it, so a continuation can print the original spelling.
    pub arg: String,
}

impl CrateTarget {
    pub fn parse(arg: &str) -> Result<CrateTarget, Fail> {
        let parsed: Target = paths::parse_target(arg)?;
        let (crate_name, inner) = match parsed.path.split_once('/') {
            Some((name, rest)) => (name, rest),
            None => (parsed.path.as_str(), ""),
        };

        if crate_name.is_empty() {
            return Err(Fail::usage(format!(
                "`{arg}` has no crate name · the target is <crate>[/<path>][:<range>]"
            )));
        }
        if crate_name.contains('@') {
            return Err(Fail::usage(format!(
                "`{crate_name}` names a version with `@`, which is not this grammar · pass \
                 --version <ver> and name the crate plainly"
            )));
        }
        if inner.is_empty() && parsed.range.is_some() {
            return Err(Fail::usage(format!(
                "`{arg}` asks for a line range with no file · add the path inside the crate, e.g. \
                 {crate_name}/src/lib.rs:40-60"
            )));
        }

        Ok(CrateTarget {
            crate_name: crate_name.to_string(),
            inner: inner.to_string(),
            range: parsed.range,
            arg: arg.to_string(),
        })
    }

    /// `crate/path:range`, for headers and exits.
    pub fn describe(&self) -> String {
        let path = self.path();
        match self.range {
            Some(range) => format!("{path}:{}", range.spec()),
            None => path,
        }
    }

    pub fn path(&self) -> String {
        if self.inner.is_empty() {
            self.crate_name.clone()
        } else {
            format!("{}/{}", self.crate_name, self.inner)
        }
    }

    /// The same target with a different range, for a continuation.
    pub fn next_range(&self, range: Range) -> String {
        format!("{}:{}", self.path(), range.spec())
    }
}

/// Resolve `inner` inside `root`, refusing anything that leaves it. Canonicalising first closes
/// `..` and a symlink that points outside, which a lexical check would miss.
pub fn resolve_in(root: &Path, inner: &str, crate_name: &str) -> Result<PathBuf, Fail> {
    let candidate = if inner.is_empty() {
        root.to_path_buf()
    } else {
        root.join(inner)
    };
    let resolved = fs::canonicalize(&candidate).map_err(|error| {
        Fail::environment(format!("cannot read {inner} in {crate_name}: {error}"))
    })?;
    if !resolved.starts_with(root) {
        return Err(Fail::refused(format!(
            "{inner} resolves outside {crate_name} ({})",
            root.display()
        )));
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_crate_is_a_target() {
        let target = CrateTarget::parse("headless_chrome").expect("a target");
        assert_eq!(target.crate_name, "headless_chrome");
        assert_eq!(target.inner, "");
        assert!(target.range.is_none());
    }

    #[test]
    fn a_path_and_range_split_at_the_last_colon() {
        let target =
            CrateTarget::parse("headless_chrome/src/browser/mod.rs:40-60").expect("a target");
        assert_eq!(target.crate_name, "headless_chrome");
        assert_eq!(target.inner, "src/browser/mod.rs");
        assert_eq!(target.range, Some(Range::Span(40, 60)));
    }

    #[test]
    fn a_range_without_a_file_is_a_usage_error() {
        let error = CrateTarget::parse("headless_chrome:40-60").expect_err("no file");
        assert!(error.message.contains("no file"));
    }

    #[test]
    fn a_version_sigil_says_where_the_version_goes() {
        let error = CrateTarget::parse("headless_chrome@1.0.22/src/lib.rs").expect_err("sigil");
        assert!(error.message.contains("--version"));
    }
}
