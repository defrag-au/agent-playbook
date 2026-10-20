//! `at-reach`'s verb table.

pub use at_core::catalogue::{exit_codes, flag_names, Flag, Tool, Verb};

const FLEET_ROOT: Flag = Flag {
    name: "--fleet-root",
    takes_value: true,
    value: "<path>",
    one_line: "Read this fleet tree instead of the one resolved by convention",
};
const HOW: Flag = Flag {
    name: "--help",
    takes_value: false,
    value: "",
    one_line: "This verb's flags (`-h` is the same flag)",
};

static VERB_FLAGS: &[Flag] = &[FLEET_ROOT, HOW];

pub static VERBS: &[Verb] = &[
    Verb {
        name: "root",
        question: "Which fleet tree is being read",
        usage: "at-reach root [--fleet-root <path>]",
        notes: "Prints the resolved fleet root, how it was found, and how many profiles it \
                contains. The same resolution order every other verb uses, so this is the read \
                that explains a surprising host list.",
        flags: VERB_FLAGS,
    },
    Verb {
        name: "hosts",
        question: "What machines are in the fleet",
        usage: "at-reach hosts [--fleet-root <path>]",
        notes: "One line per profile: id, role, OS/arch, Tailscale name, SSH alias and the repo \
                it writes. The profiles are private data; this binary only reads them.",
        flags: VERB_FLAGS,
    },
    Verb {
        name: "probe",
        question: "What is this machine doing",
        usage: "at-reach probe <host> [--fleet-root <path>]",
        notes: "Runs one fixed, read-only probe locally or over SSH: OS, arch, Nix, Home Manager, \
                Tailscale, disk, and the git HEAD/change count for each repo the profile names. \
                It never uses sudo and never runs a rebuild. `git status` may refresh the index, \
                which is the one write git itself performs.",
        flags: VERB_FLAGS,
    },
];

static SPEC: Tool = Tool {
    name: crate::TOOL,
    version: crate::VERSION,
    about: "read-only fleet inspection",
    promise: "Read-only fleet inspection. The probe is one sh or ssh invocation; no writes, no \
              sudo, no secrets. The only environment read is the fleet-root lookup and the local \
              hostname.",
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
