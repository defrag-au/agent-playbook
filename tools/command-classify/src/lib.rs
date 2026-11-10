//! Classify a shell command line: what it runs, and whether any part of it can write.
//!
//! The grammar is not this crate's. [`brush_parser`] tokenizes and parses POSIX/bash syntax into
//! an AST — quoting, operators, pipelines, redirections, substitutions, loops and functions. What
//! this crate adds is the part a grammar cannot know: **what a program does**. It walks the AST
//! and consults a small table of programs.
//!
//! The table is deliberately closed. A line is [`Effect::Read`] only when every leaf is a program
//! known not to write. A program the table does not know is [`Effect::Unknown`], never `Read`, and
//! a line the parser refuses is `Unknown` too. So a caller may treat `Read` as permission and must
//! not treat anything else as one.
//!
//! [`Effect::Run`] is the middle: a program known to execute rather than read — a build, a test, an
//! interpreter on a file. It is not a promise that nothing is written; a build writes `target/`,
//! and `cargo run` runs arbitrary code. A caller that allows `Run` owns the confinement for it,
//! which is why this module does not decide policy.
//!
//! ```
//! use command_classify::{classify, Effect};
//!
//! assert_eq!(classify("ls && rg render_claim src").effect, Effect::Read);
//! assert_eq!(classify("sed -n '1,20p' src/lib.rs").effect, Effect::Read);
//! assert_eq!(classify("sed -i 's/a/b/' src/lib.rs").effect, Effect::Write);
//! assert_eq!(classify("cargo test | grep FAIL").effect, Effect::Run);
//! assert_eq!(classify("some-unknown-tool --go").effect, Effect::Unknown);
//! ```

use std::io::Cursor;

use brush_parser::ast::{
    AndOr, AndOrList, Command, CommandPrefixOrSuffixItem, CompoundCommand, CompoundList,
    IoFileRedirectKind, IoFileRedirectTarget, IoRedirect, Pipeline, RedirectList, SimpleCommand,
    Word,
};
use brush_parser::word::{self, WordPiece};
use brush_parser::{unquote_str, Parser, ParserOptions};

/// What one command line does, taken as a whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Analysis {
    /// Every simple command in the line, in the order the parser meets it.
    pub segments: Vec<Segment>,
    /// The worst effect of any leaf: `Read` only when every leaf is known not to write.
    pub effect: Effect,
    /// How the line writes, when it does.
    pub writes: Writes,
    /// Set when the parser refused the line. [`Analysis::effect`] is then [`Effect::Unknown`].
    pub parse_error: Option<String>,
}

/// One simple command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// The program that will run, after the wrappers this crate understands are peeled away.
    pub program: String,
    /// Its arguments as written.
    pub args: Vec<String>,
    /// What this segment does.
    pub effect: Effect,
}

/// What a command can do to the working tree, worst last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Effect {
    /// Cannot write: every program is known to read, or only prints.
    Read,
    /// Runs a program, such as a build, a test or an interpreter on a file. The caller confines
    /// it; `Run` is not a promise that nothing is written.
    Run,
    /// Not understood. A caller must treat it as unsafe.
    Unknown,
    /// Writes the working tree: a redirect, a mutating program, or a git command that changes it.
    Write,
}

/// How a command writes, when it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Writes {
    /// Nothing is written.
    None,
    /// Only a build or test's output is captured to a scratch file (`cargo test > /tmp/log`).
    Capture,
    /// Anything else: the working tree, a path whose scope is not known, a content writer's
    /// output, or a write that came from a substitution.
    Other,
}

/// Where a redirection writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum WriteScope {
    /// A temporary location: `/tmp`, `/var/tmp`, `/var/folders`, or a path under `$TMPDIR`.
    Scratch,
    /// Everywhere else, including the working tree and any path whose scope is not known.
    Elsewhere,
}

/// Classify `command`, which is one line of shell as a model wrote it.
///
/// `&&`, `||`, `;`, `|`, `&`, newlines, subshells, brace groups, loops and functions are the
/// parser's; `>`, `>>`, `>&` and `&>` are redirections, and a write to `/dev/null` is not a write.
/// The wrappers that only choose how a command runs — `env`, `sudo`, `timeout`, `nice`, `xargs`,
/// `direnv exec`, `nix … -c`, `sh -c` — are peeled, and a nested command line is classified in
/// turn. A command substitution is classified by the command it runs.
///
/// ```
/// use command_classify::classify;
///
/// let analysis = classify("git status && cat src/lib.rs");
/// assert_eq!(analysis.segments.len(), 2);
/// assert_eq!(analysis.segments[0].program, "git");
/// assert_eq!(analysis.segments[1].program, "cat");
/// ```
pub fn classify(command: &str) -> Analysis {
    let options = ParserOptions::default();
    let parsed = {
        let mut parser = Parser::new(Cursor::new(command), &options);
        parser.parse_program()
    };

    match parsed {
        Ok(program) => {
            let mut walk = Walk {
                options: &options,
                segments: Vec::new(),
                writes: Writes::None,
            };
            let effect = program
                .complete_commands
                .iter()
                .map(|list| walk.list(list))
                .max()
                .unwrap_or(Effect::Read);
            Analysis {
                segments: walk.segments,
                effect,
                writes: walk.writes,
                parse_error: None,
            }
        }
        Err(error) => Analysis {
            segments: Vec::new(),
            effect: Effect::Unknown,
            writes: Writes::None,
            parse_error: Some(error.to_string()),
        },
    }
}

struct Walk<'a> {
    options: &'a ParserOptions,
    segments: Vec<Segment>,
    writes: Writes,
}

