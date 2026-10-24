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
tree-sitter.json            grammar manifest (name, scope, .dsl file type)
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

`run`, `peer`, `wait`, `eval`, `key`, `pointer`, `capture`, `burst`, `assert` and `network` are
the actions; fields are `key=value`, and `touch`, `headed` and `await` are flags. Pointer kinds are
`click`, `hover`, `drag`, `tap` and `swipe`. See `SCENARIO.md` for the full field tables.

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
| `down`/`up`, `click`/`hover`/`drag`/`tap`/`swipe` | `@constant.builtin` |
| `touch`/`headed`/`await` | `@keyword` |
| expressions | `@string.special` |
| comments | `@comment` |

Verify a query against a file:

```sh
bunx tree-sitter-cli query queries/highlights.scm ../../../.tmp/ts-sample.dsl --captures
```

## Zed

This directory is a grammar source, not an installed Zed extension. `tree-sitter.json` declares the
name (`scenario`), scope (`source.browser_shot_scenario`) and the `.dsl` file type, so a tool that
reads the manifest can load it directly; `browser-shot` itself does not need it.

Zed loads a grammar through an installed extension. Packaging it as one is three files:

```text
extension.toml                    id, name, version, and a [grammars.scenario] entry
languages/scenario/config.toml    name, grammar = "scenario", path_suffixes = ["dsl"]
grammars/ or a repository/rev pair grammar.js, src/, queries/
```

That extension belongs beside this grammar in `agent-toolbox`, not in this repository: a Zed
extension needs its own repository (or a subdirectory with a published revision) and a version
bump for every grammar change. Until then the useful local checks are `browser-shot fmt`,
`browser-shot check`, and `tree-sitter parse` in this directory.
