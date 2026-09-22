//! Roots, targets and containment.
//!
//! One tree per invocation: every path must resolve inside the root, so an allowlisted
//! `at-peek *` cannot be pointed at `/etc`, a sibling repository, or an SSH key. `--root`
//! widens the invocation to a tree the caller names explicitly, which is a visible token in
//! the command rather than a default that grew.
//!
//! Nothing here reads git data. `.git` is stat-ed for, to find the enclosing worktree, and
//! never opened — that boundary is `at-recall`'s, and it is the whole reason the two are
//! separate binaries.

use std::fs;
use std::path::{Path, PathBuf};

use crate::contract::Fail;

pub struct Root {
    dir: PathBuf,
    name: String,
}

impl Root {
    /// The enclosing worktree: the nearest ancestor holding a `.git` entry, else `cwd` — so
    /// the tool works in a tree that is not a repository at all.
    pub fn discover(cwd: &Path) -> Result<Root, Fail> {
        let start = fs::canonicalize(cwd).map_err(|e| {
            Fail::environment(format!(
                "cannot resolve the working directory {}: {e}",
                cwd.display()
            ))
        })?;
        let mut here = start.as_path();
        loop {
            if here.join(".git").exists() {
                return Root::at(here);
            }
            match here.parent() {
                Some(parent) => here = parent,
                None => return Root::at(&start),
            }
        }
    }

    /// An explicit root. Any existing directory will do: `--root` is how a path outside the
    /// enclosing worktree is addressed deliberately.
    pub fn at(dir: &Path) -> Result<Root, Fail> {
        let dir = fs::canonicalize(dir).map_err(|e| {
            Fail::environment(format!("cannot resolve --root {}: {e}", dir.display()))
        })?;
        if !dir.is_dir() {
            return Err(Fail::environment(format!(
                "--root {} is not a directory",
                dir.display()
            )));
        }
        let name = match dir.file_name() {
            Some(name) => name.to_string_lossy().into_owned(),
            None => dir.display().to_string(),
        };
        Ok(Root { dir, name })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Resolve a caller-supplied path inside the root, or refuse.
    ///
    /// Canonicalising before the prefix check is what closes the two obvious escapes: `..`
    /// segments, and a symlink inside the root that points outside it. Both are resolved to
    /// their real location and then compared against the root.
    pub fn resolve(&self, given: &str) -> Result<PathBuf, Fail> {
        if given.is_empty() {
            return Err(Fail::usage("empty path"));
        }
        let candidate = if Path::new(given).is_absolute() {
            PathBuf::from(given)
        } else {
            self.dir.join(given)
        };
        let resolved = fs::canonicalize(&candidate)
            .map_err(|e| Fail::environment(format!("cannot read {given}: {e}")))?;
        if !resolved.starts_with(&self.dir) {
            return Err(Fail::refused(format!(
                "{given} resolves outside the root ({}) · --root <path> addresses another tree \
                 on purpose",
                self.dir.display()
            )));
        }
        Ok(resolved)
    }

    /// The path as a reader wants it: relative to the root, forward slashes.
    pub fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.dir)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }
}

/// A line range, as typed after the colon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    /// `path:40`
    Line(usize),
    /// `path:40-60`, inclusive at both ends.
    Span(usize, usize),
    /// `path:40+20` — twenty lines from 40.
    From(usize, usize),
}

impl Range {
    pub fn start(self) -> usize {
        match self {
            Range::Line(n) => n,
            Range::Span(start, _) => start,
            Range::From(start, _) => start,
        }
    }

    /// How many lines the caller asked for, before the file or the limit cuts it.
    pub fn requested_lines(self) -> usize {
        match self {
            Range::Line(_) => 1,
            Range::Span(start, end) => end - start + 1,
            Range::From(_, count) => count,
        }
    }

    /// The range as it was typed, for the header.
    pub fn spec(self) -> String {
        match self {
            Range::Line(n) => n.to_string(),
            Range::Span(start, end) => format!("{start}-{end}"),
            Range::From(start, count) => format!("{start}+{count}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Target {
    pub path: String,
    pub range: Option<Range>,
}

impl Target {
    /// `path:40-60`, or the bare path when the whole file is meant.
    pub fn describe(&self, relative: &str) -> String {
        match self.range {
            Some(range) => format!("{relative}:{}", range.spec()),
            None => relative.to_string(),
        }
    }
}

/// Parse a target argument.
///
/// The split is on the *last* colon, and only when what follows parses as a range — so a path
/// containing a colon still works, and a typo fails as a missing file rather than as a
/// silently different range.
pub fn parse_target(arg: &str) -> Result<Target, Fail> {
    if let Some(at) = arg.rfind('@') {
        let (head, tail) = (&arg[..at], &arg[at + 1..]);
        let plausible_symbol = !tail.is_empty()
            && tail
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == ':')
            && tail
                .chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_');
        if !head.is_empty() && plausible_symbol {
            return Err(Fail::usage(format!(
                "`{arg}` is a symbol target, which is not implemented yet · address the lines \
                 directly, as {head}:40-60"
            )));
        }
    }

    let (path, spec) = match arg.rfind(':') {
        Some(colon) => {
            let (head, tail) = (&arg[..colon], &arg[colon + 1..]);
            if !head.is_empty() && looks_like_range(tail) {
                (head, Some(tail))
            } else {
                (arg, None)
            }
        }
        None => (arg, None),
    };

    if path.is_empty() {
        return Err(Fail::usage(format!("`{arg}` has no path")));
    }
    let range = match spec {
        Some(spec) => Some(parse_range(spec)?),
        None => None,
    };
    Ok(Target {
        path: path.to_string(),
        range,
    })
}

fn looks_like_range(tail: &str) -> bool {
    if tail.is_empty() {
        return false;
    }
    let parts = tail.split(['-', '+']).collect::<Vec<_>>();
    if parts.len() > 2 {
        return false;
    }
    parts
        .iter()
        .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

fn parse_range(spec: &str) -> Result<Range, Fail> {
    let number = |text: &str| -> Result<usize, Fail> {
        text.parse::<usize>()
            .map_err(|_| Fail::usage(format!("`{spec}` is not a line range")))
    };

    let range = if let Some((start, end)) = spec.split_once('-') {
        let (start, end) = (number(start)?, number(end)?);
        if end < start {
            return Err(Fail::usage(format!("`{spec}` ends before it starts")));
        }
        Range::Span(start, end)
    } else if let Some((start, count)) = spec.split_once('+') {
        let (start, count) = (number(start)?, number(count)?);
        if count == 0 {
            return Err(Fail::usage(format!("`{spec}` asks for no lines")));
        }
        Range::From(start, count)
    } else {
        Range::Line(number(spec)?)
    };

    if range.start() == 0 || matches!(range, Range::Span(0, _)) {
        return Err(Fail::usage(format!(
            "`{spec}`: line numbers are 1-based, so 0 is not a line"
        )));
    }
    Ok(range)
}
