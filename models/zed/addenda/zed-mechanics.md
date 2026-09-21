# Zed mechanics

## The file that gets read

**`AGENTS.md`** at the repository root. Zed reads it as workspace instructions, scoped to
that project root. `CLAUDE.md` is not read by Zed unless it is named in settings — which is
the argument for `AGENTS.md` being the canonical managed block in every repo, with
`CLAUDE.md` reduced to a pointer.

Rules can also be scoped to a specific root directory in a multi-root workspace, so a rule
that applies to one project in the workspace does not leak into the others.

## Rendering

- **Mermaid renders.** ` ```mermaid ` blocks are drawn as diagrams. Supported: flowchart,
  sequence, class, state, ER, gantt, pie, gitgraph, mindmap, timeline, quadrant, xy, journey.
  Anything else shows as a code block.
- **Inline HTML inside mermaid is not rendered** — it is better to skip the formatting than
  to write `<b>` and have it appear literally.
- **Images render from remote URLs, absolute paths, and paths relative to a workspace root.**
  This is what makes the screenshot workflow usable — write the screenshot into `.tmp/` in the
  repo and reference it.
- Do not include `%%{init}%%` directives or custom `classDef` in mermaid; the theme is
  supplied.

## Editing

Prefer the editor tools for edits, shell commands for reads and builds. Zed shows the diff
for an editor edit and does not for a `sed` — see
[`rules/core/edit-via-editor-tools`](../../../rules/core/edit-via-editor-tools.md).
