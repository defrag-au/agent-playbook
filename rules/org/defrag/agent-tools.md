---
id: defrag-agent-tools
title: Read code and history with the agent tools, not with the shell
layer: org
activation: org:defrag
priority: 60
overrides:
targets:
---

## Directive

- Reading code with `rg`, `grep`, `sed`, `cat`, `head` or `wc` → use `at-peek`. Reading history or
  working-tree state with `git status`, `git diff`, `git log` or `git show` → use `at-recall`. Both
  are read-only, bounded by construction, and say what they did not show.

| Instead of | Use |
| --- | --- |
| `rg`, `grep` | `at-peek search <pattern> [path…]` |
| `sed -n '40,60p' <file>`, `head`, `cat` | `at-peek slice <file>:40-60` |
| `wc -l`, `ls -l`, `stat` | `at-peek stat <path>…` |
| `git status`, `git status --porcelain` | `at-recall state` |
| `git diff`, `git diff --stat` | `at-recall diff [<rev>] [<path>…]` — add `--patch` for the hunks |
| anything not listed | `at-describe` — the catalogue, one screen |

- The exact lines · every mention · how many · which files → `at-peek slice <file>:40-60` ·
  `at-peek search <pat>` · `at-peek search <pat> --count` · `at-peek search <pat> --files-only`.
- `which at-peek` empty → `direnv exec . at-peek <verb>`. An agent's spawned shell does not
  inherit the devshell environment; an interactive one does.
- Output that was cut says so, and so does anything that would make it wrong. Repeat both when
  you report the finding: `# 50 of 143` is the difference between a fact and a guess.
- A `# next:` line is the next question, already spelled as a command. Run it as printed rather
  than composing your own — it is the read you just made, widened or deepened.
- Several questions in one turn → `at-recall … --summary` (the frame: what was read, what was cut,
  what to ask next), and several targets in one invocation (`diff <path> <path>`, `slice
  <path>:40-60 <path>:1-20`) rather than a loop. Never `| tail -3`: a tail is a bound you did not
  read, and the bound is the part that makes the answer reportable.
- The tools are for understanding, not for preparing an edit. Read the file with the editor tools
  before editing it.
- `at-peek` runs nothing at all; `at-recall` runs `git` and nothing else. That difference is why
  they are separate binaries, and why they are separate approvals if I have tiered them.
- `at-recall` answers `state` and `diff`. `log`, `blame`, `show`, `churn` and the report recipes
  are designed and not written — ask for the one you want rather than reaching for `git`, and name
  the question, because that is what turns it into a verb.

## Rationale

Every read through the shell is an approval I have to make and a small program I have to read
before I can decide whether to make it. `git log --format='%h %ad %an %s' --date=short | head -20`
and `sed -n '40,60p' file` are both programs whose intent I reconstruct from syntax, every time.
`git diff --stat` reads as a request for one line per file and returns whatever the repository has.

The worse half is the bound. `| head -20` is not a bound the agent understands: it cannot tell a
file with 20 matches from one with 2,000, so it reports "the only callers are…" from a truncated
list — the failure `verify-before-claiming` exists to prevent, with the evidence hidden in a pipe.

The tool exists and is on `PATH` in every defrag devshell, which is not enough on its own: an
agent reaches for the command it knows, and only looks for an alternative when that one is
unavailable. So the rule is keyed on the substitution — `rg` → `at-peek search`, `git status` →
`at-recall state` — because that is the form the thought takes at the moment of the reach.
