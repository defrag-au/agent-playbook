//! Reading `SKILL.md` just far enough to publish and verify it.
//!
//! The Agent Skills specification requires `name` and `description`; everything else is the
//! agent's business. `name` must match the parent directory, which is also the store's object
//! name, so a mismatch is refused at publish time rather than discovered when a fragment does
//! not resolve.

use std::fs;
use std::path::Path;

pub struct Meta {
    pub name: String,
    pub description: String,
}

pub fn read(dir: &Path) -> Result<Meta, String> {
    let expected = dir.file_name().and_then(|name| name.to_str());
    read_skill_md(&dir.join("SKILL.md"), expected)
}

/// Read a `SKILL.md` whose parent directory is not named for the skill — an exported tree is
/// `skill/SKILL.md`, and the name check belongs against the selected object instead.
pub fn read_skill_md(path: &Path, expected_name: Option<&str>) -> Result<Meta, String> {
    let source = fs::read_to_string(path)
        .map_err(|e| format!("{}: a skill needs a SKILL.md ({e})", path.display()))?;
    let frontmatter = frontmatter(&source).ok_or_else(|| {
        format!(
            "{}: expected YAML frontmatter between `---` lines",
            path.display()
        )
    })?;

    let mut name = None;
    let mut description = None;
    for line in frontmatter.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = unquote(value.trim());
        match key.trim() {
            "name" => name = Some(value.to_string()),
            "description" => description = Some(value.to_string()),
            _ => {}
        }
    }

    let name = name
        .filter(|name| !name.is_empty())
        .ok_or_else(|| format!("{}: `name:` is required in the frontmatter", path.display()))?;
    crate::source::validate_skill_name(&name)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let description = description
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            format!(
                "{}: `description:` is required — it is what the agent matches on",
                path.display()
            )
        })?;
    if description.chars().count() > 1024 {
        return Err(format!(
            "{}: `description:` is {} characters; the limit is 1024",
            path.display(),
            description.chars().count()
        ));
    }

    let parent = expected_name.unwrap_or_default();
    if !parent.is_empty() && parent != name {
        return Err(format!(
            "{}: `name: {name}` must match the directory name `{parent}`",
            path.display()
        ));
    }

    Ok(Meta { name, description })
}

/// The description from a `SKILL.md` body, for a catalogue line.
pub fn description(source: &str) -> Option<String> {
    let frontmatter = frontmatter(source)?;
    for line in frontmatter.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.trim() == "description" {
            let value = unquote(value.trim());
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// The text between the opening and closing `---`, without either delimiter.
pub fn frontmatter(source: &str) -> Option<&str> {
    let mut lines = source.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let rest = source.split_once('\n')?.1;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end().trim_end_matches('\r') == "---" {
            return Some(&rest[..offset]);
        }
        offset += line.len();
    }
    None
}

fn unquote(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return &value[1..value.len() - 1];
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_stops_at_the_closing_delimiter() {
        let source = "---\nname: x\ndescription: y\n---\n\n# Body\n";
        assert_eq!(frontmatter(source), Some("name: x\ndescription: y\n"));
    }

    #[test]
    fn frontmatter_may_not_be_absent() {
        assert_eq!(frontmatter("# Body\n"), None);
    }

    #[test]
    fn quoted_values_are_unquoted() {
        assert_eq!(unquote("\"hello: world\""), "hello: world");
        assert_eq!(unquote("'x'"), "x");
        assert_eq!(unquote("x"), "x");
    }
}
