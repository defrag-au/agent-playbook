//! How a response becomes content lines.
//!
//! Every verb names one of these functions in its catalogue entry, so the projection is part
//! of the grammar's description rather than something a dispatch arm has to remember to call.

use serde::de::DeserializeOwned;

use crate::contract::Fail;
use crate::model;
use crate::verbs::Options;

pub const MAX_JSON_OUTPUT: usize = 64 * 1024;
pub const MAX_HEX_OUTPUT: usize = 256 * 1024;

/// The body of an answer: content lines, plus any bound the projection itself needs to state
/// (a cut, a hex budget). The engine adds the bounds that belong to the request.
#[derive(Default)]
pub struct Rendered {
    pub lines: Vec<String>,
    pub bounds: Vec<String>,
}

impl Rendered {
    fn lines(lines: Vec<String>) -> Self {
        Self {
            lines,
            bounds: Vec::new(),
        }
    }
}

pub type Render = fn(&[u8], &Options) -> Result<Rendered, Fail>;

/// Rows that are each their own line: delegators, holdings, votes.
pub fn rows<T: model::Row + DeserializeOwned>(bytes: &[u8], _: &Options) -> Result<Rendered, Fail> {
    let rows: Vec<T> = parse(bytes)?;
    Ok(Rendered::lines(rows.iter().map(model::Row::line).collect()))
}

/// One target, several labelled lines: a block, an address, a transaction.
pub fn records<T: model::Record + DeserializeOwned>(
    bytes: &[u8],
    _: &Options,
) -> Result<Rendered, Fail> {
    let records: Vec<T> = parse(bytes)?;
    let mut lines = Vec::new();
    for (index, record) in records.iter().enumerate() {
        if index > 0 {
            lines.push(String::new());
        }
        lines.extend(record.lines());
    }
    Ok(Rendered::lines(lines))
}

/// One object rather than an array of rows — `/cli_protocol_params` is the only one.
pub fn record_object<T: model::Record + DeserializeOwned>(
    bytes: &[u8],
    _: &Options,
) -> Result<Rendered, Fail> {
    let record: T = serde_json::from_slice(bytes).map_err(|error| {
        Fail::environment(format!(
            "Koios answered JSON this tool could not read: {error}"
        ))
    })?;
    Ok(Rendered::lines(record.lines()))
}

/// A record whose body is a nested list — an account's reward history, a pool's relays, a
/// script's redeemers. The endpoint answers one record per target, so `--limit` bounds the
/// *targets*, not the nested rows; this caps what is printed and says how much was held back.
pub fn records_capped<T: model::Record + DeserializeOwned>(
    bytes: &[u8],
    options: &Options,
) -> Result<Rendered, Fail> {
    let mut rendered = records::<T>(bytes, options)?;
    let cap = model::MAX_NESTED_LINES;
    if rendered.lines.len() > cap {
        let hidden = rendered.lines.len() - cap;
        rendered.lines.truncate(cap);
        rendered.bounds.push(format!(
            "showing {cap} lines; {hidden} more · --json carries all of them, or narrow the \
             request (`--epoch`, one target)"
        ));
    }
    Ok(rendered)
}

/// `/tx_info`, whose summary depends on the parts the caller asked for.
pub fn tx_info(bytes: &[u8], options: &Options) -> Result<Rendered, Fail> {
    let rows: Vec<model::TxInfo> = parse(bytes)?;
    let mut lines = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            lines.push(String::new());
        }
        lines.extend(row.lines_with(options.inputs, options.assets));
    }
    Ok(Rendered::lines(lines))
}

/// `/tx_cbor`: the bytes themselves, cut at a budget with the cut stated.
pub fn tx_cbor(bytes: &[u8], _: &Options) -> Result<Rendered, Fail> {
    let rows: Vec<model::TxCborRow> = parse(bytes)?;
    let mut lines = Vec::new();
    let mut bounds = Vec::new();
    let mut printed = 0usize;
    for row in &rows {
        lines.push(format!("{}  {} hex chars", row.tx_hash, row.cbor.len()));
        if printed >= MAX_HEX_OUTPUT {
            continue;
        }
        let room = MAX_HEX_OUTPUT - printed;
        let take = row.cbor.len().min(room);
        for chunk in row.cbor.as_bytes()[..take].chunks(120) {
            lines.push(String::from_utf8_lossy(chunk).into_owned());
        }
        printed += take;
        if take < row.cbor.len() {
            bounds.push(format!(
                "cut at {MAX_HEX_OUTPUT} hex chars; {} remain · narrow the batch",
                row.cbor.len() - take
            ));
        }
    }
    Ok(Rendered { lines, bounds })
}

/// A payload whose shape is not modelled — metadata, a metadata document, an endpoint Koios
/// documents no schema for. Bounded, and the bound is stated.
pub fn raw(bytes: &[u8], _: &Options) -> Result<Rendered, Fail> {
    let mut bounds = Vec::new();
    if bytes.len() > MAX_JSON_OUTPUT {
        bounds.push(format!(
            "cut at {MAX_JSON_OUTPUT} bytes of {} · narrow with --select/--limit, or page with \
             --offset",
            bytes.len()
        ));
    }
    let shown = &bytes[..bytes.len().min(MAX_JSON_OUTPUT)];
    Ok(Rendered {
        lines: json_lines(shown),
        bounds,
    })
}

fn parse<T: DeserializeOwned>(bytes: &[u8]) -> Result<Vec<T>, Fail> {
    serde_json::from_slice(bytes).map_err(|error| {
        Fail::environment(format!(
            "Koios answered JSON this tool could not read: {error}"
        ))
    })
}

/// Break JSON at commas and opening delimiters outside strings, so a compact body becomes
/// readable lines without changing a byte inside a value. No `Value` tree is built.
fn json_lines(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut escaped = false;

    for character in text.chars() {
        current.push(character);
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' | '[' | ',' => {
                current.push('\n');
                let done = std::mem::take(&mut current);
                if !done.trim().is_empty() {
                    lines.push(done);
                }
            }
            _ => {}
        }
        if current.chars().count() >= 200 {
            current.push('\n');
            let done = std::mem::take(&mut current);
            if !done.trim().is_empty() {
                lines.push(done);
            }
        }
    }

    if !current.trim().is_empty() {
        lines.push(current);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_lines_break_outside_strings_only() {
        let lines = json_lines(br#"[{"tx_hash":"a,b","n":1},{"tx_hash":"c"}]"#);
        assert!(lines.iter().any(|line| line.contains("\"a,b\"")));
        assert!(lines.len() > 2);
    }
}
