//! Rendering a resolved set into the managed block.
//!
//! Output must be byte-stable for a given input: `check` diffs it, so anything
//! nondeterministic here becomes a false "stale" report.

use crate::resolve::Resolved;

pub const BEGIN_PREFIX: &str = "<!-- BEGIN agent-playbook";
pub const END_MARKER: &str = "<!-- END agent-playbook -->";

pub fn render(resolved: &Resolved) -> String {
    let mut out = String::new();

    out.push_str(&format!(
        "{BEGIN_PREFIX} (project: {}, target: {}) — generated, do not edit -->\n\n",
        resolved.project.name, resolved.target.name
    ));
    out.push_str(&format!("# Agent rules — {}\n\n", resolved.project.name));
    out.push_str(&format!(
        "Generated from agent-playbook (`projects/{}` + `models/{}`).\n\
         Rule sources live under `{}` — each heading's HTML comment names its file there.\n\
         To change a rule, change the rule — not this block.\n",
        resolved.project.name,
        resolved.target.name,
        resolved.root.display()
    ));

    if !resolved.emphasis.is_empty() {
        out.push_str("\n## Non-negotiable\n\n");
        out.push_str(
            "The lead of each rule below, repeated so it is read first. \
             Full text follows in place.\n\n",
        );
        for rule in &resolved.emphasis {
            out.push_str(&format!("- **{}** — {}\n", rule.title, rule.lead()));
        }
    }

    if !resolved.superseded.is_empty() {
        out.push_str("\nSuperseded by a higher layer in this project:\n\n");
        for id in &resolved.superseded {
            out.push_str(&format!("- `{id}`\n"));
        }
    }

    for rule in &resolved.rules {
        out.push_str(&format!("\n## {}\n", rule.title));
        out.push_str(&format!(
            "<!-- rule: {} -->\n\n",
            rule.rel.trim_end_matches(".md")
        ));
        out.push_str(&rewrite_links(&rule.body));
        out.push('\n');
    }

    for addendum in &resolved.addenda {
        out.push('\n');
        out.push_str(&rewrite_links(&demote_headings(addendum.body.trim())));
        out.push('\n');
    }

    out.push_str(END_MARKER);
    out.push('\n');
    out
}

/// Rewrite a relative markdown link to a `.md` file into a code span.
///
/// A rule cross-reference like `[the widget-screenshot skill](../../../skills/x/SKILL.md)`
/// is correct inside the playbook and dead inside `shared-crates`, because the rendered
/// block lives in a different repository. Keeping the label as a code span preserves what
/// the reader needs — a rule id or a skill name — and drops a link that would point nowhere.
fn rewrite_links(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;

    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        rest = &rest[open..];

        let rewritten = (|| {
            let label_end = rest.find("](")?;
            let target_start = label_end + 2;
            let target_end = rest[target_start..].find(')')? + target_start;
            let target = &rest[target_start..target_end];
            if !target.ends_with(".md") || target.starts_with("http") {
                return None;
            }
            let label = &rest[1..label_end];
            let shown = if label.is_empty() { target } else { label };
            Some((format!("`{shown}`"), target_end + 1))
        })();

        match rewritten {
            Some((replacement, consumed)) => {
                out.push_str(&replacement);
                rest = &rest[consumed..];
            }
            None => {
                out.push('[');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Push every heading in an addendum down one level, so it sits under the rules rather
/// than competing with them.
fn demote_headings(body: &str) -> String {
    let mut out = String::new();
    for line in body.lines() {
        if line.starts_with('#') && line.trim_start_matches('#').starts_with(' ') {
            out.push('#');
        }
        out.push_str(line);
        out.push('\n');
    }
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

/// Extract the managed block from a file, markers included.
pub fn extract_block(source: &str) -> Option<String> {
    let mut out = String::new();
    let mut inside = false;
    for line in source.lines() {
        if !inside && line.starts_with(BEGIN_PREFIX) {
            inside = true;
        }
        if inside {
            out.push_str(line);
            out.push('\n');
            if line.starts_with(END_MARKER) {
                return Some(out);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_a_block_including_markers() {
        let src = "Hand written.\n\n<!-- BEGIN agent-playbook (project: x, target: y) -->\nbody\n<!-- END agent-playbook -->\n\nMore hand written.\n";
        let block = extract_block(src).unwrap();
        assert!(block.starts_with(BEGIN_PREFIX));
        assert!(block.trim_end().ends_with(END_MARKER));
        assert!(block.contains("body"));
        assert!(!block.contains("Hand written"));
        assert!(!block.contains("More hand written"));
    }

    #[test]
    fn no_block_is_none() {
        assert!(extract_block("just prose\n").is_none());
    }

    #[test]
    fn an_unterminated_block_is_none() {
        // Better to report "no block" than to splice over a file whose markers were
        // half-deleted by hand.
        assert!(
            extract_block("<!-- BEGIN agent-playbook (project: x, target: y) -->\nbody\n")
                .is_none()
        );
    }

    #[test]
    fn headings_are_demoted_one_level() {
        let out = demote_headings("# Title\n\ntext\n\n## Sub\n");
        assert!(out.contains("## Title"));
        assert!(out.contains("### Sub"));
    }
}
