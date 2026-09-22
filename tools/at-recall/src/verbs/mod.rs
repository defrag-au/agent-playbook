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
    /// Whether the caller named the root rather than letting it be discovered. An exit that
    /// dropped a `--root` it was given would answer about a different tree, so it is echoed.
    pub root_was_explicit: bool,
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
    /// Whether the caller asked for the frame and not the rows: the verb computes what it needs to
    /// state the shape, and prints none of the body.
    pub summary: bool,
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

/// The caller's own invocation, asked again with one thing changed — the same comparison, widened
/// or deepened.
///
/// Rebuilt from the parts the caller supplied rather than typed out as a literal, so an exit cannot
/// describe a command the parser would refuse, and cannot silently answer about a different tree:
/// `--root` is echoed whenever the caller gave one, and nothing else about the invocation is
/// assumed. A flag's spelling is the one thing a verb has to supply, which is why
/// `tests/contract.rs` runs every exit it prints.
pub fn again(
    opts: &Opts,
    verb: &str,
    rev: Option<&str>,
    paths: &[String],
    flags: &[String],
) -> String {
    let mut command = format!("{} {verb}", crate::TOOL);
    for part in rev
        .map(String::from)
        .into_iter()
        .chain(paths.iter().cloned())
    {
        command.push(' ');
        command.push_str(&quoted(&part));
    }
    for flag in flags {
        command.push(' ');
        command.push_str(flag);
    }
    if opts.root_was_explicit {
        let dir = opts.root.dir().display().to_string();
        command.push_str(&format!(" --root {}", quoted(&dir)));
    }
    command
}

/// Single quotes when a shell would need them, and only then — an exit should read like the command
/// a person would type, not like a serialised argument list. `'` is closed and reopened, which is
/// the only escape a single-quoted shell word needs.
fn quoted(part: &str) -> String {
    let plain = part
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "._/-~=:,+^@{}()[]".contains(c));
    if plain {
        return part.to_string();
    }
    format!("'{}'", part.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_path_is_not_quoted() {
        assert_eq!(quoted("src/main.rs"), "src/main.rs");
        assert_eq!(quoted("HEAD~1..HEAD"), "HEAD~1..HEAD");
    }

    #[test]
    fn a_path_with_a_space_is_quoted_once() {
        assert_eq!(quoted("src/my file.rs"), "'src/my file.rs'");
    }

    #[test]
    fn a_quote_in_a_path_survives_a_shell() {
        assert_eq!(quoted("it's.rs"), r"'it'\''s.rs'");
    }
}