impl Walk<'_> {
    fn list(&mut self, list: &CompoundList) -> Effect {
        list.0
            .iter()
            .map(|item| self.and_or_list(&item.0))
            .max()
            .unwrap_or(Effect::Read)
    }

    fn and_or_list(&mut self, list: &AndOrList) -> Effect {
        let mut effect = self.pipeline(&list.first);
        for item in &list.additional {
            match item {
                AndOr::And(pipeline) | AndOr::Or(pipeline) => {
                    effect = effect.max(self.pipeline(pipeline));
                }
            }
        }
        effect
    }

    fn pipeline(&mut self, pipeline: &Pipeline) -> Effect {
        pipeline
            .seq
            .iter()
            .map(|command| self.command(command))
            .max()
            .unwrap_or(Effect::Read)
    }

    fn command(&mut self, command: &Command) -> Effect {
        match command {
            Command::Simple(simple) => self.simple(simple),
            Command::Compound(compound, redirects) => {
                let body = self.compound(compound);
                let redirected = redirects
                    .as_ref()
                    .map(|redirects| self.redirects(redirects))
                    .unwrap_or_else(Redirected::none);
                let effect = redirected.effect.max(body);
                if effect == Effect::Write {
                    self.observe_redirect(&redirected, body, false);
                }
                effect
            }
            // A definition writes the shell's function table, not the working tree. A later call
            // is a simple command whose program is not in the table, so it is already `Unknown`.
            Command::Function(_) => Effect::Read,
            Command::ExtendedTest(_, redirects) => {
                let redirected = redirects
                    .as_ref()
                    .map(|redirects| self.redirects(redirects))
                    .unwrap_or_else(Redirected::none);
                if redirected.effect == Effect::Write {
                    self.observe_redirect(&redirected, Effect::Read, false);
                }
                redirected.effect
            }
        }
    }

    fn compound(&mut self, command: &CompoundCommand) -> Effect {
        match command {
            CompoundCommand::Arithmetic(_) => Effect::Read,
            CompoundCommand::ArithmeticForClause(clause) => self.list(&clause.body.list),
            CompoundCommand::BraceGroup(group) => self.list(&group.list),
            CompoundCommand::Subshell(subshell) => self.list(&subshell.list),
            CompoundCommand::ForClause(clause) => {
                let values = clause
                    .values
                    .as_ref()
                    .map(|values| {
                        values
                            .iter()
                            .map(|value| self.word(value).effect)
                            .max()
                            .unwrap_or(Effect::Read)
                    })
                    .unwrap_or(Effect::Read);
                values.max(self.list(&clause.body.list))
            }
            CompoundCommand::CaseClause(clause) => {
                let value = self.word(&clause.value).effect;
                let cases = clause
                    .cases
                    .iter()
                    .filter_map(|case| case.cmd.as_ref())
                    .map(|body| self.list(body))
                    .max()
                    .unwrap_or(Effect::Read);
                value.max(cases)
            }
            CompoundCommand::IfClause(clause) => {
                let mut effect = self.list(&clause.condition).max(self.list(&clause.then));
                if let Some(elses) = &clause.elses {
                    for else_clause in elses {
                        if let Some(condition) = &else_clause.condition {
                            effect = effect.max(self.list(condition));
                        }
                        effect = effect.max(self.list(&else_clause.body));
                    }
                }
                effect
            }
            CompoundCommand::WhileClause(clause) | CompoundCommand::UntilClause(clause) => {
                self.list(&clause.0).max(self.list(&clause.1.list))
            }
            CompoundCommand::Coprocess(coprocess) => self.command(&coprocess.body),
        }
    }

    fn simple(&mut self, command: &SimpleCommand) -> Effect {
        let mut effect = Effect::Read;
        let mut write = None;
        let mut other_write = false;

        if let Some(prefix) = &command.prefix {
            for item in &prefix.0 {
                match item {
                    CommandPrefixOrSuffixItem::Word(word)
                    | CommandPrefixOrSuffixItem::AssignmentWord(_, word) => {
                        let value = self.word(word);
                        effect = effect.max(value.effect);
                        other_write |= value.effect == Effect::Write;
                    }
                    CommandPrefixOrSuffixItem::IoRedirect(redirect) => {
                        let redirected = self.redirect(redirect);
                        effect = effect.max(redirected.effect);
                        write = write.max(redirected.write);
                    }
                    CommandPrefixOrSuffixItem::ProcessSubstitution(_, subshell) => {
                        let inner = self.list(&subshell.list);
                        effect = effect.max(inner);
                        other_write |= inner == Effect::Write;
                    }
                }
            }
        }

        let mut args = Vec::new();
        let mut dynamic_args = Vec::new();
        if let Some(suffix) = &command.suffix {
            for item in &suffix.0 {
                match item {
                    CommandPrefixOrSuffixItem::Word(word)
                    | CommandPrefixOrSuffixItem::AssignmentWord(_, word) => {
                        let value = self.word(word);
                        effect = effect.max(value.effect);
                        other_write |= value.effect == Effect::Write;
                        dynamic_args.push(value.text.is_none());
                        args.push(value.text.unwrap_or_else(|| unquote_str(&word.value)));
                    }
                    CommandPrefixOrSuffixItem::IoRedirect(redirect) => {
                        let redirected = self.redirect(redirect);
                        effect = effect.max(redirected.effect);
                        write = write.max(redirected.write);
                    }
                    CommandPrefixOrSuffixItem::ProcessSubstitution(_, subshell) => {
                        let inner = self.list(&subshell.list);
                        effect = effect.max(inner);
                        other_write |= inner == Effect::Write;
                    }
                }
            }
        }

        // An assignment or a bare redirection: nothing is executed, but a redirect can still write.
        let Some(program_word) = &command.word_or_name else {
            if effect == Effect::Write {
                self.observe_redirect(&Redirected { effect, write }, Effect::Read, other_write);
                self.segments.push(Segment {
                    program: ">".to_owned(),
                    args: Vec::new(),
                    effect,
                });
            }
            return effect;
        };

        let program_value = self.word(program_word);
        effect = effect.max(program_value.effect);
        other_write |= program_value.effect == Effect::Write;
        let program = program_value
            .text
            .unwrap_or_else(|| unquote_str(&program_word.value));

        let (program, table) = command_effect(&program, &args);
        let table = if decides_by_flags(&program, &args, &dynamic_args) {
            Effect::Unknown
        } else {
            table
        };
        let effect = effect.max(table);

        if effect == Effect::Write {
            self.observe_redirect(&Redirected { effect, write }, table, other_write);
        }

        self.segments.push(Segment {
            program,
            args,
            effect,
        });

        effect
    }

    fn redirects(&mut self, redirects: &RedirectList) -> Redirected {
        let mut result = Redirected::none();
        for redirect in &redirects.0 {
            let one = self.redirect(redirect);
            result.effect = result.effect.max(one.effect);
            result.write = result.write.max(one.write);
        }
        result
    }

    fn redirect(&mut self, redirect: &IoRedirect) -> Redirected {
        match redirect {
            IoRedirect::File(_, kind, target) => match kind {
                IoFileRedirectKind::Read => Redirected {
                    effect: self.redirect_target(target),
                    write: None,
                },
                IoFileRedirectKind::DuplicateInput | IoFileRedirectKind::DuplicateOutput => {
                    if duplicates_fd(target) {
                        Redirected {
                            effect: self.redirect_target(target),
                            write: None,
                        }
                    } else {
                        Redirected {
                            effect: Effect::Write,
                            write: Some(write_scope(target)),
                        }
                    }
                }
                IoFileRedirectKind::Write
                | IoFileRedirectKind::Append
                | IoFileRedirectKind::ReadAndWrite
                | IoFileRedirectKind::Clobber => {
                    if writes_to_file(target) {
                        Redirected {
                            effect: Effect::Write,
                            write: Some(write_scope(target)),
                        }
                    } else {
                        Redirected {
                            effect: self.redirect_target(target),
                            write: None,
                        }
                    }
                }
            },
            IoRedirect::HereDocument(_, document) => Redirected {
                effect: self.word(&document.doc).effect,
                write: None,
            },
            IoRedirect::HereString(_, word) => Redirected {
                effect: self.word(word).effect,
                write: None,
            },
            IoRedirect::OutputAndError(word, _) => {
                if is_device_sink(&unquote_str(&word.value)) {
                    Redirected {
                        effect: self.word(word).effect,
                        write: None,
                    }
                } else {
                    Redirected {
                        effect: Effect::Write,
                        write: Some(scope_of(&unquote_str(&word.value))),
                    }
                }
            }
        }
    }

    fn redirect_target(&mut self, target: &IoFileRedirectTarget) -> Effect {
        match target {
            IoFileRedirectTarget::Filename(word) => self.word(word).effect,
            IoFileRedirectTarget::Fd(_) => Effect::Read,
            IoFileRedirectTarget::Duplicate(word) => self.word(word).effect,
            IoFileRedirectTarget::ProcessSubstitution(_, subshell) => self.list(&subshell.list),
        }
    }

    /// Record what a redirection writes, given the effect of the command it captures.
    ///
    /// A scratch redirect on a build or a test is output capture: `cargo test > /tmp/log`. Anything
    /// else is [`Writes::Other`] — a working-tree target, a content writer such as `echo` or `cat`
    /// writing a script to `/tmp`, or a write that came from a substitution rather than the
    /// redirect.
    fn observe_redirect(&mut self, redirected: &Redirected, captured: Effect, other_write: bool) {
        let capture = !other_write
            && redirected.write == Some(WriteScope::Scratch)
            && captured == Effect::Run;
        if capture {
            if self.writes == Writes::None {
                self.writes = Writes::Capture;
            }
        } else {
            self.writes = Writes::Other;
        }
    }

    /// What a word expands to, when it can be known without running anything.
    fn word(&self, word: &Word) -> WordValue {
        match word::parse(&word.value, self.options) {
            Ok(pieces) => {
                let mut text = String::new();
                let mut literal = true;
                let mut effect = Effect::Read;
                for piece in &pieces {
                    self.piece(&piece.piece, &mut text, &mut literal, &mut effect);
                }
                WordValue {
                    text: literal.then_some(text),
                    effect,
                }
            }
            // A word the expansion parser cannot read is one whose value is not known.
            Err(_) => WordValue {
                text: None,
                effect: Effect::Unknown,
            },
        }
    }

    fn piece(&self, piece: &WordPiece, text: &mut String, literal: &mut bool, effect: &mut Effect) {
        match piece {
            WordPiece::Text(value)
            | WordPiece::SingleQuotedText(value)
            | WordPiece::AnsiCQuotedText(value)
            | WordPiece::EscapeSequence(value) => text.push_str(value),
            WordPiece::DoubleQuotedSequence(pieces)
            | WordPiece::GettextDoubleQuotedSequence(pieces) => {
                for piece in pieces {
                    self.piece(&piece.piece, text, literal, effect);
                }
            }
            WordPiece::TildeExpansion(_) => text.push('~'),
            WordPiece::ParameterExpansion(_) | WordPiece::ArithmeticExpression(_) => {
                *literal = false;
            }
            WordPiece::CommandSubstitution(inner)
            | WordPiece::BackquotedCommandSubstitution(inner) => {
                *literal = false;
                *effect = (*effect).max(classify(inner).effect);
            }
        }
    }
}

