//! Reading a scenario file.
//!
//! The whole file is parsed before anything runs, and the parser resolves each `target` through
//! the same catalogue the command line uses. So a misspelled noun, an aspect the noun does not
//! have, a field the verb does not take, a reference to a name no `save` binds and an endpoint
//! that writes to the chain are all reported while the line is read — with the file, line and
//! column — rather than after the first request has been paid for.

use std::collections::BTreeSet;
use std::time::Duration;

use crate::catalogue::{self, Route, Verb};
use crate::contract::Fail;
use crate::koios::{self, Network};
use crate::verbs::targets;

use super::{
    Assert, Check, Compare, Echo, Field, Part, Row, Save, Scenario, Step, Target, Template,
    DEFAULT_TIMEOUT, MAX_BYTES, MAX_STEPS, MAX_TIMEOUT,
};

/// Read and parse `path`. The only file a scenario reads is itself.
pub fn file(path: &str) -> Result<Scenario, Fail> {
    if !path.ends_with(".dsl") {
        return Err(Fail::usage(format!(
            "{path} is not a `.dsl` file · the grammar is in tools/at-cardano/SCENARIO.md"
        )));
    }
    let metadata = std::fs::metadata(path)
        .map_err(|error| Fail::environment(format!("cannot read {path}: {error}")))?;
    if metadata.len() > MAX_BYTES {
        return Err(Fail::usage(format!(
            "{path} is {} bytes · a scenario is capped at {MAX_BYTES}",
            metadata.len()
        )));
    }
    let text = std::fs::read_to_string(path)
        .map_err(|error| Fail::environment(format!("cannot read {path}: {error}")))?;
    parse(path, &text)
}

pub fn parse(path: &str, text: &str) -> Result<Scenario, Fail> {
    let mut builder = Builder {
        path,
        run: None,
        steps: Vec::new(),
        targets: Vec::new(),
        binds: BTreeSet::new(),
    };
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let tokens = tokenize(path, line, raw)?;
        if tokens.is_empty() {
            continue;
        }
        builder.line(line, &tokens)?;
    }
    builder.finish()
}

struct Token {
    text: String,
    /// The token began with a quote, so it is one value and never a field.
    leading_quote: bool,
    column: usize,
}

struct RunLine {
    network: Option<Network>,
    keyless: bool,
    timeout: Duration,
}

struct Builder<'a> {
    path: &'a str,
    run: Option<RunLine>,
    steps: Vec<Step>,
    /// Target names, in the order they were declared.
    targets: Vec<String>,
    /// Names a `save` has bound. A `${…}` may only name one of these.
    binds: BTreeSet<String>,
}

