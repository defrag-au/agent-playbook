---
id: defrag-agent-tools
title: Read code and history with the agent tools, not with the shell
layer: org
activation: ecosystem:defrag
priority: 60
overrides:
targets:
---

## Directive

- Reading code with `rg`, `grep`, `sed`, `cat`, `head` or `wc` → `at-peek`. Installed crate source or
  build-script output → `at-crate`. History or working-tree state → `at-recall`. A Cardano chain
  question → `at-cardano`. All four are read-only and say what they did not show.
- `git show` and `git blame` have no `at-recall` verb yet — say so rather than reaching for them.

| Instead of | Use |
| --- | --- |
| `rg`, `grep` | `at-peek search <pattern> [path…]` |
| `sed -n '40,60p' <file>`, `head`, `cat` | `at-peek slice <file>:40-60` |
| `wc -l`, `ls -l`, `stat` | `at-peek stat <path>…` |
| `grep -rn … ~/.cargo/…` | `at-crate search <pattern> <crate>` · `slice` · `where` |
| `git status` | `at-recall state` |
| `git diff` | `at-recall diff [<rev>] [<path>…]` — `--patch` for hunks |
| `git log` | `at-recall log [<rev>] [<path>…]` |
| PR archaeology | `at-recall pr [--base <rev>]` |
| `curl …koios…` | `at-cardano <noun> [<aspect>]` · `query` · `run --scenario` |
| anything not listed | `at-describe` — the catalogue, one screen |

- `which at-peek` empty → `direnv exec . at-peek <verb>`. Never wrap an `at-*` call in
  `nix develop`: the evaluation is paid before the read starts (7.5 s vs 5 ms).
- Output that was cut says so, and a `# next:` line is the next read — run it as printed.
  Never `| tail -3`: the bound is the part that makes the answer reportable.
- These tools are for understanding, not for preparing an edit. Read the file with the editor
  tools before changing it.
- The `inspect-code` skill has the flags, exit semantics, recipes and traps; the `at-cardano`
  skill has the chain verbs. Invoke one when a read is not a plain `search` or `slice`.

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

Installed crate source is the same reach with a worse address: `cargo info` names a version but
not a path, the path is a registry directory with an opaque suffix, and generated code sits under
a build hash. `at-crate` resolves through the project's lock, so "the version the project uses" is
the default rather than a memory of the latest one.

**Why `ecosystem:defrag` and not `org:defrag`.** The rule is true of a repository because of the
toolchain it is built with, not because of who operates it: `~/code/hodlcroft/compositor` lives
under a hodlcroft directory and takes its devshell — and so its `at-*` binaries — from
`defrag-nix`'s `rust-worker-stack`, so it is a repository this rule is true of. An org-scoped
activation said only "the repos whose directory happens to be called defrag", which was a proxy
that went wrong the first time a repo outside that directory used the same shell. The layer stays
`org` because that is where it sorts; the activation is the precise statement of what it depends
on. See the activation table in `docs/rule-format.md`.