/// A word's text when it is fully literal, and the commands its substitutions run.
struct WordValue {
    /// `None` when the word expands at run time.
    text: Option<String>,
    /// The worst effect of any command substitution inside it.
    effect: Effect,
}

/// What one redirection does, and where it writes when it does.
struct Redirected {
    effect: Effect,
    write: Option<WriteScope>,
}

impl Redirected {
    fn none() -> Self {
        Self {
            effect: Effect::Read,
            write: None,
        }
    }
}

fn write_scope(target: &IoFileRedirectTarget) -> WriteScope {
    match target {
        IoFileRedirectTarget::Filename(word) => scope_of(&unquote_str(&word.value)),
        IoFileRedirectTarget::Duplicate(word) => scope_of(&unquote_str(&word.value)),
        IoFileRedirectTarget::Fd(_) | IoFileRedirectTarget::ProcessSubstitution(_, _) => {
            WriteScope::Elsewhere
        }
    }
}

fn scope_of(path: &str) -> WriteScope {
    if path == "/tmp"
        || path.starts_with("/tmp/")
        || path.starts_with("/var/tmp/")
        || path.starts_with("/var/folders/")
        || path.starts_with("$TMPDIR/")
        || path.starts_with("${TMPDIR}/")
    {
        WriteScope::Scratch
    } else {
        WriteScope::Elsewhere
    }
}

