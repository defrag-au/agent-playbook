//! One scalar out of an answer whose columns the scenario names at run time.
//!
//! This is the one place `at-cardano` reads a `serde_json::Value`, and it is a deliberate
//! exception to the typed-model rule: `save txs.last.tx_hash as prev` names a row and a column
//! in a file, so the shape is not known when the binary is built. The value is read once,
//! leaves as a string, and nothing else in the tool ever sees a tree — every projection the
//! tool prints is still a typed model.

use serde_json::Value;

use super::Row;

/// How many rows the answer carries. An array is its length, one object is one row, and a
/// `null` is what Koios returns for nothing.
pub fn count(bytes: &[u8]) -> Result<usize, String> {
    let value = parse(bytes)?;
    match &value {
        Value::Array(items) => Ok(items.len()),
        Value::Object(_) => Ok(1),
        Value::Null => Ok(0),
        _ => Err("the answer is a single value, not a row".to_string()),
    }
}

/// The scalar at `row`.`column`, as the string a later `${name}` substitutes.
pub fn scalar(bytes: &[u8], row: Row, column: &str) -> Result<String, String> {
    let value = parse(bytes)?;
    let rows = rows(&value)?;
    let index = match row {
        Row::First => 0,
        Row::Last => rows
            .len()
            .checked_sub(1)
            .ok_or_else(|| "the answer has no rows".to_string())?,
        Row::Index(index) => index,
    };
    let entry = rows.get(index).ok_or_else(|| {
        format!(
            "the answer has {} row{} · row {index} does not exist · use first, last or an index \
             below {}",
            rows.len(),
            if rows.len() == 1 { "" } else { "s" },
            rows.len()
        )
    })?;
    let object = entry
        .as_object()
        .ok_or_else(|| format!("row {index} is not a record"))?;
    let value = object
        .get(column)
        .ok_or_else(|| format!("row {index} has no column `{column}`{}", columns(object)))?;

    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Number(number) => Ok(number.to_string()),
        Value::Bool(flag) => Ok(flag.to_string()),
        Value::Null => Err(format!("`{column}` is null in row {index}")),
        Value::Array(_) | Value::Object(_) => Err(format!(
            "`{column}` in row {index} is a {} not a value · save a scalar column",
            if value.is_array() { "list," } else { "record," }
        )),
    }
}

fn parse(bytes: &[u8]) -> Result<Value, String> {
    serde_json::from_slice(bytes).map_err(|error| format!("the answer is not JSON: {error}"))
}

fn rows(value: &Value) -> Result<Vec<&Value>, String> {
    match value {
        Value::Array(items) => Ok(items.iter().collect()),
        Value::Object(_) => Ok(vec![value]),
        Value::Null => Ok(Vec::new()),
        _ => Err("the answer is a single value, not a row".to_string()),
    }
}

/// The columns a record has, so a misspelled one is answered with the spelling it wanted.
fn columns(object: &serde_json::Map<String, Value>) -> String {
    const SHOWN: usize = 12;
    let mut names: Vec<&str> = object.keys().map(String::as_str).collect();
    names.sort_unstable();
    let hidden = names.len().saturating_sub(SHOWN);
    names.truncate(SHOWN);
    let mut out = format!(" · columns: {}", names.join(", "));
    if hidden > 0 {
        out.push_str(&format!(" (+{hidden} more)"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANSWER: &[u8] =
        br#"[{"tx_hash":"aa","block_height":14100188},{"tx_hash":"bb","block_height":14100189}]"#;

    #[test]
    fn a_count_is_the_rows_koios_answered() {
        assert_eq!(count(ANSWER).expect("count"), 2);
        assert_eq!(count(b"[]").expect("empty"), 0);
        assert_eq!(count(b"null").expect("null"), 0);
        assert_eq!(count(br#"{"a":1}"#).expect("object"), 1);
        assert!(count(b"7").is_err());
    }

    #[test]
    fn a_scalar_is_first_last_or_an_index() {
        assert_eq!(scalar(ANSWER, Row::First, "tx_hash").expect("first"), "aa");
        assert_eq!(scalar(ANSWER, Row::Last, "tx_hash").expect("last"), "bb");
        assert_eq!(
            scalar(ANSWER, Row::Index(1), "block_height").expect("index"),
            "14100189"
        );
    }

    #[test]
    fn a_missing_row_names_the_size() {
        let error = scalar(ANSWER, Row::Index(9), "tx_hash").expect_err("missing");
        assert!(error.contains("row 9 does not exist"), "{error}");
        assert!(error.contains("2 rows"), "{error}");
    }

    #[test]
    fn a_missing_column_names_the_columns() {
        let error = scalar(ANSWER, Row::First, "tx_hahs").expect_err("missing");
        assert!(error.contains("no column `tx_hahs`"), "{error}");
        assert!(error.contains("block_height"), "{error}");
        assert!(error.contains("tx_hash"), "{error}");
    }

    #[test]
    fn a_nested_value_is_not_a_scalar() {
        let answer = br#"[{"assets":[{"policy":"aa"}]}]"#;
        let error = scalar(answer, Row::First, "assets").expect_err("list");
        assert!(error.contains("not a value"), "{error}");
    }
}
