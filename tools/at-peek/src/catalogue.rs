//! The verb table — one source for three consumers: the CLI parser (which will not accept a
//! flag that is not here), the help text, and the `at-describe` binary. A flag missing from
//! this table cannot be passed, so the catalogue cannot drift from what the tool accepts, and
//! `at-describe` cannot advertise a capability the binary does not have.

pub struct Flag {
    /// The token as it must be typed.
    pub name: &'static str,
    pub takes_value: bool,
    /// The value placeholder for help, empty when the flag takes none.
    pub value: &'static str,
    pub one_line: &'static str,
}

pub struct Verb {
    pub name: &'static str,
    /// The question this verb answers, in the words a caller would ask it.
    pub question: &'static str,
    /// The canonical one-line invocation.
    pub usage: &'static str,
    /// What the answer counts, what it refuses, and what it does not do yet.
    pub notes: &'static str,
    pub flags: &'static [Flag],
}

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
const HELP: Flag = Flag {
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

const VERB_FLAGS: &[Flag] = &[LIMIT, ROOT, SECRETS, HELP];

pub static VERBS: &[Verb] = &[
    Verb {
        name: "stat",
        question: "How big is it, when did it change",
        usage: "at-peek stat <path>… [--limit N] [--root <path>] [--include-secret-paths]",
        notes: "Lines, bytes, modification time and language per path. The \"did the write \
                land\" verb. A binary is reported as binary rather than counted.",
        flags: VERB_FLAGS,
    },
    Verb {
        name: "slice",
        question: "These exact lines",
        usage: "at-peek slice <path>:40-60 [<path>:<range>…] [--limit N] [--root <path>]",
        notes: "Ranges are 1-based and inclusive; `40+20` means twenty lines from 40, and no \
                range reads the whole file. `--limit` is shared across targets, so a batch \
                cannot print more than one verb's worth. Any target that fails fails the \
                invocation.",
        flags: VERB_FLAGS,
    },
    Verb {
        name: "search",
        question: "Every mention",
        usage: "at-peek search <pattern> [path…] [--count | --files-only] [--limit N] [--max-files N]",
        notes: "Regex, walked in path order; no path searches the whole root. Matches are counted over every file the walk considers, so the total under the listing is the real one. Directory names skipped by rule are named in the output — real gitignore semantics are not here yet. `--count` and `--files-only` are alternatives, not options to combine.",
        flags: &[COUNT, FILES_ONLY, LIMIT, MAX_FILES, ROOT, SECRETS, HELP],
    },
];

pub fn find(name: &str) -> Option<&'static Verb> {
    VERBS.iter().find(|v| v.name == name)
}

pub fn names() -> String {
    VERBS.iter().map(|v| v.name).collect::<Vec<_>>().join(", ")
}

pub fn flag_names(verb: &Verb) -> String {
    let mut names: Vec<String> = verb.flags.iter().map(|f| f.name.to_string()).collect();
    names.push("-h".to_string());
    names.join(", ")
}

/// The exit codes, in one place so `at-peek help` and `at-describe` cannot disagree.
pub fn exit_codes() -> [(&'static str, &'static str); 5] {
    [
        ("0", "something was found and printed"),
        ("1", "read successfully, nothing to show"),
        ("2", "the command is not one this tool accepts"),
        (
            "3",
            "the request was fine, the environment could not satisfy it",
        ),
        (
            "4",
            "refused: a path outside the root, or a secret-shaped path",
        ),
    ]
}

pub fn help_all() -> String {
    let mut out = format!(
        "{TOOL} {version} — read-only inspection of the working tree.\n\
         No git, no subprocess, no writes, no network, no environment configuration.\n\n",
        TOOL = crate::TOOL,
        version = crate::VERSION,
    );
    for line in verb_lines() {
        out.push_str(&line);
        out.push('\n');
    }
    out.push_str("\nFlags common to every verb\n");
    for flag in VERB_FLAGS {
        out.push_str(&flag_line(flag));
    }
    out.push_str("  -h      Same as --help\n");
    out.push_str("\nExit codes\n");
    for (code, meaning) in exit_codes() {
        out.push_str(&format!("  {code:<6} {meaning}\n"));
    }
    out.push_str("\nA `# ` line in the output is metadata: what was read, and what was not.\n");
    out.push_str("Run `at-describe` for the toolkit, `at-peek help <verb>` for one verb.\n");
    out
}

/// One line per verb, then its usage — the catalogue at the resolution an agent needs to pick
/// a verb rather than to run one.
pub fn verb_lines() -> Vec<String> {
    let mut lines = Vec::new();
    for verb in VERBS {
        lines.push(format!("  {:<7} {}", verb.name, verb.question));
        lines.push(format!("          {}", verb.usage));
    }
    lines
}

pub fn help_for(verb: &Verb) -> String {
    let mut out = format!("{} {}\n\n  {}\n\n", crate::TOOL, verb.name, verb.question);
    out.push_str(&format!("Usage\n  {}\n\n", verb.usage));
    out.push_str(&format!("{}\n\n", wrap(verb.notes, 76)));
    out.push_str("Flags\n");
    for flag in verb.flags {
        out.push_str(&flag_line(flag));
    }
    out.push_str("  -h      Same as --help\n");
    out
}

fn flag_line(flag: &Flag) -> String {
    let head = if flag.takes_value {
        format!("{} {}", flag.name, flag.value)
    } else {
        flag.name.to_string()
    };
    format!("  {head:<28} {}\n", flag.one_line)
}

/// Wrap to a width, because a help screen an agent reads has to fit one.
fn wrap(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut column = 0;
    for word in text.split_whitespace() {
        if column > 0 && column + 1 + word.len() > width {
            out.push('\n');
            column = 0;
        } else if column > 0 {
            out.push(' ');
            column += 1;
        }
        out.push_str(word);
        column += word.len();
    }
    out
}
