//! `at-cost`'s verb table. The vocabulary and the help rendering live in `at-core`, shared with
//! every other tool, so the tools cannot drift in how they state what they accept.

pub use at_core::catalogue::{exit_codes, flag_names, Flag, Tool, Verb};

const LIMIT: Flag = Flag {
    name: "--limit",
    takes_value: true,
    value: "N",
    one_line: "Cap the rows; default 200, ceiling 2000. Clamping is announced",
};
const HOW: Flag = Flag {
    name: "--help",
    takes_value: false,
    value: "",
    one_line: "This verb's flags (`-h` is the same flag)",
};

const VERB_FLAGS: &[Flag] = &[LIMIT, HOW];

pub static VERBS: &[Verb] = &[
    Verb {
        name: "repos",
        question: "Which repository is costing me",
        usage: "at-cost repos [--limit N]",
        notes: "One row per repository, found by walking each process's working directory up to \
                the nearest `.git`, falling back to the executable's own path. Credits each \
                process's own lifetime CPU and wakeups rather than a sample window, so a server \
                left running for three days shows three days. `avg%` is a floor: it divides that \
                CPU by how long the oldest process in the bucket has existed. A process with no \
                repository anywhere — an app launched from Finder has `/` as its directory — is \
                counted in a caveat, not a row. Listening ports are not read yet.",
        flags: VERB_FLAGS,
    },
    Verb {
        name: "tree",
        question: "What is still running under it",
        usage: "at-cost tree <repo> [--limit N]",
        notes: "The process tree for one repository, rooted at whatever still has a live parent. \
                A `!` marks a process whose parent is gone — the shape of an agent that started \
                a server and never stopped it. The last column is `argv` with the value of any \
                credential-shaped flag replaced.",
        flags: VERB_FLAGS,
    },
    Verb {
        name: "pid",
        question: "Which thread inside one process is hot",
        usage: "at-cost pid <pid> [--limit N]",
        notes: "Per-thread time as a share of the process's own CPU, from its rusage counters. \
                The share is exact; the seconds beside it are that share applied to the same \
                total, so the column cannot disagree with the process it came from. Thread \
                names are whatever the process registered, and often nothing.",
        flags: VERB_FLAGS,
    },
];

static SPEC: Tool = Tool {
    name: crate::TOOL,
    version: crate::VERSION,
    about: "what the machine is costing, by repository",
    promise: "No subprocess, no socket, no writes, no network, no environment configuration. \
              Reads the macOS process table and nothing else.",
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
