---
name: crate-research
description: Use when adopting a crate, checking whether a suggested crate exists, or working with a crate whose API may be newer than training data. Researches the real current version and writes a cheat sheet so later sessions don't have to guess.
---

# Researching a crate

Use this before adding a dependency, and whenever you are about to write code against a crate
whose API you are recalling from memory rather than reading.

## 1. Find the real current version

Start with the toolchain that is already installed:

```sh
cargo info <crate>              # version, license, rust-version, docs link and features
cargo info <crate>@<version>    # the same for a specific version
```

That is about 2 KB and is authoritative for the version. The crates.io summary page
(`https://crates.io/crates/<crate>`) says the same and links the repository. An internet search
will happily return a blog post about an API that was renamed two majors ago.

**Never fetch `https://crates.io/api/v1/crates/<crate>`.** It returns every version and feature
set as one JSON document: 200 kB to 1.3 MB per crate, and 1.34 MB for `clap` in the measured set.
Researching several crates this way in one turn is what exhausts a context window. If `cargo info`
does not answer the question, go to the repository, not to the API. If it fails (offline registry,
a private crate), say so and stop — the API is not the fallback.

Once the crate is in the project, the version that matters is the one `Cargo.lock` chose — not the
latest one. `at-crate where <crate>` names it, the extracted source directory, and any build-script
output. `at-crate installed <crate>` lists every extracted version when the crate is not in this
project's lock.

## 2. Read the installed source with `at-crate`

The source on disk is the version that will compile, and it is the one to read. Do not reconstruct
paths under `~/.cargo/registry` with `find`, `ls`, `grep` or `sed`:

```sh
at-crate where headless_chrome              # version, source path, build output
at-crate files headless_chrome --depth 2    # src/, examples/, json/, benches/ …
at-crate slice headless_chrome/README.md    # the crate's own description
at-crate slice headless_chrome/examples/take_screenshot.rs
at-crate search 'set_device_metrics' headless_chrome
```

Read in this order:

1. **The README** — what the crate thinks it is for, and usually a quickstart.
2. **The `examples/` directory** — this is the highest-value thing in the repository. Examples
   are compiled, so they are current in a way prose is not, and they show the shape of real
   usage rather than the shape of the API surface.
3. **The changelog** — only if you are upgrading, or if the README and examples disagree.

Generated protocol or schema code is part of the API too. `at-crate out <crate>` lists the
build-script `out` directories; `at-crate slice <crate>/<path>:1-60 --build` reads one, and
`at-crate search <pat> <crate> --build` searches them. When a crate has several build directories,
the commands name `--build-dir` rather than picking the newest — use it.

Before the crate is installed — deciding whether to adopt it at all — read the repository rather
than a docs site: take the README from `raw.githubusercontent.com`, because a rendered docs page is
whole-page HTML (235–539 kB measured), and fetch one file at a time.

## 3. Keep the research cheap

- **One crate at a time.** `cargo info` is cheap enough to batch; page fetches are not, and
  parallel fetches multiply the bytes before you have read any of them.
- **A single tool result over ~50 kB is a stop sign.** It is roughly 12k tokens, and you will
  not read most of it.
- **Bound every read.** A line range or a `grep` beats a whole file; for generated or minified
  data, check the size first and use `jq` or a byte bound.
- **Never fetch a binary asset.** It arrives as text and pays full price.

## 4. Write a cheat sheet

If the crate is being adopted, write a cheat sheet into `references/crates/<crate>-<version>.md`
in the playbook, from the template at `references/templates/crate-cheatsheet.md`.

Write it **for an agent that was not trained on this version.** Start with `## At a glance`:
5–10 lines saying what changed and which task the sheet answers. Then keep the body task-shaped. Do
not paste an API listing.

The file is cheap on disk and expensive in context. Target **≤ 200 lines and ≤ 8 KB**; if it needs
more, split it by task rather than writing a longer dump. An agent should read the at-a-glance and
the one section it needs, not the whole file.

Name the file with the exact version. A cheat sheet for `liquid-0.26` is wrong the moment
`0.27` changes a signature, and the filename is what makes that visible.

## 5. Add it to the catalogue

Add a row to `references/crates/CATALOG.md` with the crate, version, filename, first section to
read and one-line purpose. An orphan reference is a reference nobody finds.

## 6. Cite the version in the code

Pin the version you researched in `Cargo.toml`. A cheat sheet without a pin is a claim about
`latest`, which is not a version.

## Reading an existing reference

Read `references/crates/CATALOG.md` first, then one file and one section at a time:

```sh
at-peek stat references/crates/<file>.md
at-peek slice references/crates/<file>.md:1-25
at-peek search '<symbol>' references/crates/<file>.md
```

Never load several references in one turn.

## When a session has already blown up

`zed-thread-audit --explain <thread-id-or-title>` lists the largest tool results with the tool,
message index and request argument — never the body. It is in `agent-playbook` under
`tools/zed-thread-tools`; run it as
`direnv exec . cargo run -p zed-thread-tools --bin zed-thread-audit -- --explain <thread>`.
The id comes from `zed-thread-audit` with no arguments. Find the page that caused the compaction
before starting over, so the next run does not fetch it again.