impl Builder<'_> {
    fn line(&mut self, line: usize, tokens: &[Token]) -> Result<(), Fail> {
        let action = &tokens[0];
        if action.leading_quote {
            return Err(self.fail(
                line,
                action.column,
                "a line starts with run, target, save, assert or echo",
            ));
        }
        match action.text.as_str() {
            "run" => self.run_line(line, tokens),
            "target" => self.target_line(line, tokens),
            "save" => self.save_line(line, tokens),
            "assert" => self.assert_line(line, tokens),
            "echo" => self.echo_line(line, tokens),
            other => Err(self.fail(
                line,
                action.column,
                format!("`{other}` is not an action · run, target, save, assert or echo"),
            )),
        }
    }

    fn run_line(&mut self, line: usize, tokens: &[Token]) -> Result<(), Fail> {
        if self.run.is_some() || !self.steps.is_empty() {
            return Err(self.fail(
                line,
                tokens[0].column,
                "the run line comes once, before any step",
            ));
        }

        let mut network = None;
        let mut keyless = false;
        let mut timeout = DEFAULT_TIMEOUT;
        for token in &tokens[1..] {
            if token.leading_quote {
                return Err(self.fail(
                    line,
                    token.column,
                    "run takes only fields · network=, keyless, timeout=",
                ));
            }
            let (name, value) = match split_field(token) {
                Some((name, value)) => (name, value),
                None => (token.text.as_str(), None),
            };
            match (name, value) {
                ("network", Some(raw)) => {
                    network = Some(Network::parse(raw).ok_or_else(|| {
                        self.fail(
                            line,
                            token.column,
                            "network is mainnet, preprod, preview or guild",
                        )
                    })?);
                }
                ("keyless", Some(raw)) => keyless = boolean(line, token, raw, self.path)?,
                ("keyless", None) => keyless = true,
                ("timeout", Some(raw)) => {
                    timeout = duration(raw).ok_or_else(|| {
                        self.fail(
                            line,
                            token.column,
                            format!("timeout `{raw}` is not a duration · 120s, 500ms"),
                        )
                    })?;
                    if timeout.is_zero() {
                        return Err(self.fail(line, token.column, "timeout must be positive"));
                    }
                    if timeout > MAX_TIMEOUT {
                        return Err(self.fail(
                            line,
                            token.column,
                            format!("timeout is capped at {}s", MAX_TIMEOUT.as_secs()),
                        ));
                    }
                }
                ("timeout", None) => {
                    return Err(self.fail(
                        line,
                        token.column,
                        "timeout needs a duration · 120s, 500ms",
                    ))
                }
                (other, _) => {
                    return Err(self.fail(
                        line,
                        token.column,
                        format!("`{other}` is not a run field · network=, keyless, timeout="),
                    ))
                }
            }
        }

        self.run = Some(RunLine {
            network,
            keyless,
            timeout,
        });
        Ok(())
    }

    fn target_line(&mut self, line: usize, tokens: &[Token]) -> Result<(), Fail> {
        if tokens.len() < 3 {
            return Err(self.fail(
                line,
                tokens[0].column,
                "target is `target <name> <noun> [<aspect>] [positionals…]`",
            ));
        }
        let name = self.ident(line, &tokens[1], "target name")?;
        if self.targets.iter().any(|known| known == &name) {
            return Err(self.fail(
                line,
                tokens[1].column,
                format!("`{name}` is already a target · names are bound once"),
            ));
        }

        let noun = catalogue::find_noun(&tokens[2].text).ok_or_else(|| {
            self.fail(
                line,
                tokens[2].column,
                format!(
                    "`{}` is not a noun · nouns: {}",
                    tokens[2].text,
                    catalogue::names()
                ),
            )
        })?;

        // A typo in an aspect must not read as a positional: resolve it before anything else,
        // and only fall back to the noun's principal question when the noun has one.
        let mut index = 3;
        let mut resolved = None;
        if let Some(token) = tokens.get(3) {
            if !token.leading_quote && split_field(token).is_none() {
                if let Some(verb) = noun.find(&token.text) {
                    resolved = Some(verb);
                    index = 4;
                }
            }
        }
        let verb = match resolved {
            Some(verb) => verb,
            None => noun.default_verb().ok_or_else(|| {
                let column = tokens.get(3).map_or(tokens[2].column, |token| token.column);
                self.fail(
                    line,
                    column,
                    format!(
                        "`{}` needs an aspect · {}",
                        noun.name,
                        noun.aspects().join(", ")
                    ),
                )
            })?,
        };

        // A word of letters is an aspect the noun does not have, not a pool id. Without this,
        // a typo reads as a positional and is answered with "that is not a pool id". A noun with
        // one verb has no aspects to confuse, so its next token is always a target.
        if resolved.is_none() && noun.verbs.len() > 1 {
            if let Some(token) = tokens.get(3) {
                if !token.leading_quote
                    && split_field(token).is_none()
                    && looks_like_a_word(&token.text)
                {
                    return Err(self.fail(
                        line,
                        token.column,
                        format!(
                            "`{}` is not an aspect of `{}` · aspects: {}",
                            token.text,
                            noun.name,
                            noun.aspects().join(", ")
                        ),
                    ));
                }
            }
        }

        let mut positionals = Vec::new();
        let mut fields = Vec::new();
        let mut positional_column = tokens[0].column;
        for token in &tokens[index..] {
            if token.leading_quote {
                if positionals.is_empty() {
                    positional_column = token.column;
                }
                positionals.push(self.template(line, token)?);
                continue;
            }
            if let Some((field_name, value)) = split_field(token) {
                fields.push(self.field(line, verb, token, field_name, value)?);
                continue;
            }
            if Field::switch(&token.text, true).is_some()
                || Field::valued(&token.text, empty()).is_some()
                || matches!(token.text.as_str(), "network" | "keyless" | "json" | "help")
            {
                fields.push(self.field(line, verb, token, &token.text, None)?);
                continue;
            }
            if positionals.is_empty() {
                positional_column = token.column;
            }
            positionals.push(self.template(line, token)?);
        }

        match &verb.route {
            Route::Call(call) => {
                if positionals.iter().all(Template::is_literal) {
                    let literal: Vec<String> = positionals.iter().map(Template::literal).collect();
                    targets::validate(call, &literal)
                        .map_err(|error| self.fail(line, tokens[0].column, error.message))?;
                }
            }
            Route::Query => {
                if positionals.len() != 1 {
                    return Err(self.fail(
                        line,
                        tokens[0].column,
                        "`query` needs one endpoint name",
                    ));
                }
                if let Some(endpoint) = positionals[0]
                    .is_literal()
                    .then(|| positionals[0].literal())
                {
                    if koios::refused_endpoint(&endpoint) {
                        return Err(Fail::refused(format!(
                            "{}:{}:{}: /{endpoint} writes to the chain; at-cardano answers reads only",
                            self.path, line, positional_column
                        )));
                    }
                    if !koios::valid_endpoint(&endpoint) {
                        return Err(self.fail(
                            line,
                            positional_column,
                            format!("`{endpoint}` is not a Koios endpoint name"),
                        ));
                    }
                }
            }
            Route::Auth | Route::Endpoints | Route::Scenario => {
                return Err(self.fail(
                    line,
                    tokens[0].column,
                    format!(
                        "`{}` has no chain answer to check · a target names a verb that asks Koios",
                        verb.name
                    ),
                ))
            }
        }

        let body = fields.iter().any(|field| matches!(field, Field::Body(_)));
        let posted = fields
            .iter()
            .any(|field| matches!(field, Field::Post(true)));
        if body && !posted {
            return Err(self.fail(
                line,
                tokens[0].column,
                "body= is sent only with `post` · add post, or drop the body",
            ));
        }

        self.targets.push(name.clone());
        self.steps.push(Step::Target(Target {
            line,
            name,
            verb,
            positionals,
            fields,
        }));
        Ok(())
    }

    fn field(
        &self,
        line: usize,
        verb: &'static Verb,
        token: &Token,
        name: &str,
        value: Option<&str>,
    ) -> Result<Field, Fail> {
        let flag_name = format!("--{name}");
        let flag = verb
            .flags
            .iter()
            .find(|flag| flag.name == flag_name)
            .ok_or_else(|| {
                self.fail(
                    line,
                    token.column,
                    format!(
                        "`{}` has no `{name}` field · fields: {}",
                        verb.name,
                        field_names(verb)
                    ),
                )
            })?;

        match name {
            "network" | "keyless" => {
                return Err(self.fail(
                    line,
                    token.column,
                    format!("`{name}` is set once, on the run line · not per target"),
                ))
            }
            "json" => {
                return Err(self.fail(
                    line,
                    token.column,
                    "a target does not take json · the run prints every answer as the tool would",
                ))
            }
            "body-file" => {
                return Err(self.fail(
                    line,
                    token.column,
                    "a scenario reads only its own file · use body= with the JSON inline",
                ))
            }
            "help" => {
                return Err(self.fail(
                    line,
                    token.column,
                    "`help` is not a scenario field · run `at-cardano help <noun>`",
                ))
            }
            _ => {}
        }

        if !flag.takes_value {
            let value = match value {
                None => true,
                Some(raw) => boolean(line, token, raw, self.path)?,
            };
            return Field::switch(name, value).map_or_else(
                || {
                    Err(self.fail(
                        line,
                        token.column,
                        format!("`{name}` is not a field a scenario knows"),
                    ))
                },
                Ok,
            );
        }

        let Some(raw) = value else {
            return Err(self.fail(
                line,
                token.column,
                format!("`{name}` needs a value · {name}=…"),
            ));
        };
        let template = self.template_text(line, token.column, raw)?;
        let field = Field::valued(name, template).ok_or_else(|| {
            self.fail(
                line,
                token.column,
                format!("`{name}` is not a field a scenario knows"),
            )
        })?;

        // A literal value can be judged now, with the same code the run will use. A value with a
        // `${…}` hole is judged when it is filled, because that is when it becomes a value.
        if field_literal(&field).is_some_and(Template::is_literal) {
            let mut probe = crate::verbs::Options::default();
            if let Err(message) = field.apply(&mut probe, &std::collections::HashMap::new()) {
                return Err(self.fail(line, token.column, message));
            }
        }
        Ok(field)
    }

    fn save_line(&mut self, line: usize, tokens: &[Token]) -> Result<(), Fail> {
        if tokens.len() != 4 || tokens[2].text != "as" || tokens[2].leading_quote {
            return Err(self.fail(
                line,
                tokens[0].column,
                "save is `save <target>.<row>.<column> as <name>`",
            ));
        }
        let (source, row, column) = parse_path(&tokens[1].text).map_err(|message| {
            self.fail(
                line,
                tokens[1].column,
                format!("{message} · e.g. `txs.last.tx_hash`"),
            )
        })?;
        if !self.targets.iter().any(|known| known == &source) {
            return Err(self.fail(
                line,
                tokens[1].column,
                format!(
                    "`{source}` is not a target yet · a save reads an answer already asked for"
                ),
            ));
        }
        let bind = self.ident(line, &tokens[3], "save name")?;
        if !self.binds.insert(bind.clone()) {
            return Err(self.fail(
                line,
                tokens[3].column,
                format!("`{bind}` is already bound · names are bound once"),
            ));
        }
        self.steps.push(Step::Save(Save {
            line,
            source,
            row,
            column,
            bind,
        }));
        Ok(())
    }

    fn assert_line(&mut self, line: usize, tokens: &[Token]) -> Result<(), Fail> {
        if tokens.len() < 3 {
            return Err(self.fail(
                line,
                tokens[0].column,
                "assert is `assert <target> count=3 | count>0 | contains=\"…\" | absent=\"…\"`",
            ));
        }
        let source = self.ident(line, &tokens[1], "assert target")?;
        if !self.targets.iter().any(|known| known == &source) {
            return Err(self.fail(
                line,
                tokens[1].column,
                format!("`{source}` is not a target · assert checks an answer already asked for"),
            ));
        }

        let mut checks = Vec::new();
        let mut message = None;
        for token in &tokens[2..] {
            if token.leading_quote {
                return Err(self.fail(
                    line,
                    token.column,
                    "an assert is checks · count=3, count>0, contains=\"…\", absent=\"…\"",
                ));
            }
            let Some((name, operator, value)) = split_check(&token.text) else {
                return Err(self.fail(
                    line,
                    token.column,
                    "not a check · count=3, count>0, contains=\"…\", absent=\"…\"",
                ));
            };
            match (name, operator) {
                ("message", "=") => {
                    if message.is_some() {
                        return Err(self.fail(line, token.column, "one message= per assert"));
                    }
                    message = Some(self.template_text(line, token.column, value)?);
                }
                ("count", _) => {
                    let compare = Compare::parse(operator).ok_or_else(|| {
                        self.fail(
                            line,
                            token.column,
                            format!("`{operator}` is not an operator"),
                        )
                    })?;
                    let expected: usize = value.parse().map_err(|_| {
                        self.fail(
                            line,
                            token.column,
                            format!("count {value} is not a whole number"),
                        )
                    })?;
                    checks.push(Check::Count(compare, expected));
                }
                ("contains", "=") => {
                    checks.push(Check::Contains(self.template_text(
                        line,
                        token.column,
                        value,
                    )?));
                }
                ("absent", "=") => {
                    checks.push(Check::Absent(self.template_text(
                        line,
                        token.column,
                        value,
                    )?));
                }
                (other, _) => {
                    return Err(self.fail(
                        line,
                        token.column,
                        format!(
                            "`{other}` is not a check · count=, count>, count<, contains=, absent="
                        ),
                    ))
                }
            }
        }
        if checks.is_empty() {
            return Err(self.fail(
                line,
                tokens[0].column,
                "assert needs at least one check · count=, contains=, absent=",
            ));
        }
        self.steps.push(Step::Assert(Assert {
            line,
            source,
            checks,
            message,
        }));
        Ok(())
    }

    fn echo_line(&mut self, line: usize, tokens: &[Token]) -> Result<(), Fail> {
        if tokens.len() < 2 {
            return Err(self.fail(line, tokens[0].column, "echo needs text"));
        }
        let mut text = String::new();
        for (index, token) in tokens[1..].iter().enumerate() {
            if index > 0 {
                text.push(' ');
            }
            text.push_str(&token.text);
        }
        let template = self.template_text(line, tokens[1].column, &text)?;
        self.steps.push(Step::Echo(Echo {
            line,
            text: template,
        }));
        Ok(())
    }

    fn ident(&self, line: usize, token: &Token, what: &str) -> Result<String, Fail> {
        if token.leading_quote || !valid_ident(&token.text) {
            return Err(self.fail(
                line,
                token.column,
                format!(
                    "`{}` is not a {what} · lowercase letters, digits and `_`",
                    token.text
                ),
            ));
        }
        Ok(token.text.clone())
    }

    fn template(&self, line: usize, token: &Token) -> Result<Template, Fail> {
        self.template_text(line, token.column, &token.text)
    }

    fn template_text(&self, line: usize, column: usize, text: &str) -> Result<Template, Fail> {
        let mut parts = Vec::new();
        let mut literal = String::new();
        let mut index = 0;

        while index < text.len() {
            if text[index..].starts_with("${") {
                let start = index;
                let Some(end) = text[index + 2..].find('}').map(|end| index + 2 + end) else {
                    return Err(self.fail(
                        line,
                        column + start,
                        "`${` is not closed · write `${name}`",
                    ));
                };
                let name = &text[index + 2..end];
                if !valid_ident(name) {
                    return Err(self.fail(
                        line,
                        column + start,
                        format!("`${{{name}}}` is not a name · lowercase letters, digits and `_`"),
                    ));
                }
                if !self.binds.contains(name) {
                    return Err(self.fail(
                        line,
                        column + start,
                        format!(
                            "`${{{name}}}` is not bound · a `save … as {name}` line must come before \
                             it"
                        ),
                    ));
                }
                if !literal.is_empty() {
                    parts.push(Part::Literal(std::mem::take(&mut literal)));
                }
                parts.push(Part::Ref(name.to_string()));
                index = end + 1;
                continue;
            }
            let character = text[index..].chars().next().unwrap_or(' ');
            literal.push(character);
            index += character.len_utf8();
        }
        if !literal.is_empty() {
            parts.push(Part::Literal(literal));
        }
        Ok(Template::new(parts, text.to_string()))
    }

    fn finish(self) -> Result<Scenario, Fail> {
        if self.steps.is_empty() {
            return Err(Fail::usage(format!(
                "{}: the scenario has no steps",
                self.path
            )));
        }
        if self.steps.len() > MAX_STEPS {
            return Err(Fail::usage(format!(
                "{}: {} steps · a scenario is capped at {MAX_STEPS}",
                self.path,
                self.steps.len()
            )));
        }
        let run = self.run.unwrap_or(RunLine {
            network: None,
            keyless: false,
            timeout: DEFAULT_TIMEOUT,
        });
        Ok(Scenario {
            network: run.network,
            keyless: run.keyless,
            timeout: run.timeout,
            steps: self.steps,
        })
    }

    fn fail(&self, line: usize, column: usize, message: impl std::fmt::Display) -> Fail {
        Fail::usage(format!("{}:{}:{column}: {message}", self.path, line))
    }
}

