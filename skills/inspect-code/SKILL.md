---
name: inspect-code
description: Use when finding, reading, counting or locating code or history — replaces rg, grep, sed, cat, head, wc, git status, git diff and git log for that job with the read-only at-peek and at-recall verbs, and covers what to do when the toolkit is not installed.
---

# Reading code and history with the agent toolkit

The situation that should have brought you here: you are about to run `rg`, `grep`, `sed`, `cat`,
`head`, `wc`, `git status`, `git diff` or `git log` to look at something. Don't — reach for
`at-peek` for the working tree and `at-recall` for history and state. Both are read-only, bounded
by construction, and their output says what they did not show.

`at-describe` lists everything the toolkit can do, on one screen. This skill is the mapping and
the traps.

## The mapping

```sh
# the working tree — at-peek runs nothing at all
at-peek search 'render_claim'                 # every mention, walked from the repo root
at-peek search 'fn \w+' tools/ --count        # matches per file, with the real total
at-peek search 'FIXME' --files-only           # just the paths
at-peek slice crates/x/src/lib.rs:40-60       # these exact lines, numbered
at-peek slice crates/x/src/lib.rs:40+20       # twenty lines from 40
at-peek stat crates/x/src/lib.rs              # lines, bytes, language, mtime

# history and state — at-recall runs git, read verbs only, and never the reflog
at-recall state                               # branch, HEAD, merge or rebase in progress, changed paths
at-recall diff                                # what changed, one line per file, with the totals
at-recall diff crates/x/src/lib.rs --patch    # the hunks, for one file
at-recall diff HEAD~1..HEAD                   # a range of history: reads no working-tree file
at-recall log --limit 5                       # the last five commits, with the true total
at-recall log crates/x/src/lib.rs             # that file's history

# the recipe — one read for "write me a PR description"
at-recall pr                                  # base, commits, diffstat by kind, areas
at-recall pr --with areas                     # just the shape of the change set
```

`--limit N` caps the output (default 200 lines); `--root <path>` points the tool at another tree;
`--max-files N` bounds a walk. Every ceiling is announced when it clamps.

## What the output tells you

A `# ` line is metadata: the header names what was read, and the trailing bounds name what was
not. `# 50 of 143 matches in 27 files` means what it says — 143 is the count over every file the
walk considered, not the number it happened to print. Repeat the bound when you report the
finding; the whole point of the tool is that a truncated answer stops being mistakable for a
complete one.

`# skipped by rule: target, node_modules` and `# skipped: 3 secret-shaped · 2 binary` are the same
discipline: what it refused to read, named rather than dropped.

## Surveying several questions

Asking five things in a row is normal, and the two habits that assemble it — `; echo "=== x ==="` to
tag each block, and `| tail -3` to keep each one short — are both worse than the tools:

- **The header is the tag.** Every answer starts `# at-recall state · <root> · <branch> · <sha>`, so
a block is self-labelling. An `echo` label is for `git`, which does not name itself.
- **`--summary` is the short form.** It prints the frame — header, context, counts, bounds, exits —
and none of the rows. Five questions cost about twenty lines instead of two hundred, and nothing has
to be piped.
- **`| tail -3` is a bound you did not read.** It hides the very lines that say what was cut. If
an answer is too long to read, `--summary` it or narrow it with `--limit`/a path.

```sh
at-recall state --summary                       # branch, HEAD, pending, # 6 paths, not shown
at-recall diff --summary                        # # 6 files, +78 -33, not shown
at-peek search 'fn main' --summary               # # 12 matches in 4 files, not shown
at-recall diff src/a.rs src/b.rs --patch        # several paths, one invocation
```

A `--summary` answer still ends with its exits, so the survey is where the loop starts: read the
frames, then follow exactly one of them. `stat` and `slice` have no `--summary` — their rows *are*
the answer, so there is no frame to ask for; `at-peek search` also has `--count` and `--files-only`
as shapes between the listing and the frame.

## The exits

An answer ends with its exits, when it has any:

```
# 10 of 119 lines
# next: at-peek slice src/cache.rs:11-40 · the lines that follow
```

```
# 14 of 367 lines · --limit 14 reached
# next: at-recall diff HEAD --patch --limit 367 · all 367 lines
```

That is a command, not a suggestion — **run it as printed.** It is the same read you just made,
widened (a cut answer offers the limit that would have fit) or deepened (the hunks behind the
table, the lines after the range, the files behind a refusal). It carries `--root` when you gave
one, so it reads the tree you meant.

A silent footer means what it says: nothing was cut, and there is nothing further at this level. If
you need something the exits do not offer, the answer genuinely does not contain it — ask, rather
than composing a pipeline to go looking.

