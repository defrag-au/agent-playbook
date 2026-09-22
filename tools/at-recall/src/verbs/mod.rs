//! The verbs.
//!
//! Each returns an [`Outcome`] — a report and the exit code it implies — from the shared contract
//! in `at-core`, so `at-recall` answers in exactly the shape `at-peek` does and one set of reading
//! habits covers both. The output contract is the same document in both tools, and it has to be:
//! a bound stated one way here and another way there is not a contract.
//!
//! A diff is the easiest place in the toolkit to lose that property, because git will print a
//! hundred thousand lines without being asked twice. So the verbs here spend their budget while
//! building the answer rather than trimming it afterwards, and every cut is a `#` line.

pub mod diff;
pub mod state;

use at_core::contract::{truncate, Report, MAX_LINE_WIDTH};
use at_core::paths::Root;

use crate::git::Git;

// Re-exported under the names the verbs use, the way `at-peek` does it: the answer's shape is the
// contract's, and a verb should not have to name `at_core` to return one.
pub use at_core::contract::{plural, Outcome};

/// What a verb needs that is not its own arguments.
pub struct Opts {
    /// The worktree, canonicalised. Every path git prints is relative to this.
    pub root: Root,
    /// The one subprocess behind every verb.
    pub git: Git,
    /// Content lines this invocation may print, shared across every section it prints.
    pub limit: usize,
    /// Set when the caller asked for more than the ceiling, so the clamp is announced rather than
    /// silently applied.
    pub limit_clamped_from: Option<usize>,
    /// Whether `--patch` may print content from a secret-shaped path.
    pub include_secret_paths: bool,
    /// Whether hunks were asked for as well as the per-file table.
    pub patch: bool,
}

/// What one verb may still print, and what it gave up to stay inside `--limit`.
///
/// The counters are what the verb turns into `#` lines afterwards: a bound a reader cannot see is
/// not a bound, so nothing here is allowed to stay internal.
pub struct Budget {
    remaining: usize,
    dropped: usize,
    cut: usize,
}

impl Budget {
    pub fn new(limit: usize) -> Budget {
        Budget {
            remaining: limit,
            dropped: 0,
            cut: 0,
        }
    }

    /// Print one line as content.
    ///
    /// Past the limit the line is dropped rather than shortened, because a half-line reads as the
    /// whole thing; at the width cap it is cut and flagged, the way `at-peek slice` treats a long
    /// line. Either way the verb states the count afterwards.
    pub fn push(&mut self, report: &mut Report, line: impl Into<String>) {
        if self.remaining == 0 {
            self.dropped += 1;
            return;
        }
        let (text, cut) = truncate(&line.into());
        if cut {
            self.cut += 1;
        }
        report.content(text);
        self.remaining -= 1;
    }

    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// The width caveat, when there is one to state.
    pub fn width_caveat(&self, report: &mut Report) {
        if self.cut > 0 {
            report.bound(format!(
                "caveat: {} line(s) wider than {MAX_LINE_WIDTH} characters, truncated",
                self.cut
            ));
        }
    }
}