/// The fields a scenario can set on a verb, derived from its flags — so the parse error and the
/// help cannot disagree.
fn field_names(verb: &Verb) -> String {
    let mut names = Vec::new();
    for flag in verb.flags {
        let name = flag.name.trim_start_matches('-');
        if matches!(name, "network" | "keyless" | "json" | "body-file" | "help") {
            continue;
        }
        if Field::switch(name, true).is_some() || Field::valued(name, empty()).is_some() {
            names.push(name);
        }
    }
    if names.is_empty() {
        "none".to_string()
    } else {
        names.join(", ")
    }
}

fn empty() -> Template {
    Template::new(Vec::new(), String::new())
}

fn field_literal(field: &Field) -> Option<&Template> {
    match field {
        Field::Limit(template)
        | Field::Offset(template)
        | Field::Order(template)
        | Field::Select(template)
        | Field::Epoch(template)
        | Field::Body(template)
        | Field::Query(template) => Some(template),
        _ => None,
    }
}

fn split_field(token: &Token) -> Option<(&str, Option<&str>)> {
    if token.leading_quote {
        return None;
    }
    token
        .text
        .split_once('=')
        .map(|(name, value)| (name, Some(value)))
}

/// The first operator in a check: `count=3`, `count>=1`, `contains="…"`.
fn split_check(text: &str) -> Option<(&str, &str, &str)> {
    let bytes = text.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'=' => return Some((&text[..index], "=", &text[index + 1..])),
            b'>' | b'<' => {
                let (operator, width) = match bytes.get(index + 1) {
                    Some(b'=') => {
                        if *byte == b'>' {
                            (">=", 2)
                        } else {
                            ("<=", 2)
                        }
                    }
                    _ => {
                        if *byte == b'>' {
                            (">", 1)
                        } else {
                            ("<", 1)
                        }
                    }
                };
                return Some((&text[..index], operator, &text[index + width..]));
            }
            _ => {}
        }
    }
    None
}

