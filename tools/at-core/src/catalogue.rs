//! The grammar's vocabulary, shared by every `at-*` binary.
//!
//! A tool supplies its verb table; this supplies the vocabulary that table is written in, the flag
//! list the parser will accept, and the help text that advertises exactly what the parser accepts.
//! One implementation, because "the catalogue cannot drift from the grammar" has to hold in every
//! tool rather than just the first one — and because a parser duplicated into a second binary is a
//! second closed grammar, which is not a closed grammar.

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

/// A binary's identity and its verbs — everything the help text needs.
pub struct Tool {
    pub name: &'static str,
    pub version: &'static str,
    /// One line, under the name in `help`.
    pub about: &'static str,
    /// What this binary does *not* do, stated per tool because it differs: `at-peek` never spawns
    /// anything, `at-recall` spawns git and nothing else.
    pub promise: &'static str,
    pub verbs: &'static [Verb],
}

/// The frame of an answer without its body.
///
/// Shared rather than declared per tool, because it means the same thing everywhere it is offered:
/// the header, the counts, the bounds and the exits, and none of the rows. It exists for the survey
/// — several questions asked in a row, where the shape of each answer is what is wanted and the
/// rows are not — which is otherwise assembled with `; echo "=== … ==="` and `| tail -3`, a tag the
/// tool already prints and a bound hidden in a pipe.
///
/// A verb whose rows *are* its answer does not list the flag. That is the grammar saying so, rather
/// than a mode that prints nothing.
pub const SUMMARY: Flag = Flag {
    name: "--summary",
    takes_value: false,
    value: "",
    one_line: "The frame without the rows: what was read, what was cut, what to ask next",
};

pub fn find(verbs: &'static [Verb], name: &str) -> Option<&'static Verb> {
    verbs.iter().find(|verb| verb.name == name)
}

pub fn names(verbs: &'static [Verb]) -> String {
    verbs
        .iter()
        .map(|verb| verb.name)
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn flag_names(verb: &Verb) -> String {
    let mut names: Vec<String> = verb
        .flags
        .iter()
        .map(|flag| flag.name.to_string())
        .collect();
    names.push("-h".to_string());
    names.join(", ")
}

/// The exit codes, in one place so every tool's help and `at-describe` describe them identically.
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

/// One line per verb, then its usage — the catalogue at the resolution an agent needs to pick a
/// verb rather than to run one.
pub fn verb_lines(verbs: &'static [Verb]) -> Vec<String> {
    let mut lines = Vec::new();
    for verb in verbs {
        lines.push(format!("  {:<7} {}", verb.name, verb.question));
        lines.push(format!("          {}", verb.usage));
    }
    lines
}

pub fn help_all(tool: &Tool) -> String {
    let mut out = format!(
        "{} {} — {}\n{}\n\n",
        tool.name, tool.version, tool.about, tool.promise
    );
    for line in verb_lines(tool.verbs) {
        out.push_str(&line);
        out.push('\n');
    }
    out.push_str("\nExit codes\n");
    for (code, meaning) in exit_codes() {
        out.push_str(&format!("  {code:<6} {meaning}\n"));
    }
    out.push_str("\nA `# ` line in the output is metadata: what was read, and what was not.\n");
    out.push_str(&format!(
        "Run `at-describe` for the toolkit, `{} help <verb>` for one verb.\n",
        tool.name
    ));
    out
}

pub fn help_for(tool: &Tool, verb: &Verb) -> String {
    let mut out = format!("{} {}\n\n  {}\n\n", tool.name, verb.name, verb.question);
    out.push_str(&format!("Usage\n  {}\n\n", verb.usage));
    out.push_str(&format!("{}\n\n", wrap(verb.notes, 76)));
    out.push_str("Flags\n");
    for flag in verb.flags {
        out.push_str(&flag_line(flag));
    }
    // `-h` is an alias rather than a table entry, so it is listed once and aligned with the rest.
    out.push_str(&format!("  {:<28} {}\n", "-h", "Same as --help"));
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
