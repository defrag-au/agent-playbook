//! The verbs.
//!
//! Each has a `collect` that reads and returns typed facts, and a `run` that renders those facts
//! into an [`Outcome`] — a report and the exit code it implies — from the shared contract in
//! `at-core`, so `at-recall` answers in exactly the shape `at-peek` does and one set of reading
//! habits covers both. The output contract is the same document in both tools, and it has to be:
//! a bound stated one way here and another way there is not a contract.
//!
//! A diff is the easiest place in the toolkit to lose that property, because git will print a
//! hundred thousand lines without being asked twice. So the verbs here spend their budget while
//! building the answer rather than trimming it afterwards, and every cut is a `#` line.

pub mod diff;
pub mod log;
pub mod pr;
pub mod state;

use at_core::contract::{truncate, Fail, Report, DEFAULT_LIMIT, MAX_LIMIT, MAX_LINE_WIDTH};
use at_core::paths::Root;

use crate::git::{Git, Noun};

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
    /// Whether a diff should ignore whitespace when comparing lines, and say what that hid.
    pub ignore_space: bool,
    /// The revision a recipe should compare against, when the caller named one. `None` means the
    /// verb resolves a default and says which one it used.
    pub base: Option<String>,
    /// The sections a recipe was asked to print. Empty means all of them.
    pub with: Vec<String>,
}

/// The flags a run is configured with, before the worktree is resolved.
///
/// One value rather than eight arguments: the CLI parses flags into it and a harness passes its
/// typed ones, so the two front ends cannot disagree about a default or a clamp.
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// Content lines this invocation may print.
    pub limit: Option<usize>,
    /// Whether `--patch` may print content from a secret-shaped path.
    pub include_secret_paths: bool,
    /// Whether hunks were asked for as well as the per-file table.
    pub patch: bool,
    /// Whether the caller asked for the frame and not the rows.
    pub summary: bool,
    /// Whether a diff should ignore whitespace when comparing lines.
    pub ignore_space: bool,
    /// The revision a recipe should compare against.
    pub base: Option<String>,
    /// The sections a recipe was asked to print.
    pub with: Vec<String>,
}

impl Opts {
    /// Resolve the worktree and the one subprocess a run needs.
    ///
    /// The named root when there is one, else the worktree enclosing the working directory. Git is
    /// asked where the top of the worktree is rather than trusting the guess: `--root` may name a
    /// subdirectory, and every path git prints is relative to the top. This is also the check that
    /// the tree is a worktree at all, so a repository-less directory fails with one clear sentence
    /// instead of four of git's.
    pub fn resolve(named_root: Option<&std::path::Path>, config: Config) -> Result<Opts, Fail> {
        let guess = match named_root {
            Some(path) => Root::at(path)?,
            None => {
                let cwd = std::env::current_dir().map_err(|e| {
                    Fail::environment(format!("cannot read the working directory: {e}"))
                })?;
                Root::discover(&cwd)?
            }
        };

        let probe = Git::at(guess.dir());
        let toplevel = probe
            .probe(Noun::RevParse, &["--show-toplevel"])
            .ok_or_else(|| {
                Fail::environment(format!(
                    "{} is not a git worktree · at-recall reads history, at-peek reads the working tree",
                    guess.dir().display()
                ))
            })?;
        let root = Root::at(std::path::Path::new(toplevel.trim()))?;
        let git = Git::at(root.dir());

        let (limit, limit_clamped_from) = match config.limit {
            Some(asked) => {
                if asked == 0 {
                    return Err(Fail::usage(
                        "--limit 0 asks for no output · the smallest useful limit is 1",
                    ));
                }
                if asked > MAX_LIMIT {
                    (MAX_LIMIT, Some(asked))
                } else {
                    (asked, None)
                }
            }
            None => (DEFAULT_LIMIT, None),
        };

        Ok(Opts {
            root,
            root_was_explicit: named_root.is_some(),
            git,
            limit,
            limit_clamped_from,
            include_secret_paths: config.include_secret_paths,
            patch: config.patch,
            summary: config.summary,
            ignore_space: config.ignore_space,
            base: config.base,
            with: config.with,
        })
    }
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

/// The verbs' spelling of [`at_core::contract::again`]: the revision and paths the caller gave, in
/// the order they gave them, and the root echoed whenever they named one.
pub fn again(
    opts: &Opts,
    verb: &str,
    rev: Option<&str>,
    paths: &[String],
    flags: &[String],
) -> String {
    let positionals: Vec<String> = rev
        .map(String::from)
        .into_iter()
        .chain(paths.iter().cloned())
        .collect();
    let root = opts
        .root_was_explicit
        .then(|| opts.root.dir().display().to_string());
    at_core::contract::again(crate::TOOL, verb, &positionals, flags, root.as_deref())
}

/// Split positionals into a revision and paths: the first argument is a revision when it names one
/// and a path otherwise — the rule git uses, so `log main` and `log src/main.rs` both mean what they
/// look like. Shared by `diff` and `log`, because a rule stated twice drifts once.
///
/// A name git can resolve but this tool will not pass — the reflog above all — is refused here
/// rather than quietly demoted to a path, because "that is reflog syntax and this tool does not read
/// the reflog" is the answer the caller needs.
pub fn split_rev_and_paths(
    args: &[String],
    opts: &Opts,
) -> Result<(Option<String>, Vec<String>), Fail> {
    match args.split_first() {
        Some((first, rest)) => {
            // Refused before git is asked anything: whether the reference resolves has nothing to do
            // with the fact that this tool does not read the reflog, and a `HEAD@{1}` that happens
            // not to exist must not be quietly read as a path.
            crate::git::refuse_reflog(first)?;
            if opts.git.names_a_revision(first) {
                return Ok((Some(crate::git::rev(first)?), rest.to_vec()));
            }
            // A token with a range in it is meant as one, whatever it resolves to. Reading it as a
            // path instead would answer "nothing changed" about a range that does not exist.
            if first.contains("..") {
                return Err(Fail::environment(format!(
                    "`{first}` is not a revision range in {} · a range needs both of its ends to \
                     resolve",
                    opts.root.name()
                )));
            }
            Ok((None, args.to_vec()))
        }
        None => Ok((None, Vec::new())),
    }
}