fn parse_path(text: &str) -> Result<(String, Row, String), String> {
    let parts: Vec<&str> = text.split('.').collect();
    if parts.len() != 3 {
        return Err(format!(
            "`{text}` is not `<target>.<row>.<column>` · the row is a number, `first` or `last`"
        ));
    }
    let source = parts[0];
    if !valid_ident(source) {
        return Err(format!(
            "`{source}` is not a target name · lowercase letters, digits and `_`"
        ));
    }
    let row = match parts[1] {
        "first" => Row::First,
        "last" => Row::Last,
        raw => Row::Index(
            raw.parse()
                .map_err(|_| format!("`{raw}` is not a row · the index, `first` or `last`"))?,
        ),
    };
    let column = parts[2];
    if !valid_column(column) {
        return Err(format!(
            "`{column}` is not a column name · lowercase letters, digits, `_`, and not a `.`"
        ));
    }
    Ok((source.to_string(), row, column.to_string()))
}

/// A word is the shape an aspect typo takes: short, lowercase, not hex (a hash of `a`s is not
/// a word) and not a bech32 or numeric target.
fn looks_like_a_word(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 24
        && !targets::is_hex(text)
        && text
            .chars()
            .all(|character| character.is_ascii_lowercase() || character == '-')
}

fn valid_ident(text: &str) -> bool {
    let mut characters = text.chars();
    matches!(characters.next(), Some(first) if first.is_ascii_lowercase())
        && characters.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
}

