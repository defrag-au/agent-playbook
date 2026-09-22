//! The one subprocess this toolkit runs, pinned so that "read-only" is a property of the code
//! rather than a hope about how git happens to be invoked.
//!
//! Three things make it auditable:
//!
//! * [`Noun`] is the complete set of git commands the tool can run. No string other than
//!   [`Noun::as_str`] ever reaches `Command::new`, so adding a command is a diff in a closed list
//!   rather than a new call site inside a verb.
//! * The invocation neutralises everything a repository or a user can configure to make git run
//!   *itself*: pagers, `core.fsmonitor`, hooks, external diff drivers, textconv filters, and
//!   system and global config entirely. `tests/contract.rs` proves it by configuring a hostile
//!   pager and diff driver and asserting nothing ran.
//! * Revisions are validated before they are passed, so one cannot become an option, and `@{…}` —
//!   the reflog — is not expressible. The tool reads objects, never the reflog.
//!
//! Aliases are not a hole either: tested, a git alias cannot shadow a built-in command, so
//! invoking the exec-path binary is not necessary.

use std::path::{Path, PathBuf};
use std::process::Command;

use at_core::contract::Fail;

/// Every git command `at-recall` may run. All of them read; [`MUTATING`] holds the words that must
/// never appear here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Noun {
    Blame,
    CatFile,
    CheckAttr,
    Diff,
    ForEachRef,
    Log,
    RevList,
    RevParse,
    Status,
}

impl Noun {
    pub fn as_str(self) -> &'static str {
        match self {
            Noun::Blame => "blame",
            Noun::CatFile => "cat-file",
            Noun::CheckAttr => "check-attr",
            Noun::Diff => "diff",
            Noun::ForEachRef => "for-each-ref",
            Noun::Log => "log",
            Noun::RevList => "rev-list",
            Noun::RevParse => "rev-parse",
            Noun::Status => "status",
        }
    }

    pub fn all() -> [Noun; 9] {
        [
            Noun::Blame,
            Noun::CatFile,
            Noun::CheckAttr,
            Noun::Diff,
            Noun::ForEachRef,
            Noun::Log,
            Noun::RevList,
            Noun::RevParse,
            Noun::Status,
        ]
    }
}

/// Words a read-only tool must never run. A test asserts none of them is in [`Noun`]: the enum is
/// the boundary, and this is what a careless addition has to get past.
pub const MUTATING: &[&str] = &[
    "add",
    "am",
    "apply",
    "branch",
    "checkout",
    "cherry-pick",
    "clean",
    "commit",
    "config",
    "fetch",
    "filter-branch",
    "gc",
    "hash-object",
    "merge",
    "mv",
    "notes",
    "prune",
    "pull",
    "push",
    "rebase",
    "reflog",
    "reset",
    "restore",
    "revert",
    "rm",
    "stash",
    "submodule",
    "switch",
    "tag",
    "update-index",
    "update-ref",
    "worktree",
];

/// The program, as a constant, so that "what can this binary execute" has one answer in the source.
pub const GIT: &str = "git";

/// Paths per `check-attr` invocation. Conservative: a path can be long, and the argument list has
/// to survive an operating system limit that reports itself as a failing command rather than as a
/// long one.
const CHECK_ATTR_CHUNK: usize = 500;

pub struct Git {
    root: PathBuf,
}

struct Output {
    ok: bool,
    stdout: String,
    stderr: String,
}

impl Git {
    pub fn at(root: &Path) -> Git {
        Git {
            root: root.to_path_buf(),
        }
    }

    /// One read command, or the reason it could not be read.
    pub fn run(&self, noun: Noun, args: &[&str]) -> Result<String, Fail> {
        let output = self.invoke(noun, args)?;
        if !output.ok {
            return Err(Fail::environment(format!(
                "git {} failed: {}",
                noun.as_str(),
                first_line(&output.stderr)
            )));
        }
        Ok(output.stdout)
    }

    /// For a probe whose failure is itself the answer — "is there a MERGE_HEAD", "is there an
    /// upstream". `None` means git declined, which is a fact rather than an error.
    pub fn probe(&self, noun: Noun, args: &[&str]) -> Option<String> {
        let output = self.invoke(noun, args).ok()?;
        output.ok.then_some(output.stdout)
    }