fn writes_to_file(target: &IoFileRedirectTarget) -> bool {
    match target {
        IoFileRedirectTarget::Filename(word) => !is_device_sink(&unquote_str(&word.value)),
        IoFileRedirectTarget::Fd(_) => false,
        IoFileRedirectTarget::Duplicate(word) => !is_fd_word(&unquote_str(&word.value)),
        IoFileRedirectTarget::ProcessSubstitution(_, _) => false,
    }
}

fn duplicates_fd(target: &IoFileRedirectTarget) -> bool {
    match target {
        IoFileRedirectTarget::Fd(_) => true,
        IoFileRedirectTarget::Duplicate(word) => is_fd_word(&unquote_str(&word.value)),
        _ => false,
    }
}

fn is_fd_word(text: &str) -> bool {
    !text.is_empty() && (text == "-" || text.chars().all(|character| character.is_ascii_digit()))
}

fn is_device_sink(text: &str) -> bool {
    matches!(
        text,
        "/dev/null" | "/dev/stdout" | "/dev/stderr" | "/dev/fd/1" | "/dev/fd/2"
    )
}

/// What a command runs, after the wrappers that only choose how it runs are peeled away.
enum Peeling {
    Command {
        program: String,
        args: Vec<String>,
    },
    /// A command line a wrapper hands to a shell (`sh -c`, `nix … -c`).
    Nested(String),
}

fn command_effect(program: &str, args: &[String]) -> (String, Effect) {
    match peel(program, args) {
        Peeling::Nested(inner) => (program.to_owned(), classify(&inner).effect),
        Peeling::Command { program, args } => {
            let effect = effect_of(&program, &args);
            (program, effect)
        }
    }
}

fn peel(program: &str, args: &[String]) -> Peeling {
    let mut program = program.to_owned();
    let mut args = args.to_vec();

    loop {
        let name = basename(&program).to_owned();

        match name.as_str() {
            "env" => {
                skip_env_args(&mut args);
                if args.is_empty() {
                    return Peeling::Command { program, args };
                }
            }
            "sudo" | "timeout" | "nice" | "nohup" | "command" | "builtin" | "exec" => {
                skip_wrapper_args(name.as_str(), &mut args);
                if args.is_empty() {
                    return Peeling::Command { program, args };
                }
            }
            "xargs" => {
                skip_wrapper_args("xargs", &mut args);
                if args.is_empty() {
                    return Peeling::Command { program, args };
                }
            }
            "nix" => {
                return match args
                    .iter()
                    .position(|arg| arg == "-c" || arg == "--command")
                {
                    Some(position) => Peeling::Nested(args[position + 1..].join(" ")),
                    None => Peeling::Command { program, args },
                };
            }
            "direnv" if args.first().map(String::as_str) == Some("exec") => {
                args.remove(0);
                if !args.is_empty() {
                    args.remove(0);
                }
                if args.is_empty() {
                    return Peeling::Command { program, args };
                }
            }
            "sh" | "bash" | "zsh" | "dash" | "fish" | "ksh" => {
                let Some(position) = args.iter().position(|arg| is_command_flag(arg)) else {
                    return Peeling::Command { program, args };
                };
                let Some(nested) = args.get(position + 1) else {
                    return Peeling::Command { program, args };
                };
                return Peeling::Nested(nested.clone());
            }
            _ => return Peeling::Command { program, args },
        }

        program = args.remove(0);
    }
}

fn skip_env_args(words: &mut Vec<String>) {
    loop {
        let skip = words
            .first()
            .is_some_and(|word| word.starts_with('-') || is_assignment(word));
        if !skip {
            break;
        }
        words.remove(0);
    }
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
            && name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
    })
}

fn skip_wrapper_args(name: &str, words: &mut Vec<String>) {
    match name {
        "timeout" => {
            while words.first().is_some_and(|word| word.starts_with('-')) {
                words.remove(0);
            }
            if !words.is_empty() {
                words.remove(0);
            }
        }
        "nice" => {
            while let Some(word) = words.first() {
                if !word.starts_with('-') {
                    break;
                }
                let flag = words.remove(0);
                if (flag == "-n" || flag == "--adjustment") && !words.is_empty() {
                    words.remove(0);
                }
            }
        }
        "sudo" => {
            while let Some(word) = words.first() {
                if !word.starts_with('-') {
                    break;
                }
                let flag = words.remove(0);
                if matches!(
                    flag.as_str(),
                    "-u" | "-g" | "-p" | "-C" | "-h" | "--user" | "--group"
                ) && !words.is_empty()
                {
                    words.remove(0);
                }
            }
        }
        "xargs" => {
            while let Some(word) = words.first() {
                if !word.starts_with('-') {
                    break;
                }
                let flag = words.remove(0);
                if matches!(
                    flag.as_str(),
                    "-n" | "-I" | "-P" | "-d" | "-s" | "-L" | "-a" | "-E"
                ) && !words.is_empty()
                {
                    words.remove(0);
                }
            }
        }
        _ => {
            while words.first().is_some_and(|word| word.starts_with('-')) {
                words.remove(0);
            }
        }
    }
}

