# tree-sitter-browser-shot-scenario

A tree-sitter grammar for the scenario DSL documented in [`../SCENARIO.md`](../SCENARIO.md).
It is the editor-facing half of the DSL: `browser-shot check` remains the semantic source of truth.

## Layout

```text
grammar.js                  the grammar
src/parser.c                generated; checked in
src/node-types.json         generated; checked in
queries/highlights.scm      capture names for editors
test/corpus/scenario.txt    corpus test
```

The parser is generated with ABI 15 by tree-sitter 0.27.1. Regenerate after editing `grammar.js`:

```sh
cd tools/browser-shot/tree-sitter-scenario
bunx tree-sitter-cli generate --js-runtime native
bunx tree-sitter-cli test
```

`--js-runtime native` lets the CLI use its built-in QuickJS runtime. Without it, the CLI spawns
`node` to load `grammar.js`, which is not installed on this machine. If you have `node`, plain
`tree-sitter generate` works too.

## What it parses

The shape is one declaration per line, with `#` comments and blank lines:

```text
run timeout=20s

peer a url="file:///tmp/keys.html" viewport=390x844 touch
pointer a click selector="#game"
key a KeyW down hold=12
assert a "window.__keys[0].focus === 'game'" message="the canvas held focus"
capture a out=a.png
```

`run`, `peer`, `wait`, `eval`, `key`, `pointer`, `capture`, `assert` and `network` are the actions;
fields are `key=value`, and `touch`, `headed` and `await` are flags. See `SCENARIO.md` for the full
field tables.

Two constraints keep the grammar unambiguous:

- A value containing `#` is quoted. `browser-shot fmt` does this; it is why `selector="#game"` is
  canonical and `selector=#game` is not.
- Every line ends with a newline. `fmt` ensures a final newline.

The grammar intentionally does not enforce the semantic rules (known peers, required fields per
action, `hold` only on a down step). Those are `check`'s job. The grammar is for highlighting,
outline, structural selection and local parse errors.

## Queries

`queries/highlights.scm` captures:

| Node | Capture |
| --- | --- |
| action keywords | `@keyword` |
| field names | `@property` |
| quoted values, bare values | `@string` |
| peer names | `@variable` |
| key codes | `@constant` |
| `down`/`up`, `click`/`hover` | `@constant.builtin` |
| `touch`/`headed`/`await` | `@keyword` |
| expressions | `@string.special` |
| comments | `@comment` |

Verify a query against a file:

```sh
bunx tree-sitter-cli query queries/highlights.scm ../../../.tmp/ts-sample.dsl --captures
```

## Zed

This directory is a grammar source, not an installed Zed extension. Zed loads grammars from an
extension that declares a `grammars` entry and points at a grammar repository or a local path. The
next step is to package this as a small extension (or move it into an `agent-toolbox` grammar repo)
with this same `grammar.js`, `src/`, `queries/` and a `tree-sitter.json`/`package.json` describing
the `source.browser_shot_scenario` scope and the `.dsl` file type.

Until that packaging exists, the useful local checks are `browser-shot fmt`, `browser-shot check`,
and `tree-sitter parse` in this directory.
