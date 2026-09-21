//! Flat `key: value` readers for rule frontmatter and `.conf` files.
//!
//! The format is deliberately flat. A rule should be readable by a person and parseable
//! by a program in a screenful; nesting, block sequences and multi-line strings are not
//! supported. If a rule needs structure, it needs to be a reference file instead.

use std::collections::BTreeMap;

/// A document split into its flat header and the body that follows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Split<'a> {
    pub fields: BTreeMap<String, String>,
    pub body: &'a str,
}

impl Split<'_> {
    pub fn get(&self, key: &str) -> &str {
        self.fields.get(key).map(String::as_str).unwrap_or("")
    }

    pub fn list(&self, key: &str) -> Vec<String> {
        split_list(self.get(key))
    }
}

/// Split a markdown document into frontmatter and body.
///
/// A document that does not open with `---` has no frontmatter and is all body. An
/// unterminated header is treated the same way rather than guessed at — a rule with a
/// missing closing `---` should fail to resolve loudly, not acquire a body that starts
/// halfway through its own header.
pub fn split_document(src: &str) -> Split<'_> {
    let empty = BTreeMap::new();

    let first_line_len = match src.find('\n') {
        Some(i) => i + 1,
        None => src.len(),
    };
    if src[..first_line_len].trim_end_matches(['\n', '\r']) != "---" {
        return Split {
            fields: empty,
            body: src,
        };
    }

    let mut fields = BTreeMap::new();
    let mut offset = first_line_len;

    for line in src[offset..].split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed == "---" {
            let end = offset + line.len();
            return Split {
                fields,
                body: &src[end..],
            };
        }
        if let Some((key, value)) = split_pair(trimmed) {
            fields.insert(key.to_string(), value.to_string());
        }
        offset += line.len();
    }

    // Unterminated header: do not guess where the body starts.
    Split {
        fields: empty,
        body: src,
    }
}

/// Parse a flat `key: value` config. `#` comments and blank lines are skipped.
pub fn parse_conf(src: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    for line in src.lines() {
        let trimmed = line.trim_end_matches('\r');
        if trimmed.trim().is_empty() || trimmed.trim_start().starts_with('#') {
            continue;
        }
        if let Some((key, value)) = split_pair(trimmed) {
            fields.insert(key.to_string(), value.to_string());
        }
    }
    fields
}

/// `key: value` — split at the **first** colon, so values may contain colons
/// (`activation: language:rust`).
fn split_pair(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    let key = key.trim();
    if key.is_empty() || key.contains(char::is_whitespace) {
        return None;
    }
    Some((key, value.trim()))
}

/// Split a comma-separated list, trimming entries and dropping empties.
pub fn split_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_frontmatter_from_body() {
        let src = "---\nid: x\ntitle: A rule\n---\n\nBody line.\n";
        let split = split_document(src);
        assert_eq!(split.get("id"), "x");
        assert_eq!(split.get("title"), "A rule");
        assert_eq!(split.body.trim(), "Body line.");
    }

    #[test]
    fn value_may_contain_a_colon() {
        let src = "---\nactivation: language:rust\n---\n";
        let split = split_document(src);
        assert_eq!(split.get("activation"), "language:rust");
    }

    #[test]
    fn empty_value_is_empty_not_missing() {
        let src = "---\noverrides:\ntargets:\n---\n";
        let split = split_document(src);
        assert_eq!(split.get("overrides"), "");
        assert!(split.list("overrides").is_empty());
    }

    #[test]
    fn document_without_frontmatter_is_all_body() {
        let src = "# Just a heading\n\nText.\n";
        let split = split_document(src);
        assert!(split.fields.is_empty());
        assert_eq!(split.body, src);
    }

    #[test]
    fn unterminated_frontmatter_is_not_guessed_at() {
        let src = "---\nid: x\n\nBody that was never closed off.\n";
        let split = split_document(src);
        assert!(split.fields.is_empty());
        assert_eq!(split.body, src);
    }

    #[test]
    fn crlf_line_endings_are_tolerated() {
        let src = "---\r\nid: x\r\n---\r\nBody.\r\n";
        let split = split_document(src);
        assert_eq!(split.get("id"), "x");
        assert!(split.body.contains("Body."));
    }

    #[test]
    fn conf_skips_comments_and_blanks() {
        let src = "# a comment\n\nproject: shared-crates\norg: defrag\n";
        let conf = parse_conf(src);
        assert_eq!(
            conf.get("project").map(String::as_str),
            Some("shared-crates")
        );
        assert_eq!(conf.get("org").map(String::as_str), Some("defrag"));
        assert_eq!(conf.len(), 2);
    }

    #[test]
    fn conf_keeps_colons_in_values() {
        let conf = parse_conf("path: ~/code/x:y\n");
        assert_eq!(conf.get("path").map(String::as_str), Some("~/code/x:y"));
    }

    #[test]
    fn lists_split_and_trim() {
        assert_eq!(split_list("a, b ,c"), vec!["a", "b", "c"]);
        assert!(split_list("").is_empty());
        assert!(split_list("  ").is_empty());
    }

    #[test]
    fn prose_lines_do_not_become_fields() {
        let src = "---\nid: x\nThis is a stray prose line\n---\n";
        let split = split_document(src);
        assert_eq!(split.get("id"), "x");
        assert_eq!(split.fields.len(), 1);
    }
}