fn is_command_flag(word: &str) -> bool {
    if word == "--command" {
        return true;
    }
    let Some(letters) = word.strip_prefix('-') else {
        return false;
    };
    !letters.is_empty()
        && letters.chars().all(|letter| "cefilx".contains(letter))
        && letters.contains('c')
}

/// Whether an argument that expands at run time could change what this program does.
///
/// `git` and `cargo` decide by their subcommand, so only a dynamic subcommand matters; a dynamic
/// later argument is a commit, a path or a test filter. The rest decide by a flag, and a dynamic
/// argument could be that flag (`sed $FLAG`).
fn decides_by_flags(program: &str, args: &[String], dynamic_args: &[bool]) -> bool {
    if !dynamic_args.iter().any(|dynamic| *dynamic) {
        return false;
    }

    match basename(program) {
        "git" | "cargo" => subcommand_is_dynamic(args, dynamic_args),
        "sed" | "tar" | "curl" | "find" | "perl" | "ruby" | "python" | "python3" | "node"
        | "deno" | "bun" | "prettier" | "eslint" => true,
        _ => false,
    }
}

/// Whether the first argument that is not a flag expands at run time.
fn subcommand_is_dynamic(args: &[String], dynamic_args: &[bool]) -> bool {
    let mut index = 0;
    while index < args.len() {
        if matches!(
            args[index].as_str(),
            "-C" | "-c" | "--git-dir" | "--work-tree"
        ) {
            index += 2;
        } else if args[index].starts_with('-') {
            index += 1;
        } else {
            return dynamic_args.get(index).copied().unwrap_or(false);
        }
    }
    false
}

fn effect_of(program: &str, args: &[String]) -> Effect {
    let name = basename(program);

    match name {
        "" => Effect::Unknown,
        "sed" => {
            if has_flag(args, "-i") || has_flag(args, "--in-place") {
                Effect::Write
            } else {
                Effect::Read
            }
        }
        "perl" | "ruby" => {
            if has_flag(args, "-i") {
                Effect::Write
            } else if has_inline_code(args) {
                Effect::Unknown
            } else {
                Effect::Run
            }
        }
        "find" => {
            if args.iter().any(|arg| {
                matches!(
                    arg.as_str(),
                    "-exec" | "-execdir" | "-ok" | "-okdir" | "-delete" | "-fprint" | "-fprintf"
                )
            }) {
                Effect::Write
            } else {
                Effect::Read
            }
        }
        "git" => git_effect(args),
        // `rg --pre` runs a command for every file it searches, so it is not a read of the tree.
        "rg" => {
            if has_flag(args, "--pre") {
                Effect::Unknown
            } else {
                Effect::Read
            }
        }
        "cargo" => match subcommand(args) {
            Some("fmt" | "fix") => Effect::Write,
            _ => Effect::Run,
        },
        "rustfmt" => Effect::Write,
        "prettier" => {
            if has_flag(args, "--write") || has_flag(args, "-w") {
                Effect::Write
            } else {
                Effect::Read
            }
        }
        "eslint" => {
            if has_flag(args, "--fix") {
                Effect::Write
            } else {
                Effect::Run
            }
        }
        "tar" => tar_effect(args),
        "curl" => {
            if has_flag(args, "-o")
                || has_flag(args, "--output")
                || has_flag(args, "-O")
                || has_flag(args, "--remote-name")
            {
                Effect::Write
            } else {
                Effect::Run
            }
        }
        "wget" => Effect::Write,
        "scp" => Effect::Unknown,
        "python" | "python3" | "node" | "deno" | "bun" => {
            if has_inline_code(args) {
                Effect::Unknown
            } else {
                Effect::Run
            }
        }
        name if is_write_program(name) => Effect::Write,
        name if is_read_program(name) => Effect::Read,
        name if is_run_program(name) => Effect::Run,
        _ => Effect::Unknown,
    }
}

fn git_effect(args: &[String]) -> Effect {
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if matches!(arg.as_str(), "-C" | "-c" | "--git-dir" | "--work-tree") {
            index += 2;
        } else if arg.starts_with('-') {
            index += 1;
        } else {
            break;
        }
    }

    let Some(subcommand) = args.get(index).map(String::as_str) else {
        return Effect::Unknown;
    };
    let rest = &args[index + 1..];

    match subcommand {
        "status" | "diff" | "log" | "show" | "blame" | "grep" | "ls-files" | "ls-tree"
        | "cat-file" | "rev-parse" | "rev-list" | "describe" | "shortlog" | "reflog"
        | "for-each-ref" | "show-ref" | "merge-base" | "name-rev" | "whatchanged"
        | "count-objects" | "diff-tree" | "diff-index" | "verify-commit" | "verify-tag"
        | "fsck" | "check-ignore" => Effect::Read,
        "fetch" | "gc" | "maintenance" => Effect::Run,
        "branch" => {
            if rest.iter().any(|arg| {
                matches!(
                    arg.as_str(),
                    "-l" | "--list" | "-v" | "-vv" | "-a" | "-r" | "--show-current"
                )
            }) {
                Effect::Read
            } else {
                Effect::Write
            }
        }
        "tag" => {
            if rest
                .iter()
                .any(|arg| matches!(arg.as_str(), "-l" | "--list" | "-n" | "-v"))
            {
                Effect::Read
            } else {
                Effect::Write
            }
        }
        "remote" => {
            if rest.is_empty()
                || rest
                    .iter()
                    .any(|arg| matches!(arg.as_str(), "-v" | "--verbose" | "show" | "get-url"))
            {
                Effect::Read
            } else {
                Effect::Write
            }
        }
        "config" => {
            if rest.iter().any(|arg| {
                matches!(
                    arg.as_str(),
                    "--get" | "--get-all" | "--list" | "-l" | "--get-regexp" | "--show-origin"
                )
            }) {
                Effect::Read
            } else {
                Effect::Write
            }
        }
        "stash" => match rest.first().map(String::as_str) {
            Some("list" | "show") => Effect::Read,
            _ => Effect::Write,
        },
        "worktree" => {
            if rest.first().map(String::as_str) == Some("list") {
                Effect::Read
            } else {
                Effect::Write
            }
        }
        "submodule" => {
            if rest.first().map(String::as_str) == Some("status") {
                Effect::Read
            } else {
                Effect::Write
            }
        }
        "add" | "commit" | "push" | "pull" | "merge" | "rebase" | "reset" | "checkout"
        | "switch" | "restore" | "clean" | "apply" | "am" | "cherry-pick" | "revert" | "rm"
        | "mv" | "init" | "clone" | "update-index" | "update-ref" | "notes" | "filter-branch"
        | "format-patch" | "archive" | "bisect" => Effect::Write,
        _ => Effect::Unknown,
    }
}

