//! `at-peek`'s verb table. The vocabulary and the help rendering live in `at-core`, shared with
//! `at-recall`, so the two tools cannot drift in how they state what they accept — and a second
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
const ROOT: Flag = Flag {
    name: "--root",
    takes_value: true,
    value: "<path>",
    one_line: "Resolve paths against this root instead of the enclosing worktree",
};
const SECRETS: Flag = Flag {
    name: "--include-secret-paths",
    takes_value: false,
    value: "",
    one_line: "Read a path the deny-list refused; the refusal names the rule it matched",
};
const HOW: Flag = Flag {
    name: "--help",
    takes_value: false,
    value: "",
    one_line: "This verb's flags (`-h` is the same flag)",
};
const COUNT: Flag = Flag {
    name: "--count",
    takes_value: false,
    value: "",
    one_line: "Matches per file, instead of the matching lines",
};
const FILES_ONLY: Flag = Flag {
    name: "--files-only",
    takes_value: false,
    value: "",
    one_line: "Paths containing a match, without lines",
};
const MAX_FILES: Flag = Flag {
    name: "--max-files",
    takes_value: true,
    value: "N",
    one_line: "Files a walk considers before it stops; default 20000, ceiling 200000",
};

const VERB_FLAGS: &[Flag] = &[LIMIT, ROOT, SECRETS, HOW];

pub static VERBS: &[Verb] = &[
    Verb {
        name: "stat",
        question: "How big is it, when did it change",
        usage: "at-peek stat <path>… [--limit N] [--root <path>] [--include-secret-paths]",
        notes: "Lines, bytes, modification time and language per path. The \"did the write land\" \
                verb. A binary is reported as binary rather than counted.",
        flags: VERB_FLAGS,
    },
    Verb {
        name: "slice",
        question: "These exact lines",
        usage: "at-peek slice <path>:40-60 [<path>:<range>…] [--limit N] [--root <path>]",
        notes: "Ranges are 1-based and inclusive; `40+20` means twenty lines from 40, and no range \
                reads the whole file. `--limit` is shared across targets, so a batch cannot print \
                more than one verb's worth. Any target that fails fails the invocation.",
        flags: VERB_FLAGS,
    },
    Verb {
        name: "search",
        question: "Every mention",
        usage: "at-peek search <pattern> [path…] [--count | --files-only] [--summary] [--limit N] [--max-files N]",
        notes: "Regex, walked in path order; no path searches the whole root. Matches are counted over every file the walk considers, so the total under the listing is the real one. Directory names skipped by rule are named in the output — real gitignore semantics are not here yet. `--count` and `--files-only` are alternatives, not options to combine; `--summary` is the coarser answer again — how many, and where the walk stopped — without the rows.",
        flags: &[
            COUNT,
            FILES_ONLY,
            SUMMARY,
            LIMIT,
            MAX_FILES,
            ROOT,
            SECRETS,
            HOW,
        ],
    },
];

static SPEC: Tool = Tool {
    name: crate::TOOL,
    version: crate::VERSION,
    about: "read-only inspection of the working tree",
    promise: "No git, no subprocess, no writes, no network, no environment configuration.",
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
