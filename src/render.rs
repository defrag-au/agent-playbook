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
    // The heading is the target's title alone. Naming the project here read as
    // "# Personal instructions — personal", and for a repo block the project name is already
    // in the file's path and in the provenance line below.
    out.push_str(&format!("# {}\n\n", resolved.target.title));
    // No root path. It was here so a reader could find the playbook that produced the block, and
    // it read as provenance — but a path is only true on the machine that rendered it, it differs
    // between a checkout (`/Users/…`) and the packaged copy (`/nix/store/…`), and that difference
    // made every `check` from a devshell report `stale` against a block that was in fact in sync.
    // What identifies a rule is the file named in its own comment, which is playbook-relative and
    // says the same thing everywhere. Which *revision* a block came from is a separate question —
    // the flake knows its own rev and could bake it in, at the cost of a bump making every
    // installed block stale by design.
    out.push_str(&format!(
        "Generated from agent-playbook (`projects/{}` + `models/{}`).\n\
         Each rule's source file is named in the comment above its heading.\n\
         To change a rule, change the rule — not this block.\n",
        resolved.project.name, resolved.target.name
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
        // Only the directive is compiled. The rationale stays in the rule file at source —
        // the block is for an agent's attention budget, not for the argument behind the rule.
        out.push_str(&rewrite_links(&demote_headings(&rule.directive)));
        out.push('\n');
    }

    for section in &resolved.memory {
        blank_line(&mut out);
        out.push_str(&rewrite_links(&demote_headings(section.body.trim())));
        out.push('\n');
    }

    for addendum in &resolved.addenda {
        blank_line(&mut out);
        out.push_str(&rewrite_links(&demote_headings(addendum.body.trim())));
        out.push('\n');
    }

    out.push_str(END_MARKER);
    out.push('\n');
    out
}

/// Ensure the buffer ends with exactly one blank line, so consecutive blocks are
/// separated by one line rather than accumulating one per block.
fn blank_line(out: &mut String) {
    if out.is_empty() || out.ends_with("\n\n") {
        return;
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
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
            let label = rest[1..label_end].trim();
            let shown = if label.is_empty() { target } else { label };
            // A rule cross-reference is usually written `[`id`](path)` — the label is
            // already a code span, and wrapping it again would produce double backticks.
            let replacement = if shown.len() >= 2 && shown.starts_with('`') && shown.ends_with('`')
            {
                shown.to_string()
            } else {
                format!("`{shown}`")
            };
            Some((replacement, target_end + 1))
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

/// Push every heading down one level, so a rule's own sub-headings sit under its title rather
/// than competing with it.
///
/// Fenced code blocks are skipped: a `#` comment in a shell example is not a heading, and
/// demoting it corrupts the example. It did — a `.env.example` block came out reading
/// `## .env.example — committed`, which is not what anyone would write in a `.env` file.
fn demote_headings(body: &str) -> String {
    let mut out = String::new();
    let mut in_fence = false;

    for line in body.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
        } else if !in_fence
            && line.starts_with('#')
            && line.trim_start_matches('#').starts_with(' ')
        {
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
    fn addenda_are_separated_by_exactly_one_blank_line() {
        let mut out = String::from("rule body\n");
        blank_line(&mut out);
        out.push_str("first addendum\n");
        blank_line(&mut out);
        out.push_str("second addendum\n");
        assert!(!out.contains("\n\n\n"), "got: {out:?}");
        assert!(out.contains("rule body\n\nfirst addendum\n\nsecond addendum\n"));
    }

    #[test]
    fn blank_line_is_idempotent() {
        let mut out = String::from("a\n");
        blank_line(&mut out);
        blank_line(&mut out);
        assert_eq!(out, "a\n\n");
    }

    #[test]
    fn headings_are_demoted_one_level() {
        let out = demote_headings("# Title\n\ntext\n\n## Sub\n");
        assert!(out.contains("## Title"));
        assert!(out.contains("### Sub"));
    }

    #[test]
    fn a_hash_comment_inside_a_code_block_is_not_demoted() {
        // The regression: a `.env.example` block came out reading `## .env.example —
        // committed`, which is not what anyone would write in a `.env` file.
        let body = "```\n# .env.example — committed\nKEY=value\n```\n";
        assert_eq!(demote_headings(body), body);
    }

    #[test]
    fn headings_outside_a_code_block_are_still_demoted() {
        let body = "# Title\n\n```sh\n# a comment\n```\n\n## Sub\n";
        let out = demote_headings(body);
        assert!(out.contains("## Title"));
        assert!(out.contains("# a comment"));
        assert!(out.contains("### Sub"));
    }

    #[test]
    fn a_fence_with_a_language_tag_toggles_too() {
        let body = "```sh\n# comment\n```\n\n# Heading\n";
        let out = demote_headings(body);
        assert!(out.contains("# comment"));
        assert!(out.contains("## Heading"));
    }

    #[test]
    fn a_tilde_fence_is_recognised() {
        let body = "~~~\n# comment\n~~~\n";
        assert_eq!(demote_headings(body), body);
    }

    #[test]
    fn relative_md_links_become_code_spans() {
        // The rendered block lives in another repo, where `../core/x.md` points nowhere.
        let body = "see [`core/working-first`](../core/working-first.md) for why";
        let out = rewrite_links(body);
        assert_eq!(out, "see `core/working-first` for why");
        assert!(!out.contains(".."));
    }

    #[test]
    fn links_in_addenda_are_rewritten_too() {
        let body = "See [`core/planning-stays-in-thinking`](../../../rules/core/planning-stays-in-thinking.md).";
        let out = rewrite_links(body);
        assert_eq!(out, "See `core/planning-stays-in-thinking`.");
    }

    #[test]
    fn absolute_links_are_left_alone() {
        let body = "[crates.io](https://crates.io/crates/x) and [docs](https://x.dev/a.md)";
        assert_eq!(rewrite_links(body), body);
    }

    #[test]
    fn non_markdown_links_are_left_alone() {
        let body = "[notes](notes.txt)";
        assert_eq!(rewrite_links(body), body);
    }

    #[test]
    fn text_with_brackets_survives() {
        // Rule bodies contain arrays and task lists. Neither is a link, and neither may
        // be mangled by the rewriter.
        let body = "an array `[u8; 4]` and a list:\n\n- [x] done\n- [ ] todo\n";
        assert_eq!(rewrite_links(body), body);
    }

    #[test]
    fn a_link_label_is_kept_when_it_differs_from_the_target() {
        let body = "the [widget-screenshot skill](../../../skills/widget-screenshot/SKILL.md)";
        assert_eq!(rewrite_links(body), "the `widget-screenshot skill`");
    }
}
