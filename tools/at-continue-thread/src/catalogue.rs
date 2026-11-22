//! `at-continue-thread`'s verb table. The vocabulary and the help rendering live in `at-core`, so
//! the help text and the parser read the same table.

pub use at_core::catalogue::{exit_codes, flag_names, Flag, Tool, Verb};

const DB: Flag = Flag {
    name: "--db",
    takes_value: true,
    value: "<path>",
    one_line: "Read a thread store other than the platform default",
};
const BUDGET: Flag = Flag {
    name: "--budget-kb",
    takes_value: true,
    value: "N",
    one_line: "Agent-activity budget in kB; default 64, ceiling 256. Clamping is announced",
};
const LIMIT: Flag = Flag {
    name: "--limit",
    takes_value: true,
    value: "N",
    one_line: "Rows `find` may print; default 20, ceiling 200. Clamping is announced",
};
const HOW: Flag = Flag {
    name: "--help",
    takes_value: false,
    value: "",
    one_line: "This verb's flags (`-h` is the same flag)",
};

pub static VERBS: &[Verb] = &[
    Verb {
        name: "digest",
        question: "Enough of one thread to continue it in a new one",
        usage: "at-continue-thread digest <id-or-title> [--budget-kb N] [--db <path>]",
        notes: "The thread is resolved by id, or by a unique fragment of its title; an ambiguous \
                fragment is refused and the candidates are listed rather than guessed between. The \
                digest keeps the user's messages, the files touched and a head-and-tail window of \
                the agent's activity. Images are replaced by a one-line placeholder naming the file \
                and its size — the base64 is the reason a thread this large cannot be sent — and \
                thinking is dropped. Tool results are clipped to a line each. The activity window is \
                budgeted; the middle is what goes, and the bound says how much. Nothing is \
                rewritten: the thread in the store is left exactly as it was.",
        flags: &[BUDGET, DB, HOW],
    },
    Verb {
        name: "find",
        question: "Which threads match this title fragment",
        usage: "at-continue-thread find <title-fragment> [--limit N] [--db <path>]",
        notes: "One line per thread: id, when it was last updated, its stored size and its title. \
                Use this to resolve a fragment that `digest` refused as ambiguous, then pass the id. \
                It reads no blobs, so it is cheap even against a store with hundreds of threads.",
        flags: &[LIMIT, DB, HOW],
    },
];

static SPEC: Tool = Tool {
    name: crate::TOOL,
    version: crate::VERSION,
    about: "read-only continuation of a Zed agent thread",
    promise: "Reads one file outside the tree, Zed's threads.db, read-only. No subprocess, no \
              network, no write path.",
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