fn tar_effect(args: &[String]) -> Effect {
    let mut writes = false;
    let mut reads = false;
    for arg in args {
        if matches!(
            arg.as_str(),
            "--extract" | "--create" | "--append" | "--update" | "--get"
        ) {
            writes = true;
        } else if arg == "--list" {
            reads = true;
        } else if let Some(letters) = arg.strip_prefix('-') {
            if letters
                .chars()
                .any(|letter| matches!(letter, 'x' | 'c' | 'r' | 'u'))
            {
                writes = true;
            }
            if letters.contains('t') {
                reads = true;
            }
        }
    }

    if writes {
        Effect::Write
    } else if reads {
        Effect::Read
    } else {
        Effect::Unknown
    }
}

fn is_write_program(name: &str) -> bool {
    matches!(
        name,
        "rm" | "rmdir"
            | "mv"
            | "cp"
            | "install"
            | "touch"
            | "truncate"
            | "tee"
            | "dd"
            | "mkdir"
            | "chmod"
            | "chown"
            | "chgrp"
            | "ln"
            | "rsync"
            | "patch"
            | "shred"
            | "zip"
            | "unzip"
            | "gzip"
            | "gunzip"
            | "cpio"
    )
}

fn is_read_program(name: &str) -> bool {
    matches!(
        name,
        "ls" | "pwd"
            | "cat"
            | "bat"
            | "head"
            | "tail"
            | "less"
            | "more"
            | "rg"
            | "grep"
            | "egrep"
            | "fgrep"
            | "ag"
            | "ack"
            | "awk"
            | "gawk"
            | "cut"
            | "sort"
            | "uniq"
            | "tr"
            | "nl"
            | "wc"
            | "stat"
            | "file"
            | "du"
            | "df"
            | "tree"
            | "which"
            | "whereis"
            | "type"
            | "printenv"
            | "id"
            | "whoami"
            | "groups"
            | "uname"
            | "hostname"
            | "date"
            | "cal"
            | "tty"
            | "test"
            | "["
            | "true"
            | "false"
            | ":"
            | "sleep"
            | "echo"
            | "printf"
            | "seq"
            | "basename"
            | "dirname"
            | "realpath"
            | "readlink"
            | "diff"
            | "cmp"
            | "comm"
            | "join"
            | "paste"
            | "column"
            | "jq"
            | "yq"
            | "env"
            | "xargs"
            | "md5sum"
            | "shasum"
            | "sha256sum"
            | "man"
            | "help"
            | "cd"
            | "pushd"
            | "popd"
            // The toolkit's own binaries, listed exactly. A prefix rule (`at-*`) would bless any
            // executable that chose the name; these are the names that exist, and each is
            // read-only by construction.
            | "at-peek"
            | "at-recall"
            | "at-crate"
            | "at-cardano"
            | "at-reach"
            | "at-cost"
            | "at-describe"
    )
}

fn is_run_program(name: &str) -> bool {
    matches!(
        name,
        "cargo"
            | "rustc"
            | "rustup"
            | "nix"
            | "direnv"
            | "node"
            | "npm"
            | "pnpm"
            | "yarn"
            | "bun"
            | "deno"
            | "python"
            | "python3"
            | "pytest"
            | "uv"
            | "poetry"
            | "make"
            | "just"
            | "trunk"
            | "wrangler"
            | "docker"
            | "podman"
            | "curl"
            | "wget"
            | "ssh"
            | "open"
            | "gh"
            | "tsc"
            | "sh"
            | "bash"
            | "zsh"
            | "dash"
            | "fish"
            | "ksh"
            | "exec"
            | "watch"
            | "entr"
    )
}

/// Whether `flag` is present, including inside a combined short-flag cluster (`sed -Ei`,
/// `perl -pi`) and with a value attached (`-i.bak`, `--in-place=.bak`).
///
/// A filename that begins with `-` and contains the flag's letter reads as a false positive. That
/// is the safe direction: the command is refused rather than run.
fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| {
        if arg == flag
            || arg
                .strip_prefix(flag)
                .is_some_and(|rest| rest.starts_with('=') || rest.starts_with('.'))
        {
            return true;
        }

        // Only a single-letter short flag can be clustered with others.
        if flag.len() != 2 || !flag.starts_with('-') {
            return false;
        }
        let short = &flag[1..2];

        arg.strip_prefix('-')
            .filter(|letters| !letters.starts_with('-'))
            .is_some_and(|letters| letters.contains(short))
    })
}

fn has_inline_code(args: &[String]) -> bool {
    args.iter()
        .any(|arg| matches!(arg.as_str(), "-c" | "-e" | "--eval" | "-p" | "--print"))
        || args.first().map(String::as_str) == Some("eval")
}

fn subcommand(args: &[String]) -> Option<&str> {
    args.iter()
        .find(|arg| !arg.starts_with('-'))
        .map(String::as_str)
}