`at-recall state` exits 0 even on a clean tree, because the branch and HEAD *are* the answer to
"what am I looking at" — read the count from the bound: `# 0 paths`. `at-recall diff` exits 1 when
nothing differs, and 4 when it refused. `at-recall pr` exits 1 when the branch has nothing on top of
its base, which is the one answer you can branch on the code alone.

## Recipes

A recipe is one verb that composes the reads its sibling verbs use, so its count cannot disagree with
the listing beside it. That is why it is a verb and not a sequence of the ones above: the sequence
costs an approval per step and a re-read of each step's text.

```sh
at-recall pr                          # "write me a PR description": the facts for it, in one read
at-recall pr --base main              # when the branch tracks nothing and there is no origin/HEAD
at-recall pr --with commits,diffstat  # sections: commits, diffstat, areas
at-recall pr --with areas             # one line: the shape of the change set
```

`pr` prints the base it resolved, the branch's own commits, every changed file with its counts and
its **kind** — `manifest`, `lockfile`, `schema`, `generated`, `docs`, `tests`, `assets` or `code`, by
path name only, first rule wins — and then the change set by kind:
`# areas      17 files · 2 manifest · 2 lockfile · 3 docs · 10 code`. It reports unasked when part of
a diff is nothing but whitespace: `# whitespace only: 1 file, +1 -1 of the lines`.

Two things it will not do, and both matter when you write the description:

- **It never fetches.** The base is whatever this repository has locally, and every answer says so:
  `# caveat: origin/main is a local ref and this tool never fetches — the remote may be ahead`.
- **It does not write the description.** Why a change exists is the one fact not in the repository,
  so the recipe hands you the facts and leaves the prose to you.

Its exits name the primitives rather than the recipe — `at-recall log <range> --limit N` for a cut
commit list, `at-recall diff <range> --patch` for the hunks — so following one leaves the recipe
without leaving the question.

## Traps

- **Neither tool is a read for the purposes of editing.** They do not satisfy the editor tools'
  read-before-edit check, so a `slice` followed by an edit gets a stale-read refusal. Use them to
  understand, then read the file with the editor tools before changing it.
- **`path:40-60` is one token, not two arguments**, and ranges are 1-based and inclusive.
- **`at-peek` never opens `.git`.** "Is this file tracked", "what branch is this" and "who wrote
  it" are `at-recall` questions — of which only the first two are answered so far. `blame` is
  designed and not written; ask rather than reaching for `git`.
- **A revision is a revision only if it resolves.** `at-recall diff <first>` treats its first
  argument as a revision when it names a commit and as a path otherwise — git's own rule, so
  `diff main` works and `diff src/main.rs` does. A range (`A..B`, `A...B`) that does not resolve
  is an error rather than a path, and `HEAD@{1}` is refused by name: this tool does not read the
  reflog.
- **`at-recall diff` compares against HEAD, not the index**, so staged and unstaged changes both
  count. An untracked file never appears — no diff between commits can show it — and the tool says
  so rather than printing "no differences" and leaving you to guess.
- **A repository that routes paths through a filter driver is refused by name.** `filter.<driver>.clean`
  in `.gitattributes` is a program the repository names, and `at-recall` runs git only. The refusal
  lists the paths (`a.foo (filter=lfs)`); if you need the filtered form, that is a `git diff` to ask
  about.
- **A big diff may be formatting.** `at-recall diff --ignore-space` compares lines ignoring
  whitespace (`git diff -w`) and states what that hid: `# with whitespace: 9 files, +412 -118
  (3 whitespace-only)`. One read answers "is this change real"; the two-read version with an `echo`
  between them is the habit it replaces.
- **A file in another repository needs `--root`.** Each invocation resolves one root — the worktree
  you are in, or the path you name — so reading a file in a sibling repo is
  `at-peek slice --root ~/code/github/pallas src/hashes.rs:95-166`. An absolute path is not a
  reason to reach for `sed`; it is a reason to name the root.
- **`search` does not honour `.gitignore`.** It refuses a fixed list of directory names
  (`target`, `node_modules`, `.direnv`, `dist`, `result`, `.tmp`, `.git`) and names what it
  skipped. If a search looks incomplete, read the trailer before concluding the code is absent.
- **A walk is bounded by `--max-files`** (default 20000) and says so when it stops. Narrow the
  path rather than raising it.
- **If `which at-peek` finds nothing**, the devshell environment is not loaded — a spawned shell
  does not inherit it, only an interactive one does. Reach for it explicitly:
  `direnv exec . at-peek search '<pat>'`. Do not fall back to `rg` without saying so.
