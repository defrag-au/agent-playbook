//! `at-recall`'s verb table. The vocabulary and the help rendering live in `at-core`, shared with
//! `at-peek`, so the two tools cannot drift in how they state what they accept — and a second
//! parser would be a second closed grammar, which is not a closed grammar.

// Re-exported rather than imported privately, so a module that already depends on the catalogue —
// the argument parser, which reads its flag table — does not have to reach past it into `at-core`
// for a type.
pub use at_core::catalogue::{exit_codes, flag_names, Flag, Tool, Verb};

const LIMIT: Flag = Flag {
    name: "--limit",
    takes_value: true,
    value: "N",
    one_line: "Cap the output; default 200, ceiling 2000. Clamping is announced",
};
const ROOT: Flag = Flag {
    name: "--root",
    takes_value: true,
    value: "<path>",
    one_line: "Read a worktree other than the one enclosing the shell",
};
const PATCH: Flag = Flag {
    name: "--patch",
    takes_value: false,
    value: "",
    one_line: "The hunks themselves, not only the per-file totals",
};
const SECRETS: Flag = Flag {
    name: "--include-secret-paths",
    takes_value: false,
    value: "",
    one_line: "Include a secret-shaped path that `--patch` refused by name",
};
const HOW: Flag = Flag {
    name: "--help",
    takes_value: false,
    value: "",
    one_line: "This verb's flags (`-h` is the same flag)",
};

pub static VERBS: &[Verb] = &[
    Verb {
        name: "state",
        question: "What am I looking at",
        usage: "at-recall state [--limit N] [--root <path>]",
        notes: "Branch, HEAD, upstream divergence, any merge, rebase, cherry-pick, revert or \
                bisect in progress, and the changed paths with `git status --short` codes. Exits 0 \
                on a clean tree too, because the branch and HEAD are part of the answer — read the \
                `#` bound for the count. Untracked directories are shown collapsed, ending in `/`.",
        flags: &[LIMIT, ROOT, HOW],
    },
    Verb {
        name: "diff",
        question: "What is the difference",
        usage: "at-recall diff [<rev>] [<path>…] [--patch] [--limit N] [--root <path>]",
        notes: "The working tree against HEAD by default, as a per-file table with the totals; \
                `--patch` adds the hunks. A first argument that resolves to a commit is a \
                revision and anything else is a path, which is the rule git uses, and several \
                paths are one invocation. The table names every changed path; `--patch` withholds \
                the hunks of a secret-shaped one and says which. A path git does not track is \
                named rather than passed over, because no difference looks like no change.",
        flags: &[PATCH, LIMIT, ROOT, SECRETS, HOW],
    },
];

static SPEC: Tool = Tool {
    name: crate::TOOL,
    version: crate::VERSION,
    about: "read-only inspection of history and the working tree",
    promise: "One subprocess, git, read verbs only. No writes, no network, no environment \
              configuration, and never the reflog.",
    verbs: VERBS,
};

pub fn find(name: &str) -> Option<&'static Verb> {
    at_core::catalogue::find(VERBS, name)
}

pub fn names() -> String {
    at_core::catalogue::names(VERBS)
}

pub fn verb_lines() -> Vec<String> {
    at_core::catalogue::verb_lines(VERBS)
}

pub fn help_all() -> String {
    at_core::catalogue::help_all(&SPEC)
}

pub fn help_for(verb: &Verb) -> String {
    at_core::catalogue::help_for(&SPEC, verb)
}