fn basename(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Classify a line that must parse, and return its effect.
    fn effect(command: &str) -> Effect {
        let analysis = classify(command);
        let error = analysis.parse_error;
        assert!(error.is_none(), "`{command}`: {error:?}");
        analysis.effect
    }

    #[test]
    fn reads_are_read() {
        for command in [
            "ls -la",
            "pwd",
            "cat src/lib.rs",
            "head -20 Cargo.toml",
            "tail -n 5 Cargo.toml",
            "sed -n '40,60p' src/lib.rs",
            "awk 'NR>=1 && NR<=5' f",
            "rg -n render_claim src",
            "grep -rn TODO src",
            "wc -l src/*.rs",
            "stat src/lib.rs",
            "find . -name '*.rs'",
            "git status --short",
            "git diff --stat",
            "git log --oneline -5",
            "git -C ../.. rev-parse --show-toplevel",
            "tar -tzf archive.tar.gz",
            "nl -ba README.md | sed -n '120,150p'",
            "echo '---'",
            "which direnv nix cargo",
            "for f in *.rs; do echo \"$f\"; done",
        ] {
            assert_eq!(effect(command), Effect::Read, "`{command}`");
        }
    }

    #[test]
    fn writes_are_writes() {
        for command in [
            "sed -i 's/a/b/' src/lib.rs",
            "sed -i.bak 's/a/b/' f",
            "rm -rf target",
            "mv a b",
            "cp a b",
            "touch out",
            "mkdir -p .tmp/x",
            "tee out.txt",
            "find . -exec rm {} \\;",
            "find . -delete",
            "git add src/lib.rs",
            "git commit -m 'x'",
            "git checkout -- src/lib.rs",
            "git restore src/lib.rs",
            "git clean -fd",
            "git reset --hard",
            "git branch -D old",
            "git tag v1",
            "git stash",
            "cargo fmt",
            "rustfmt src/lib.rs",
            "tar -xzf archive.tar.gz",
            "curl -o out.json https://example.com",
            "wget https://example.com/x",
        ] {
            assert_eq!(effect(command), Effect::Write, "`{command}`");
        }
    }

    #[test]
    fn builds_and_runs_run() {
        for command in [
            "cargo test",
            "cargo build --workspace",
            "cargo clippy --all-targets",
            "node scripts/build.mjs",
            "python3 tools/x.py",
            "make",
            "just docs-build",
            "nix build .#x",
            "git fetch",
            "curl https://example.com",
        ] {
            assert_eq!(effect(command), Effect::Run, "`{command}`");
        }
    }

    #[test]
    fn an_unknown_program_is_unknown() {
        for command in [
            "./build.sh",
            "some-unknown-tool --go",
            "eval \"$X\"",
            "python3 -c 'open(\"x\", \"w\")'",
            "node -e 'require(\"fs\").rmSync(\"x\")'",
            "scp host:file .",
        ] {
            assert_eq!(effect(command), Effect::Unknown, "`{command}`");
        }
    }

    #[test]
    fn a_redirect_is_a_write_even_when_the_program_reads() {
        assert_eq!(effect("echo hi > out.txt"), Effect::Write);
        assert_eq!(effect("cat src/lib.rs >> all.txt"), Effect::Write);
        assert_eq!(effect("> out.txt"), Effect::Write);
    }

    #[test]
    fn a_device_sink_is_not_a_write() {
        assert_eq!(effect("ls > /dev/null"), Effect::Read);
        assert_eq!(effect("cargo test 2>&1 | tail -40"), Effect::Run);
        assert_eq!(effect("git status 2>&1 | head -5"), Effect::Read);
    }

    #[test]
    fn the_worst_segment_decides() {
        assert_eq!(effect("ls && pwd"), Effect::Read);
        assert_eq!(effect("ls && cargo test"), Effect::Run);
        assert_eq!(effect("cargo test && rm -rf target"), Effect::Write);
        assert_eq!(effect("ls; some-unknown-tool"), Effect::Unknown);
        assert_eq!(effect("ls | head -5"), Effect::Read);
    }

    #[test]
    fn a_pipeline_filter_is_not_a_tree_write() {
        assert_eq!(effect("cargo test | grep FAIL"), Effect::Run);
        assert_eq!(effect("rg foo . | head -20"), Effect::Read);
    }

    #[test]
    fn wrappers_are_peeled() {
        assert_eq!(effect("env FOO=1 cargo test"), Effect::Run);
        assert_eq!(effect("timeout 600 cargo test"), Effect::Run);
        assert_eq!(effect("nice -n 5 cargo test"), Effect::Run);
        assert_eq!(effect("direnv exec . cargo test"), Effect::Run);
        assert_eq!(effect("nix develop ../.. -c cargo test"), Effect::Run);
        assert_eq!(effect("xargs rm"), Effect::Write);
        assert_eq!(effect("xargs -n 1 grep foo"), Effect::Read);
    }

    #[test]
    fn a_shell_string_is_classified_by_what_it_runs() {
        assert_eq!(effect("sh -c 'rm -rf target'"), Effect::Write);
        assert_eq!(effect("bash -lc 'cargo test 2>&1 | tail -5'"), Effect::Run);
    }

    #[test]
    fn a_substitution_is_classified_by_what_it_runs() {
        assert_eq!(effect("echo \"$(date)\""), Effect::Read);
        assert_eq!(effect("ls \"$(rm -rf x)\""), Effect::Write);
        assert_eq!(effect("ls $(find . -name x)"), Effect::Read);
        assert_eq!(effect("cat <(ls)"), Effect::Read);
    }

    #[test]
    fn a_dynamic_program_is_unknown() {
        assert_eq!(effect("$CMD foo"), Effect::Unknown);
        assert_eq!(effect("git $SUB"), Effect::Unknown);
        assert_eq!(effect("cargo $CMD"), Effect::Unknown);
    }

    #[test]
    fn a_dynamic_argument_only_matters_where_a_flag_decides() {
        assert_eq!(effect("rg \"$PATTERN\" src"), Effect::Read);
        assert_eq!(effect("cat \"$FILE\""), Effect::Read);
        // A commit is a later argument; the subcommand is still `show`.
        assert_eq!(
            effect("git show --stat --format='%h' $commit | head -30"),
            Effect::Read
        );
        // A dynamic flag on `sed` could be `-i`.
        assert_eq!(effect("sed $FLAG file"), Effect::Unknown);
    }

    #[test]
    fn a_parse_error_is_unknown() {
        let analysis = classify("rg 'foo");
        assert_eq!(analysis.effect, Effect::Unknown);
        assert!(analysis.parse_error.is_some());
    }

    #[test]
    fn a_quoted_operator_is_a_word() {
        let analysis = classify("rg 'alpha|beta' .");
        assert_eq!(analysis.segments.len(), 1);
        assert_eq!(analysis.segments[0].args, ["alpha|beta", "."]);
    }

    #[test]
    fn the_analysis_names_each_segment() {
        let analysis = classify("cd integrations/rig && cargo test 2>&1 | tail -40");
        let programs: Vec<&str> = analysis
            .segments
            .iter()
            .map(|segment| segment.program.as_str())
            .collect();
        assert_eq!(programs, ["cd", "cargo", "tail"]);
        assert_eq!(analysis.effect, Effect::Run);
    }

    #[test]
    fn an_in_place_edit_is_a_write_in_every_spelling() {
        for command in [
            "sed -i 's/a/b/' f",
            "sed -i.bak 's/a/b/' f",
            "sed -Ei 's/a/b/' f",
            "sed -ni '1p' f",
            "sed --in-place 's/a/b/' f",
            "sed --in-place=.bak 's/a/b/' f",
            "perl -pi -e 's/a/b/' f",
        ] {
            assert_eq!(effect(command), Effect::Write, "`{command}`");
        }

        for command in ["sed -n '1p' f", "sed 's/a/b/' f", "sed -E 's/(a)/b/' f"] {
            assert_eq!(effect(command), Effect::Read, "`{command}`");
        }
    }

    #[test]
    fn a_flag_inside_a_short_cluster_is_seen() {
        assert_eq!(
            effect("curl -so out.json https://example.com"),
            Effect::Write
        );
        assert_eq!(effect("curl -s https://example.com"), Effect::Run);
    }

    #[test]
    fn rg_pre_is_not_a_read() {
        assert_eq!(effect("rg --pre 'cat' pattern"), Effect::Unknown);
        assert_eq!(effect("rg --pre=cat pattern"), Effect::Unknown);
        assert_eq!(effect("rg -n pattern src"), Effect::Read);
    }

    #[test]
    fn git_check_ignore_is_a_read() {
        assert_eq!(effect("git check-ignore -v integrations/rig"), Effect::Read);
    }

    #[test]
    fn a_build_capture_to_scratch_is_a_capture() {
        for command in [
            "cargo test > /tmp/log 2>&1",
            "cargo test &> /tmp/log",
            "direnv exec . cargo test > /tmp/all.log",
            "{ cargo test; } > /tmp/log",
        ] {
            let analysis = classify(command);
            assert_eq!(analysis.effect, Effect::Write, "`{command}`");
            assert_eq!(analysis.writes, Writes::Capture, "`{command}`");
        }
    }

    #[test]
    fn a_content_writer_to_scratch_is_not_a_capture() {
        for command in [
            "echo 'rm -rf /' > /tmp/x.sh",
            "cat script.sh > /tmp/x.sh",
            "printf x > /tmp/x",
            "{ echo x; } > /tmp/x.sh",
        ] {
            let analysis = classify(command);
            assert_eq!(analysis.effect, Effect::Write, "`{command}`");
            assert_eq!(analysis.writes, Writes::Other, "`{command}`");
        }
    }

    #[test]
    fn a_working_tree_redirect_is_not_a_capture() {
        assert_eq!(classify("cargo test > out.log").writes, Writes::Other);
        assert_eq!(classify("echo x > src/lib.rs").writes, Writes::Other);
        assert_eq!(classify("cargo test").writes, Writes::None);
    }

    #[test]
    fn a_write_from_a_substitution_is_not_a_capture() {
        assert_eq!(
            classify("cargo test \"$(rm -rf x)\" > /tmp/log").writes,
            Writes::Other
        );
    }

    #[test]
    fn the_toolkit_binaries_are_reads() {
        for command in [
            "at-peek search 'fn' src",
            "at-peek slice src/lib.rs:1-20",
            "at-recall state",
            "at-recall diff --patch",
            "at-crate where serde",
            "at-cardano --help",
            "at-reach --help",
            "at-cost",
            "at-describe",
        ] {
            assert_eq!(effect(command), Effect::Read, "`{command}`");
        }
    }

    #[test]
    fn a_blessed_read_does_not_bless_a_compound() {
        for command in [
            "at-peek search 'fn' src && rm -rf target",
            "at-peek search 'fn' src; git checkout -- .",
            "at-peek search 'fn' src | tee /tmp/x",
            "at-peek search 'fn' src || rm -rf target",
        ] {
            assert_eq!(effect(command), Effect::Write, "`{command}`");
        }

        assert_eq!(
            effect("echo \"$(at-peek search 'fn' src && rm x)\""),
            Effect::Write
        );
        assert_eq!(
            effect("bash -c 'at-peek search fn src && rm x'"),
            Effect::Write
        );
    }

    #[test]
    fn a_heredoc_is_read() {
        assert_eq!(effect("cat <<EOF\nhello\nEOF"), Effect::Read);
    }

    #[test]
    fn a_function_definition_is_read() {
        assert_eq!(effect("greet() { echo hi; }"), Effect::Read);
    }
}