    fn invoke(&self, noun: Noun, args: &[&str]) -> Result<Output, Fail> {
        let mut argv: Vec<String> = [
            "core.pager=cat",
            "core.fsmonitor=false",
            "core.hooksPath=/dev/null",
            "diff.external=",
            "core.quotePath=false",
        ]
        .iter()
        .flat_map(|setting| ["-c".to_string(), setting.to_string()])
        .collect();
        argv.push("-c".to_string());
        argv.push(format!("safe.directory={}", self.root.display()));
        argv.push("--no-pager".to_string());
        argv.push("--no-optional-locks".to_string());
        argv.push(noun.as_str().to_string());
        argv.extend(args.iter().map(|arg| arg.to_string()));

        let output = Command::new(GIT)
            .current_dir(&self.root)
            .args(&argv)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("LC_ALL", "C")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_PAGER")
            .env_remove("GIT_EXTERNAL_DIFF")
            .output()
            .map_err(|e| Fail::environment(format!("cannot run {GIT}: {e}")))?;

        Ok(Output {
            ok: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    /// Whether the commit named by `rev` exists. Used to disambiguate `diff`'s first argument the
    /// way git does: a name that resolves to a commit is a revision, anything else is a path.
    pub fn resolves(&self, rev: &str) -> bool {
        self.probe(
            Noun::RevParse,
            &["--verify", "--quiet", &format!("{rev}^{{commit}}")],
        )
        .is_some()
    }

    /// Whether the token names a revision this tool should treat as one — a commit, or a range
    /// whose two ends both resolve. Checked end by end rather than as a whole, because `--verify`
    /// asks about single revisions: `A..B` is not one.
    pub fn names_a_revision(&self, token: &str) -> bool {
        match split_range(token) {
            Some((left, right)) => self.resolves(left) && self.resolves(right),
            None => self.resolves(token),
        }
    }

    /// The `filter` attribute of each path, as `(path, value)`.
    ///
    /// `check-attr` is the only thing that knows what `.gitattributes` resolves to — in-tree, in
    /// `.git/info`, and in any global file — without reading a worktree file. Chunked because the
    /// paths arrive as arguments: a large change set would otherwise run into the argument limit
    /// and fail in a way that looked like a broken repository.
    pub fn check_filter_attribute(&self, paths: &[String]) -> Result<Vec<(String, String)>, Fail> {
        let mut attributes = Vec::new();
        for chunk in paths.chunks(CHECK_ATTR_CHUNK) {
            let mut args: Vec<&str> = vec!["filter", "--"];
            args.extend(chunk.iter().map(String::as_str));
            let raw = self.run(Noun::CheckAttr, &args)?;
            for line in raw.lines() {
                // `path: filter: value`. Split from the right, because a path may contain `: `.
                if let Some((path, value)) = line.rsplit_once(": filter: ") {
                    attributes.push((path.to_string(), value.to_string()));
                }
            }
        }
        Ok(attributes)
    }

    pub fn head_exists(&self) -> bool {
        self.probe(Noun::RevParse, &["--verify", "--quiet", "HEAD"])
            .is_some()
    }
}

/// Revisions this tool will not read, refused by name rather than left to fail somewhere else.
///
/// Reflog syntax is called out separately from the character set below because the refusal has to
/// be independent of whether the reference resolves: `HEAD@{1}` in a repository with one commit is
/// not a resolution this tool wants, it is the reflog, and it is not read.
pub fn refuse_reflog(token: &str) -> Result<(), Fail> {
    if token.contains("@{") {
        return Err(Fail::usage(format!(
            "`{token}` is reflog syntax, and this tool does not read the reflog · address the commit \
             itself"
        )));
    }
    Ok(())
}

/// A revision the caller supplied, validated.
///
/// Conservative on purpose: names, ancestry and separators are enough for everything this tool
/// answers. `{` and `}` are not in the set, which keeps `@{…}` — the reflog — outside its reach
/// rather than merely discouraged, and the check runs before the character set so that the
/// refusal explains itself.
pub fn rev(token: &str) -> Result<String, Fail> {
    if token.is_empty() {
        return Err(Fail::usage("empty revision"));
    }
    if token.starts_with('-') {
        return Err(Fail::usage(format!(
            "`{token}` starts with `-`, which git would read as a flag"
        )));
    }
    refuse_reflog(token)?;
    if token.contains('{') || token.contains('}') {
        return Err(Fail::usage(format!(
            "`{token}` is brace syntax, which this tool does not pass to git"
        )));
    }
    if !token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-' | '^' | '~' | '@'))
    {
        return Err(Fail::usage(format!(
            "`{token}` is not a revision this tool will pass to git"
        )));
    }
    Ok(token.to_string())
}

/// Split `A..B` or `A...B` into its two ends, an omitted end meaning HEAD — which is what git
/// means by it, and what `main..` is missing.
fn split_range(token: &str) -> Option<(&str, &str)> {
    // `...` before `..`, or the merge-base form splits into a name and `.B`.
    for separator in ["...", ".."] {
        if let Some((left, right)) = token.split_once(separator) {
            return Some((
                if left.is_empty() { "HEAD" } else { left },
                if right.is_empty() { "HEAD" } else { right },
            ));
        }
    }
    None
}

/// Pathspecs go after `--`. It is the one `--` in the toolkit's grammar, and it is ours rather
/// than a pass-through: it is what makes a path beginning with `-` a path rather than a flag.
/// Containment needs no checking here — git confines a pathspec to the repository itself.
pub fn with_paths(argv: &mut Vec<String>, paths: &[String]) {
    if paths.is_empty() {
        return;
    }
    argv.push("--".to_string());
    argv.extend(paths.iter().cloned());
}

/// Collect `&str` arguments into the owned form the runner takes.
pub fn owned(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| arg.to_string()).collect()
}

fn first_line(text: &str) -> String {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("no output")
        .trim()
        .to_string()
}
