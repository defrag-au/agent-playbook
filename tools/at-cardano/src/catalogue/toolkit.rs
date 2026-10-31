//! The verbs that are about the tool itself: the endpoint catalogue, the key, and the escape
//! hatch.

use super::{flags, Noun, Route, Verb};

pub static NOUNS: &[Noun] = &[ENDPOINTS, AUTH, QUERY, RUN];

const ENDPOINTS: Noun = Noun {
    name: "endpoints",
    question: "The complete Koios surface, and what answers each part of it",
    verbs: &[&ENDPOINTS_LIST],
};

const ENDPOINTS_LIST: Verb = Verb {
    name: "endpoints",
    question: "Which Koios endpoints exist, and which noun answers each",
    usage: "at-cardano endpoints [<text>] [--help]",
    notes: "Every endpoint in the published Koios spec (101 paths: 100 reads and /submittx), \
            with the method, the kind of target it takes, and the verb that answers it — or \
            `query`, for the long tail. A word filters endpoint, verb and question. This makes \
            coverage checkable rather than claimed.",
    flags: flags::PLAIN,
    route: Route::Endpoints,
};

const AUTH: Noun = Noun {
    name: "auth",
    question: "Which key and endpoint will be used",
    verbs: &[&AUTH_STATUS],
};

const AUTH_STATUS: Verb = Verb {
    name: "auth",
    question: "Which key and endpoint will be used",
    usage: "at-cardano auth [--network N] [--keyless]",
    notes: "Prints the endpoint, the key source and whether KOIOS_API_KEY is set. It makes no \
            request and never prints the key; it is how a run that is unexpectedly keyless is \
            diagnosed.",
    flags: flags::LOCAL,
    route: Route::Auth,
};

const QUERY: Noun = Noun {
    name: "query",
    question: "Anything else Koios answers",
    verbs: &[&QUERY_ANY],
};

const QUERY_ANY: Verb = Verb {
    name: "query",
    question: "Anything else Koios answers",
    usage: "at-cardano query <endpoint> [--post] [--body JSON | --body-file PATH] [--query k=v]…",
    notes: "The long tail: one Koios endpoint name, GET by default. It cannot choose a host, \
            add a header or follow a redirect, and /submittx is refused. The answer is raw \
            JSON, bounded to 64 KiB; narrow it at the source with --select or --limit.",
    flags: flags::QUERY_FLAGS,
    route: Route::Query,
};

const RUN: Noun = Noun {
    name: "run",
    question: "A scenario file: several questions, values carried between them, checks",
    verbs: &[&RUN_SCENARIO],
};

const RUN_SCENARIO: Verb = Verb {
    name: "run",
    question: "A scenario file: several questions, values carried between them, checks",
    usage: "at-cardano run --scenario <file.dsl> [--check] [--network N] [--keyless]",
    notes: "Runs one scenario file: `target` lines ask the same questions the command line \
            asks, `save` binds a value out of an answer, `assert` checks one and `echo` prints a \
            line. The grammar is closed — a scenario cannot name a host, add a header, or reach \
            /submittx — so allowlisting `at-cardano run` grants no more than allowlisting \
            `at-cardano`. Exit 0 means every check passed; 2 means one did not, or the file is \
            wrong. `--check` parses the file and resolves every reference, then stops before the \
            first request.",
    flags: flags::SCENARIO_FLAGS,
    route: Route::Scenario,
};