fn valid_column(text: &str) -> bool {
    let mut characters = text.chars();
    matches!(characters.next(), Some(first) if first.is_ascii_lowercase() || first == '_')
        && characters.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
}

fn boolean(line: usize, token: &Token, raw: &str, path: &str) -> Result<bool, Fail> {
    match raw {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(Fail::usage(format!(
            "{path}:{line}:{}: `{other}` is not a boolean · true or false",
            token.column
        ))),
    }
}

fn duration(raw: &str) -> Option<Duration> {
    let (digits, milliseconds) = if let Some(rest) = raw.strip_suffix("ms") {
        (rest, 1u64)
    } else if let Some(rest) = raw.strip_suffix('s') {
        (rest, 1000)
    } else {
        (raw, 1)
    };
    let value: u64 = digits.parse().ok()?;
    Some(Duration::from_millis(value.checked_mul(milliseconds)?))
}

/// Split one line into tokens. A quote in the middle of a token is part of it, so
/// `body='{"a":1}'` is one token whose value happens to be quoted; a token that *begins* with a
/// quote is one value, and never a field.
fn tokenize(path: &str, line: usize, raw: &str) -> Result<Vec<Token>, Fail> {
    let characters: Vec<char> = raw.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < characters.len() {
        if characters[index].is_whitespace() {
            index += 1;
            continue;
        }
        if characters[index] == '#' {
            break;
        }

        let column = index + 1;
        let mut text = String::new();
        let mut leading_quote = false;
        let mut started = false;

        while index < characters.len() {
            let character = characters[index];
            if character.is_whitespace() {
                break;
            }
            if character == '"' || character == '\'' {
                if !started {
                    leading_quote = true;
                }
                started = true;
                let quote = character;
                index += 1;
                loop {
                    let Some(current) = characters.get(index).copied() else {
                        return Err(Fail::usage(format!(
                            "{path}:{line}:{column}: unterminated quote"
                        )));
                    };
                    if current == quote {
                        index += 1;
                        break;
                    }
                    if current == '\\' {
                        let Some(escaped) = characters.get(index + 1).copied() else {
                            return Err(Fail::usage(format!(
                                "{path}:{line}:{column}: unterminated quote"
                            )));
                        };
                        match escaped {
                            'n' => text.push('\n'),
                            't' => text.push('\t'),
                            '\\' => text.push('\\'),
                            '"' => text.push('"'),
                            '\'' => text.push('\''),
                            other => {
                                return Err(Fail::usage(format!(
                                    "{path}:{line}:{column}: `\\{other}` is not an escape · \
                                     \\n, \\t, \\\\, \\\" and \\' only"
                                )))
                            }
                        }
                        index += 2;
                        continue;
                    }
                    text.push(current);
                    index += 1;
                }
                continue;
            }
            text.push(character);
            started = true;
            index += 1;
        }

        tokens.push(Token {
            text,
            leading_quote,
            column,
        });
    }

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error(text: &str) -> String {
        match parse("test.dsl", text) {
            Ok(_) => panic!("must not parse"),
            Err(fail) => fail.message,
        }
    }

    #[test]
    fn a_token_beginning_with_a_quote_is_never_a_field() {
        let tokens = tokenize("t.dsl", 1, "target a pool \"pool1=x\" # note").expect("tokens");
        assert_eq!(tokens.len(), 4);
        assert!(tokens[3].leading_quote);
        assert_eq!(tokens[3].text, "pool1=x");
    }

    #[test]
    fn a_hash_inside_a_token_is_kept_and_at_the_start_is_a_comment() {
        let tokens = tokenize("t.dsl", 1, "echo abc#def # trailing").expect("tokens");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[1].text, "abc#def");
    }

    #[test]
    fn quoted_values_keep_spaces_and_equals() {
        let tokens =
            tokenize("t.dsl", 1, "assert t contains=\"a b=c\" message='why not'").expect("tokens");
        assert_eq!(tokens[2].text, "contains=a b=c");
        assert_eq!(tokens[3].text, "message=why not");
    }

    #[test]
    fn escapes_are_the_ones_the_siblings_document() {
        let tokens = tokenize("t.dsl", 1, r#"echo "a\nb\tc""#).expect("tokens");
        assert_eq!(tokens[1].text, "a\nb\tc");
        assert!(tokenize("t.dsl", 1, r#"echo "a\qb""#).is_err());
    }

    const VALID: &str = "\
# a block's transactions, and the first of them
run network=mainnet timeout=60s

target tip tip
save tip.0.block_height as height
target block block ${height}
save block.0.hash as block_hash
target txs block txs ${block_hash} limit=3

assert txs count>=1
assert txs count=3 message=\"three transactions\"
save txs.last.tx_hash as last_tx
echo \"block ${block_hash} last tx ${last_tx}\"\n";

    #[test]
    fn a_scenario_with_every_action_parses() {
        let scenario = parse("test.dsl", VALID).expect("valid");
        assert_eq!(scenario.network, Some(Network::Mainnet));
        assert_eq!(scenario.timeout.as_secs(), 60);
        assert_eq!(scenario.steps.len(), 9);
    }

    #[test]
    fn an_unknown_action_names_the_actions() {
        let message = error("fetch tip\n");
        assert!(message.contains("`fetch` is not an action"), "{message}");
        assert!(message.contains("target"), "{message}");
    }

    #[test]
    fn an_unknown_noun_lists_the_nouns() {
        let message = error("target t tipp\n");
        assert!(message.contains("`tipp` is not a noun"), "{message}");
        assert!(message.contains("tip"), "{message}");
        assert!(message.contains("pool"), "{message}");
    }

    #[test]
    fn a_misspelled_aspect_lists_the_aspects() {
        let message = error("target p pool deegators pool1abc\n");
        assert!(
            message.contains("`deegators` is not an aspect of `pool`"),
            "{message}"
        );
        assert!(message.contains("delegators"), "{message}");
    }

    #[test]
    fn a_noun_without_a_principal_question_needs_an_aspect() {
        let message = error("target p proposal gov_action1abc\n");
        assert!(message.contains("needs an aspect"), "{message}");
        assert!(message.contains("summary"), "{message}");
    }

    #[test]
    fn a_field_the_verb_does_not_take_is_caught_while_the_line_is_read() {
        let message = error("target t tip limit=3\n");
        assert!(message.contains("`tip` has no `limit` field"), "{message}");
        assert!(message.contains("fields: none"), "{message}");
    }

    #[test]
    fn a_field_the_scenario_reserves_is_named_as_such() {
        let message = error("target t tip json=true\n");
        assert!(message.contains("does not take json"), "{message}");
        let message = error("target t tip keyless\n");
        assert!(message.contains("set once, on the run line"), "{message}");
    }

    #[test]
    fn a_literal_field_value_is_validated_at_parse_time() {
        let message = error("target t address utxos addr1example limit=zero\n");
        assert!(message.contains("limit zero is not a number"), "{message}");
    }

    #[test]
    fn a_field_that_needs_a_value_says_so() {
        let message = error("target t address utxos addr1example limit\n");
        assert!(message.contains("`limit` needs a value"), "{message}");
    }

    #[test]
    fn an_undeclared_reference_is_an_error_while_the_line_is_read() {
        let message = error("target t tip\ntarget b block ${height}\n");
        assert!(message.contains("`${height}` is not bound"), "{message}");
        assert!(message.contains("save"), "{message}");
    }

    #[test]
    fn a_save_reads_an_answer_already_asked_for() {
        let message = error("run\nsave txs.0.tx_hash as prev\n");
        assert!(message.contains("`txs` is not a target yet"), "{message}");
    }

    #[test]
    fn a_save_path_is_target_row_column() {
        let message = error("target t tip\nsave t.middle.x as y\n");
        assert!(message.contains("`middle` is not a row"), "{message}");
        let message = error("target t tip\nsave t.0 as y\n");
        assert!(
            message.contains("is not `<target>.<row>.<column>`"),
            "{message}"
        );
    }

    #[test]
    fn a_save_name_is_bound_once() {
        let message = error("target t tip\nsave t.0.a as x\nsave t.0.b as x\n");
        assert!(message.contains("`x` is already bound"), "{message}");
    }

    #[test]
    fn a_target_name_is_bound_once() {
        let message = error("target t tip\ntarget t tip\n");
        assert!(message.contains("`t` is already a target"), "{message}");
    }

    #[test]
    fn an_assert_needs_a_check_not_only_a_message() {
        let message = error("target t tip\nassert t message=\"well\"\n");
        assert!(message.contains("at least one check"), "{message}");
    }

    #[test]
    fn an_assert_checks_an_answer_already_asked_for() {
        let message = error("assert t count=1\n");
        assert!(message.contains("`t` is not a target"), "{message}");
    }

    #[test]
    fn a_local_verb_has_no_answer_to_check() {
        let message = error("target e endpoints\n");
        assert!(
            message.contains("`endpoints` has no chain answer to check"),
            "{message}"
        );
    }

    #[test]
    fn submitting_a_transaction_is_refused_while_the_line_is_read() {
        match parse("test.dsl", "target s query submittx\n") {
            Ok(_) => panic!("must be refused"),
            Err(fail) => {
                assert_eq!(fail.exit.code(), 4);
                assert!(
                    fail.message.contains("writes to the chain"),
                    "{}",
                    fail.message
                );
            }
        }
    }

    #[test]
    fn a_body_is_sent_only_with_post() {
        let message = error("target q query tx_info body=\"{}\"\n");
        assert!(message.contains("only with `post`"), "{message}");
    }

    #[test]
    fn the_run_line_comes_once_and_first() {
        let message = error("target t tip\nrun network=preprod\n");
        assert!(message.contains("run line comes once"), "{message}");
        let message = error("run network=mars\n");
        assert!(message.contains("mainnet, preprod"), "{message}");
    }

    #[test]
    fn a_scenario_with_no_steps_is_an_error() {
        let message = error("# nothing but a comment\n");
        assert!(message.contains("no steps"), "{message}");
    }
}
