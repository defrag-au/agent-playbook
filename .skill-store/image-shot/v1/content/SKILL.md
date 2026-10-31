---
name: image-shot
description: Use when you need to inspect, crop, resize, compose or diff an image — a screenshot, widget story, game capture or art asset — instead of Python/Pillow, ImageMagick, sips or ffmpeg. Runs image-shot, writes a derived view under .tmp, and reads the PNG plus its text or JSON report.
argument-hint: <image or scenario file, e.g. .tmp/capture.png>
---

# image-shot

Image work without a hand-written resize script. `image-shot` observes an image, derives a view,
compares two views, or composes several operations in one scenario. It writes only under its
artifacts directory and never over an input.

It is a `*-shot` tool, not an `at-*` tool: it has a write path. Do not use it as a reason to add
`at-image`.

## Use the tool, not PIL or ImageMagick

Do not reach for `python` + Pillow, ImageMagick, `sips` or `ffmpeg` for crop, resize, format
conversion, contact sheets or pixel diffs. The shell commands differ on crop ordering (`sips` is
height-first, `ffmpeg` is `w:h:x:y`, Pillow is `(x1,y1,x2,y2)`); this tool has one order.

`image-shot` is packaged by `agent-playbook`. If it is on `PATH`, use it directly:

```sh
which image-shot
```

If that is empty, the defrag shell has not picked up the image tooling yet. Use the built crate
rather than installing a replacement:

```sh
# in agent-playbook
direnv exec . cargo run -p image-shot -- <args>

# from another repository with cargo on PATH
direnv exec . cargo run --manifest-path ~/code/defrag/agent-playbook/Cargo.toml -p image-shot -- <args>
```

If neither works, say the shell lacks the tool and stop. Do not substitute PIL or ImageMagick
silently.

Keep scratch images under `.tmp/` in the repository that owns them.

## Observe before deriving

```sh
image-shot info .tmp/capture.png          # format, dimensions, colour type, bytes — header only
image-shot stats .tmp/capture.png         # mean/sd, alpha, colours, ink bbox, sharpness
```

`info` does not decode pixels. `stats` is the text answer for a question the eye cannot settle
(how many colours, whether there is alpha, where the ink ends).

Then **look at the image**. Use the editor's image reading on the file; the numbers are not a
substitute for seeing it.

## Derive one view

```sh
image-shot crop  .tmp/capture.png --box 0,0,780,200 --zoom 2
image-shot fit   .tmp/capture.png --edge 24
image-shot cover .tmp/capture.png --size 1200,630
image-shot square .tmp/capture.png --size 256
image-shot render .tmp/capture.png full/400,/0/gray.png
```

- `--box` is **X,Y,W,H**, top-left origin, every time.
- `--zoom N` is an integer scale. It uses nearest-neighbour by default so pixels stay blocks; pass
  `--smooth` (or `--filter lanczos3`) for a photo.
- `fit` never upscales without `--upscale`. `fit --edge 24` on an image already within 24 px reports
  that and writes nothing.
- `render` takes the full IIIF operation string: `region/size/rotation/quality.format`.
- Writes go under `--artifacts` (default `.tmp/image-shot`). With no `--out`, the name is derived
  from the source bytes and the canonical operation, so the same request is the same file.
- A source with alpha written as JPEG needs `--background RRGGBB`; `to_rgb8()` would drop alpha.
- The report is `in/op/out/notes`. Read `notes`: a clamped region, an aspect distortion, a
  flattened alpha channel.

## Compare two views

```sh
image-shot diff .tmp/before.png .tmp/after.png --tolerance 2
image-shot diff .tmp/before.png .tmp/after.png --out diffs/change.png
```

Exit `0` means identical, `2` means different, `1` is an operational error. The report gives changed
pixels, percentage, max/mean delta and the bounding box. `--out` writes a review image: the source
dimmed, changed pixels in red.

A dimension mismatch is refused. Crop or fit both sides to the same size first; do not let a tool
silently stretch one of them.

## Compose a scenario

For more than one view, write a scenario instead of a shell pipeline. It is data, not a script: no
shell, no network action.

```sh
image-shot run   --scenario .tmp/review.dsl
image-shot check --scenario .tmp/review.dsl
image-shot fmt   --scenario .tmp/review.dsl --write
```

```text
run artifacts=.tmp/review-run from=.tmp/capture-run

source home from=page/home.png
crop  home box=0,0,390,120 as=top out=crops/top.png
fit   top edge=24 as=top24 out=crops/top-24.png
sheet cols=2 cell=64x64 from=top24,home out=sheets/home.png
overlay home layer=top x=10 y=20 as=marked out=out/marked.png
trim  marked as=trimmed out=out/trimmed.png
stats trimmed
diff  home marked out=diffs/marked.png
assert same home marked max=0.001 message="the header did not move"
assert size top eq=390x120
```

Actions: `source`, `render`, `fit`, `crop`, `cover`, `square`, `trim`, `sheet`, `overlay`, `stats`,
`diff`, `assert` (`size`, `alpha`, `same`, `different`, `colours`).

- `source` takes a name and `from=`; later actions refer to that name.
- `as=` binds a derived image for later steps; `out=` writes it under the artifacts directory.
- `from=` on the `run` line is the source base; `out=` is always relative to the artifacts
  directory.
- `sheet` makes a contact sheet (`cols`, `cell=WxH`, `from=a,b,c`) and does not upscale a cell.
- `overlay` composites a named layer over a base at `x,y` with alpha.
- `trim` removes a border matching the top-left pixel within `tolerance`.
- A failed assertion does not stop the run. `run.txt` lists every failure and `run.json` carries the
  same data; exit `2`. An operational error exits `1` and stops at the failing step.
- `check` validates the whole file without reading an image. `fmt` canonicalises layout and drops
  comments; `--check` exits `2` when it would rewrite.

Full grammar: `~/code/defrag/agent-playbook/tools/image-shot/SCENARIO.md`.

## What it refuses

- PNG, JPEG, GIF and WebP decode. BMP, TIFF, SVG, AVIF, HEIC and PDF are refused by name. Do not
  quietly convert with another tool; report the unsupported format and ask.
- Arbitrary rotation: only 0, 90, 180 and 270.
- Lossy WebP: this build is lossless only.
- Images over 64 MP are refused before pixel allocation. `--max-pixels` is a deliberate override,
  not a default to raise.
- Animated GIF is a single frame. EXIF/ICC metadata is not preserved.
- `out` may not escape the artifacts directory, contain `..`, name an input, or be `run.txt` /
  `run.json`.

## Report what you observed

- Say which `image-shot` command ran and where it wrote.
- Look at the produced PNG before describing it. A successful command is not a rendered review.
- If a crop looks wrong, use `info`/`stats` on the result and the source rather than guessing.
- Do not claim a scenario passed unless `run` exited `0` and you read `run.txt`; a failed assertion
  exits `2` and still writes the report.

Reference: `~/code/defrag/agent-playbook/tools/image-shot/README.md` and `SCENARIO.md` beside it.
