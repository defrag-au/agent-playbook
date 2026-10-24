//! `at-crate`'s verb table. The vocabulary and help rendering live in `at-core`, shared with
//! `at-peek`, `at-recall`, `at-reach` and `at-describe`, so the tools cannot drift in how they
//! state what they accept.

pub use at_core::catalogue::{exit_codes, flag_names, Flag, Tool, Verb, SUMMARY};

const LIMIT: Flag = Flag {
    name: "--limit",
    takes_value: true,
    value: "N",
    one_line: "Cap the output; default 200, ceiling 2000. Clamping is announced",
};
const MAX_FILES: Flag = Flag {
    name: "--max-files",
    takes_value: true,
    value: "N",
    one_line: "Files a walk considers before it stops; default 20000, ceiling 200000",
};
const CARGO_HOME: Flag = Flag {
    name: "--cargo-home",
    takes_value: true,
    value: "<path>",
    one_line: "Read this cargo home instead of $CARGO_HOME or ~/.cargo",
};
const PROJECT: Flag = Flag {
    name: "--project",
    takes_value: true,
    value: "<path>",
    one_line: "Resolve Cargo.lock and target/ from this directory instead of the working tree",
};
const TARGET_DIR: Flag = Flag {
    name: "--target-dir",
    takes_value: true,
    value: "<path>",
    one_line: "Read build output from this target directory instead of <project>/target",
};
const VERSION: Flag = Flag {
    name: "--version",
    takes_value: true,
    value: "<ver>",
    one_line: "Read this exact version instead of the one Cargo.lock chooses",
};
const REGISTRY: Flag = Flag {
    name: "--registry",
    takes_value: true,
    value: "<dir>",
    one_line: "Pick one registry directory when a version is extracted from more than one",
};
const BUILD: Flag = Flag {
    name: "--build",
    takes_value: false,
    value: "",
    one_line: "Read build-script output under target/…/build/<crate>-*/out instead of the source",
};
const BUILD_DIR: Flag = Flag {
    name: "--build-dir",
    takes_value: true,
    value: "<path>",
    one_line: "Use this build directory (or its out/) when --build finds more than one",
};
const DEPTH: Flag = Flag {
    name: "--depth",
    takes_value: true,
    value: "N",
    one_line: "Directory levels to list; default 2, ceiling 6",
};
const SECRETS: Flag = Flag {
    name: "--include-secret-paths",
    takes_value: false,
    value: "",
    one_line: "Read a path the deny-list refused; the refusal names the rule it matched",
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
const HOW: Flag = Flag {
    name: "--help",
    takes_value: false,
    value: "",
    one_line: "This verb's flags (`-h` is the same flag)",
};

pub static VERBS: &[Verb] = &[
    Verb {
        name: "where",
        question: "Where is this crate installed",
        usage: "at-crate where <crate> [--version <ver>] [--project <path>] [--cargo-home <path>] \
                [--target-dir <path>]",
        notes: "Resolves the crate the project's nearest Cargo.lock names, then the extracted \
                registry copy of exactly that version, or the matching git checkout under \
                $CARGO_HOME/git/checkouts. With no lock entry it falls back to the installed \
                copies only when there is exactly one, and says so; two installed versions are an \
                error naming both, never a guess. Prints the source directory and any build-script \
                output directories. It never runs cargo, so it cannot fetch an uninstalled crate.",
        flags: &[
            LIMIT, CARGO_HOME, PROJECT, VERSION, REGISTRY, TARGET_DIR, HOW,
        ],
    },
    Verb {
        name: "installed",
        question: "Which crates and versions are extracted here",
        usage: "at-crate installed [<crate>] [--limit N] [--cargo-home <path>]",
        notes: "One line per extracted registry copy: name, version, registry directory and path, \
                newest version first. With a crate name, every installed version of it. Bounded \
                and counted, so a listing cannot read as the whole store when it is not.",
        flags: &[LIMIT, CARGO_HOME, HOW],
    },
    Verb {
        name: "files",
        question: "What is in the crate",
        usage: "at-crate files <crate>[/<subpath>] [--depth N] [--version <ver>]",
        notes: "A sorted tree of the crate, two levels deep by default. Files carry their size; \
                directories are not descended past `--depth`. Secret-shaped names are skipped and \
                counted unless `--include-secret-paths` is given.",
        flags: &[
            DEPTH, LIMIT, CARGO_HOME, PROJECT, VERSION, REGISTRY, SECRETS, HOW,
        ],
    },
    Verb {
        name: "slice",
        question: "These exact lines",
        usage: "at-crate slice <crate>/<path>:<range> [<target>…] [--build]",
        notes:
            "The same range syntax as at-peek: 1-based and inclusive, `40+20` means twenty lines \
                from 40, and no range reads the whole file. The first path component is the crate; \
                the rest is relative to its root and cannot escape it. `--build` reads the same \
                relative path under the crate's build-script `out` directory instead.",
        flags: &[
            BUILD, BUILD_DIR, TARGET_DIR, LIMIT, CARGO_HOME, PROJECT, VERSION, REGISTRY, SECRETS,
            HOW,
        ],
    },
    Verb {
        name: "search",
        question: "Every mention",
        usage: "at-crate search <pattern> <crate>[/<subpath>] [<crate>…] [--count | --files-only]",
        notes: "Regex, walked in path order. A bare crate name searches its whole source tree; a \
                crate-relative path narrows it. `--count` and `--files-only` are alternatives, not \
                options to combine; `--summary` is the coarser answer again. `--build` searches \
                build-script output, and rows carry the build directory when there is more than \
                one.",
        flags: &[
            COUNT, FILES_ONLY, SUMMARY, BUILD, BUILD_DIR, TARGET_DIR, LIMIT, MAX_FILES, CARGO_HOME,
            PROJECT, VERSION, REGISTRY, SECRETS, HOW,
        ],
    },
    Verb {
        name: "out",
        question: "What did its build script generate",
        usage: "at-crate out [<crate>] [--project <path>] [--target-dir <path>] [--limit N]",
        notes:
            "Lists `target/**/build/<crate>-<hash>/out` directories and the files in each. With \
                no crate, every build-script output in the target directory. It reads what a past \
                build left behind; it never runs one, and a stale directory is reported as a \
                directory rather than as the current build.",
        flags: &[LIMIT, PROJECT, TARGET_DIR, HOW],
    },
];

static SPEC: Tool = Tool {
    name: crate::TOOL,
    version: crate::VERSION,
    about: "read-only inspection of installed crate source and build output",
    promise:
        "No subprocess, no network, no writes. Reads the cargo home named by --cargo-home, \
              CARGO_HOME or ~/.cargo; the project's Cargo.lock; and the project's target directory.",
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
