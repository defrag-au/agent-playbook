//! The verbs.
//!
//! Each returns an [`Outcome`] — a report and the exit code it implies — so that "nothing
//! found" is a fact the caller can branch on rather than an empty success, and so a verb cannot
//! print content and then exit as though it had found none. Both live in the contract in
//! `at-core`, because `at-recall` answers in the same shape and the two must not drift. Any
//! target that fails fails the whole invocation: a partial result that looks complete is the
//! failure this toolkit exists to remove, and it is cheaper to re-run without the bad target
//! than to notice that one line of a batch was missing.

pub mod search;
pub mod slice;
pub mod stat;

use std::fs;
use std::path::Path;
use std::time::SystemTime;

use crate::contract::{secret_shaped, Fail};
use crate::paths::{self, Root, Target};

pub use crate::contract::{plural, Outcome};

pub struct Opts {
    pub root: Root,
    pub limit: usize,
    /// Set when the caller asked for more than [`crate::contract::MAX_LIMIT`], so the clamp
    /// can be announced rather than silently applied.
    pub limit_clamped_from: Option<usize>,
    /// Files a walk may consider before it stops and says so.
    pub max_files: usize,
    pub max_files_clamped_from: Option<usize>,
    pub include_secret_paths: bool,
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

/// What reading a file produced. A binary or non-UTF-8 file is a fact about the file rather
/// than a failure: `search` counts it and moves on, `slice` says so and shows nothing.
pub enum Content {
    Text(String),
    Binary(usize),
    NotUtf8(usize),
}

/// Read a file that is already known to be inside the root. `name` is what the caller wants
/// in a message — a path relative to the root, never the absolute one.
pub fn read_text(path: &Path, name: &str) -> Result<Content, Fail> {
    let bytes =
        fs::read(path).map_err(|e| Fail::environment(format!("cannot read {name}: {e}")))?;
    if let Some(offset) = bytes.iter().take(8192).position(|byte| *byte == 0) {
        return Ok(Content::Binary(offset));
    }
    match String::from_utf8(bytes) {
        Ok(text) => Ok(Content::Text(text)),
        Err(e) => Ok(Content::NotUtf8(e.utf8_error().valid_up_to())),
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

    match read_text(&abs, &rel)? {
        Content::Text(text) => Ok(Opened {
            target,
            rel,
            text,
            len,
            modified,
            binary: None,
        }),
        Content::Binary(offset) => Ok(Opened {
            target,
            rel,
            text: String::new(),
            len,
            modified,
            binary: Some(offset),
        }),
        Content::NotUtf8(offset) => Err(Fail::environment(format!(
            "{rel} is not UTF-8 text (first bad byte at offset {offset})"
        ))),
    }
}
