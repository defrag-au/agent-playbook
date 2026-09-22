---
id: defrag-agent-tools
title: Read code with the agent tools, not with the shell
layer: org
activation: org:defrag
priority: 60
overrides:
targets:
---

## Directive

- Reading code with `rg`, `grep`, `sed`, `cat`, `head` or `wc` → use `at-peek` instead. It is
  read-only, bounded by construction, and says what it did not show.

| Instead of | Use |
| --- | --- |
| `rg`, `grep` | `at-peek search <pattern> [path…]` |
| `sed -n '40,60p' <file>`, `head`, `cat` | `at-peek slice <file>:40-60` |
| `wc -l`, `ls -l`, `stat` | `at-peek stat <path>…` |
| anything not listed | `at-describe` — the catalogue, one screen |

- The exact lines · every mention · how many · which files → `at-peek slice <file>:40-60` ·
  `at-peek search <pat>` · `at-peek search <pat> --count` · `at-peek search <pat> --files-only`.
- Output that was cut says so, and so does anything that would make it wrong. Repeat both when
  you report the finding: `# 50 of 143` is the difference between a fact and a guess.
- `at-peek` is for understanding, not for preparing an edit. Read the file with the editor tools
  before editing it.
- History (`git log`, `git blame`, `git status`) is not covered by `at-peek`, and the tool that
  will cover it does not exist yet. Ask before reaching for `git`.

## Rationale

Every read through the shell is an approval I have to make and a small program I have to read
before I can decide whether to make it. `git log --format='%h %ad %an %s' --date=short | head -20`
and `sed -n '40,60p' file` are both programs whose intent I reconstruct from syntax, every time.

The worse half is the bound. `| head -20` is not a bound the agent understands: it cannot tell a
file with 20 matches from one with 2,000, so it reports "the only callers are…" from a truncated
list — the failure `verify-before-claiming` exists to prevent, with the evidence hidden in a pipe.

The tool exists and is on `PATH` in every defrag devshell, which is not enough on its own: an
agent reaches for the command it knows, and only looks for an alternative when that one is
unavailable. So the rule is keyed on the substitution — `rg` → `at-peek search` — because that is
the form the thought takes at the moment of the reach.
