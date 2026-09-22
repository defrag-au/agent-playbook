//! The verbs.
//!
//! Each returns an [`Outcome`] — a report and the exit code it implies — so that "nothing
//! found" is a fact the caller can branch on rather than an empty success. Any target that
//! fails fails the whole invocation: a partial result that looks complete is the failure this
//! toolkit exists to remove, and it is cheaper to re-run without the bad target than to
//! notice that one line of a batch was missing.

pub mod slice;
pub mod stat;

use std::fs;
use std::time::SystemTime;

use crate::contract::{secret_shaped, Exit, Fail, Report};
use crate::paths::{self, Root, Target};

pub struct Opts {
    pub root: Root,
    pub limit: usize,
    /// Set when the caller asked for more than [`crate::contract::MAX_LIMIT`], so the clamp
    /// can be announced rather than silently applied.
    pub limit_clamped_from: Option<usize>,
    pub include_secret_paths: bool,
}

pub struct Outcome {
    pub report: Report,
    pub exit: Exit,
}

impl Outcome {
    pub fn from_report(report: Report) -> Outcome {
        let exit = if report.content_lines() > 0 {
            Exit::Results
        } else {
            Exit::Nothing
        };
        Outcome { report, exit }
    }
}

/// A target that resolved, survived the deny-list, and read as text.
pub struct Opened {
    pub target: Target,
    /// Relative to the root, forward slashes — the form a transcript can cite.
    pub rel: String,
    pub text: String,
    pub len: u64,
    pub modified: Option<SystemTime>,
    /// Byte offset of the first NUL, when the file is not line-addressable.
    pub binary: Option<usize>,
}

impl Opened {
    /// Lines as a reader counts them: a trailing newline does not add one.
    pub fn lines(&self) -> usize {
        self.text.lines().count()
    }
}

/// Resolve and read one target, or say why not.
pub fn open(opts: &Opts, arg: &str) -> Result<Opened, Fail> {
    let target = paths::parse_target(arg)?;
    let abs = opts.root.resolve(&target.path)?;
    let rel = opts.root.relative(&abs);

    if !opts.include_secret_paths {
        let name = abs
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Some(rule) = secret_shaped(&name) {
            return Err(Fail::refused(format!(
                "{rel} is refused: {rule} · --include-secret-paths reads it anyway"
            )));
        }
    }

    let meta =
        fs::metadata(&abs).map_err(|e| Fail::environment(format!("cannot stat {rel}: {e}")))?;
    if meta.is_dir() {
        return Err(Fail::environment(format!(
            "{rel} is a directory, not a file"
        )));
    }

    let len = meta.len();
    let modified = meta.modified().ok();
    let bytes = fs::read(&abs).map_err(|e| Fail::environment(format!("cannot read {rel}: {e}")))?;

    if let Some(offset) = bytes.iter().take(8192).position(|byte| *byte == 0) {
        return Ok(Opened {
            target,
            rel,
            text: String::new(),
            len,
            modified,
            binary: Some(offset),
        });
    }

    let text = String::from_utf8(bytes).map_err(|e| {
        Fail::environment(format!(
            "{rel} is not UTF-8 text (first bad byte at offset {})",
            e.utf8_error().valid_up_to()
        ))
    })?;
    Ok(Opened {
        target,
        rel,
        text,
        len,
        modified,
        binary: None,
    })
}

/// `1 path` / `3 paths`, because "1 path(s)" reads like a tool that is not sure.
pub fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}
