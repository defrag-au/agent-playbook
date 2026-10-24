---
name: crate-research
description: Use when adopting a crate, checking whether a suggested crate exists, or working with a crate whose API may be newer than training data. Researches the real current version and writes a cheat sheet so later sessions don't have to guess.
---

# Researching a crate

Use this before adding a dependency, and whenever you are about to write code against a crate
whose API you are recalling from memory rather than reading.

## 1. Find the real current version

Consult **crates.io**, not a search engine:

```
https://crates.io/crates/<crate>
```

The summary page is authoritative for the version, and it links the source repository. An
internet search will happily return a blog post about an API that was renamed two majors ago.

Once the crate is in the project, the version that matters is the one `Cargo.lock` chose — not
the latest one. `at-crate where <crate>` names it, the extracted source directory, and any
build-script output. `at-crate installed <crate>` lists every extracted version when the crate
is not in this project's lock.

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

## 3. Write a cheat sheet

If the crate is being adopted, write a cheat sheet into `references/crates/<crate>-<version>.md`
in the playbook, from the template at `references/templates/crate-cheatsheet.md`.

Write it **for an agent that was not trained on this version.** Be as detailed as the crate
needs — this file is cheap, and the alternative is every future session rediscovering the same
renames.

Name the file with the exact version. A cheat sheet for `liquid-0.26` is wrong the moment
`0.27` changes a signature, and the filename is what makes that visible.

## 4. Cite the version in the code

Pin the version you researched in `Cargo.toml`. A cheat sheet without a pin is a claim about
`latest`, which is not a version.
