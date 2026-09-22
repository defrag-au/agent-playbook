//! `at-recall`'s verb table. The vocabulary and the help rendering live in `at-core`, shared with
//! `at-peek`, so the two tools cannot drift in how they state what they accept — and a second
//! parser would be a second closed grammar, which is not a closed grammar.

// Re-exported rather than imported privately, so a module that already depends on the catalogue —
// the argument parser, which reads its flag table — does not have to reach past it into `at-core`
// for a type.
pub use at_core::catalogue::{exit_codes, flag_names, Flag, Tool, Verb, SUMMARY};

const LIMIT: Flag = Flag {
    name: "--limit",
    takes_value: true,
    value: "N",
    one_line: "Cap the output; default 200, ceiling 2000. Clamping is announced",
};
const IGNORE_SPACE: Flag = Flag {
    name: "--ignore-space",
    takes_value: false,
    value: "",
    one_line: "Compare lines ignoring whitespace (`git diff -w`), and state what that hid",
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
const BASE: Flag = Flag {
    name: "--base",
    takes_value: true,
    value: "<rev>",
    one_line: "What the branch diverged from; default is its upstream, else origin/HEAD",
};
const WITH: Flag = Flag {
    name: "--with",
    takes_value: true,
    value: "<sections>",
    one_line: "Comma-separated sections to print; default is all of them",
};

/// What `at-recall pr` prints, in the order it prints it. The names live here because the verb's
/// help, `at-describe`'s catalogue and `--with`'s validation all read them from one place.
pub const PR_SECTIONS: &[&str] = &["commits", "diffstat", "areas"];

/// A recipe: a verb whose answer is several sections rather than one, so a reader can ask for a
/// subset and a reviewer can read the set.
///
/// The sections are a reference to the slice `--with` validates against rather than a second copy
/// of it, so a section can neither be offered here and refused by the parser nor the reverse.
pub struct Recipe {
    pub verb: &'static str,
    pub one_line: &'static str,
    pub sections: &'static [&'static str],
}

pub const RECIPES: &[Recipe] = &[Recipe {
    verb: "pr",
    one_line: "the facts a PR description is written from",
    sections: PR_SECTIONS,
}];

pub static VERBS: &[Verb] = &[
    Verb {
        name: "state",
        question: "What am I looking at",
        usage: "at-recall state [--summary] [--limit N] [--root <path>]",
        notes: "Branch, HEAD, upstream divergence, any merge, rebase, cherry-pick, revert or \
                bisect in progress, and the changed paths with `git status --short` codes. Exits 0 \
                on a clean tree too, because the branch and HEAD are part of the answer — read the \
                `#` bound for the count. Untracked directories are shown collapsed, ending in `/`.",
        flags: &[SUMMARY, LIMIT, ROOT, HOW],
    },
    Verb {
        name: "log",
        question: "What changed lately",
        usage: "at-recall log [<rev>] [<path>…] [--limit N] [--root <path>]",
        notes: "One line per commit, newest first: short hash, date, author, subject. Paths are a \
                filter, so `log <path>` is that file's history and not the repository's, and the \
                count under the listing comes from walking the same revision and paths — so the \
                total is the number the listing was drawn from. Author and date filters are flags \
                on this question and are not here yet.",
        flags: &[LIMIT, ROOT, HOW],
    },
    Verb {
        name: "diff",
        question: "What is the difference",
        usage: "at-recall diff [<rev>] [<path>…] [--patch] [--summary] [--limit N] [--root <path>]",
        notes: "The working tree against HEAD by default, as a per-file table with the totals; \
                `--patch` adds the hunks. A first argument that resolves to a commit is a \
                revision and anything else is a path, which is the rule git uses, and several \
                paths are one invocation. The table names every changed path; `--patch` withholds \
                the hunks of a secret-shaped one and says which. A path git does not track is \
                named rather than passed over, because no difference looks like no change.",
        flags: &[PATCH, IGNORE_SPACE, SUMMARY, LIMIT, ROOT, SECRETS, HOW],
    },
    Verb {
        name: "pr",
        question: "The facts a PR description is written from",
        usage: "at-recall pr [--base <rev>] [--with <sections>] [--limit N] [--root <path>]",
        notes: "One recipe with three sections, the whole branch in one read: `commits` (its own \
                history), `diffstat` (every changed path with its counts and its kind) and `areas` \
                (the change set by kind, which is what the first paragraph of a description is \
                written from). The base is `--base`, else the branch's upstream, else `origin/HEAD`; \
                nothing is fetched, so it is the ref this repository has locally and the output says \
                so on every run. It prints facts and never conclusions — a path's kind comes from \
                its name, not its contents — and it does not write the description, because why the \
                change exists is the one fact not in the repository. Every section carries its \
                count, so a cut section says what it was cut from, and a branch with nothing on top \
                of its base exits 1 rather than 0: there is no description to write.",
        flags: &[BASE, WITH, LIMIT, ROOT, HOW],
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

/// The recipes, one line each, in the shape [`verb_lines`] renders the verbs — and naming the
/// binary, because the toolkit view lists the recipes of every binary under one heading.
pub fn recipe_lines() -> Vec<String> {
    RECIPES
        .iter()
        .map(|recipe| {
            format!(
                "  {} {} · {} · sections: {}",
                crate::TOOL,
                recipe.verb,
                recipe.one_line,
                recipe.sections.join(", ")
            )
        })
        .collect()
}

pub fn help_all() -> String {
    at_core::catalogue::help_all(&SPEC)
}

pub fn help_for(verb: &Verb) -> String {
    at_core::catalogue::help_for(&SPEC, verb)
}
