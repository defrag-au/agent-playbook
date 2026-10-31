# image-shot

Inspect, derive and compare image views, using the IIIF Image API's five-axis operation grammar.

`image-shot` is the sibling of `browser-shot` and `tui-shot`: it owns the files it reads and the
files it writes, and it exists so an agent does not have to assemble `sips`, `ffmpeg`, ImageMagick
or Pillow command lines to answer "what does this image look like, and what changed?".

It is **not** an `at-*` tool. Every `at-*` binary has no write path; `image-shot` writes derived
views. Keeping it in the `*-shot` family keeps that promise intact while giving image inspection the
same shape as the other capture tools.

## Why the IIIF grammar

The IIIF worker in `cnft.dev-workers` already has a precise, composable request grammar. A request
is one point in five independent axes, applied in a fixed order:

```text
region/size/rotation/quality.format
```

The order is the contract. It makes a request reproducible, gives the report a canonical string to
print, and makes the derived filename stable. `browser-shot`'s seam with a page is a URL; this
tool's seam with an image is the same operation string a IIIF URL carries.

## Usage

```sh
image-shot info  .tmp/capture.png
image-shot stats .tmp/capture.png
image-shot crop  .tmp/capture.png --box 0,0,780,200 --zoom 2
image-shot fit   .tmp/capture.png --edge 24
image-shot render .tmp/capture.png full/400,/0/gray.png
image-shot diff  .tmp/before.png .tmp/after.png --tolerance 2
```

```
image-shot <verb> [options]

OBSERVE
  info   <src>...                         header only: format, dimensions, colour type, size
  stats  <src>...                         mean/sd, alpha, colours, ink bbox, sharpness
  diff   <a> <b> [--out <path>] [--tolerance N]
                                          0 identical · 1 different · 2 error

DERIVE
  render <src> <op> [--out <path>]        the full IIIF operation grammar
  fit    <src> --edge N [--upscale]       scale down to fit N px, preserving aspect ratio
  crop   <src> --box X,Y,W,H [--zoom N]   crop, then zoom by an integer factor
  cover  <src> --size W,H [--upscale]     centre-crop to the target aspect, then exact size
  square <src> --size N [--upscale]       centred square region, N px

COMMON
  --artifacts DIR        where derived views are written (default .tmp/image-shot)
  --max-pixels N         decode/encode cap (default 64000000)
  --format F             png (default), jpg, webp or gif, for the intent verbs
  --filter F             nearest, lanczos3 or catmullrom
  --compression N        JPEG quality, 1-100 (default 85)
  --background RRGGBB    required when a source with alpha is written as JPEG
```

`--box` is always **X,Y,W,H**, with the top-left origin. It does not change meaning between tools,
which is the point: `sips` is height-then-width with a y-then-x offset, `ffmpeg` is `w:h:x:y`, and
Pillow is `(x1,y1,x2,y2)`. One order, everywhere.

## The operation string

| Axis | Forms |
| --- | --- |
| `region` | `full` · `square` · `x,y,w,h` · `pct:x,y,w,h` |
| `size` | `max` · `^max` · `w,` · `,h` · `w,h` · `^w,h` · `pct:n` |
| `rotation` | `0`–`360`, or `!n` to mirror horizontally first |
| `quality` | `default` · `color` · `gray` · `bitonal` |
| `format` | `png` · `jpg` · `webp` · `gif` · `auto` |

Trailing segments may be omitted:

```sh
image-shot render .tmp/capture.png 0,0,400,200/800,
# op 0,0,400,200/800,/0/default.png
```

`quality` is **colour**, not compression. `gray` and `bitonal` are the IIIF meanings; JPEG quality
is `--compression`. Rotation is right angles only — `45°` is refused by name rather than silently
approximated.

## Artifacts and naming

The tool owns one directory. `--out` is relative to it, may not contain `..`, and may not name the
source. The directory is created if needed, and its resolved path is checked again after the parent
is canonicalised, so a symlinked subdirectory cannot point the write outside.

With no `--out`, the name is derived from the source bytes, the canonical operation and the tool
version:

```text
<stem>-<hash12>.<ext>
```

The same request on the same bytes is the same file. A changed source is a different file. Nothing
is ever written over an input.

## What a render writes

`render`, `fit`, `crop`, `cover` and `square` print the operation and what happened:

```text
in     .tmp/og-image.png · png · 1200x630 · 20.6 KiB
op     0,0,600,300/1200,600/0/default.png
out    .tmp/image-shot/crops/og-top.png · png · 1200x600 · 19.1 KiB
notes  filter nearest · source unchanged · metadata dropped (no EXIF/ICC preservation)
```

The notes are part of the report: a clamped region, an aspect-ratio distortion, a flattened alpha
channel, a lossless-WebP limitation, or a no-op request all appear there rather than being inferred
from the output.

`diff` is the observation half:

```text
a      .tmp/og-image.png · 1200x630
b      .tmp/out/renders/og-gray.png · 1200x630
result different · 745061 px (98.55%) · max Δ 114 · mean Δ 5.63
bbox   0,0–1199,629
```

With `--out` it also writes a review image: the source dimmed, changed pixels in red. Exit `0` means
identical, `1` means different, `2` is an operational error. A diff of two images with different
dimensions is refused rather than resized implicitly.

`stats` reports mean and standard deviation per channel, the alpha fraction, the number of colours
(capped at 4096), an ink bounding box, and the variance of a 3×3 Laplacian as a rough sharpness
number. It is an observation aid, not a focus score.

## What it refuses

- **Unsupported containers.** PNG, JPEG, GIF and WebP decode. BMP, TIFF, SVG, AVIF, HEIC and PDF are
  named in the error instead of producing a guess or a blank image.
- **Large images.** The header is read before pixel allocation, and anything over `--max-pixels`
  (64 MP by default) is refused with the dimensions and the cap. There is no scaled decode in this
  build; the refusal is the large-image story.
- **Arbitrary rotation.** Only 0, 90, 180 and 270 are applied.
- **JPEG from an alpha source without `--background`.** `to_rgb8()` drops alpha; flattening is an
  explicit choice, not a default.
- **Writes outside the artifacts directory**, `..` in `--out`, and an `--out` that names an input.

## Limits

- **No EXIF or ICC preservation.** The report says so on every write. Metadata is not copied.
- **WebP is lossless only** in the `image` build this tool links; lossy WebP is not offered rather
  than being silently encoded differently.
- **GIF output is a single frame.** The report says so when it applies.
- **`auto` resolves to PNG.** A derived view is for inspection, and lossless is the useful default;
  there is no art-kind classifier in this build.
- **The decode cap is a decode cap, not a streaming decoder.** The file is read once, its header is
  inspected, and a source over the cap is refused before a pixel buffer is allocated. A future
  scaled-decode path would be a separate change.

## Dependencies

Two: `anyhow`, and `image` with `default-features = false` and only `png`, `jpeg`, `gif` and `webp`.
No native libraries, no `libvips`, no `sha2` (the derived name uses FNV-1a), no `serde` (the report
is text). The build stays cheap enough to install beside the other `*-shot` tools.

## Build and test

```sh
direnv exec . cargo test -p image-shot
direnv exec . cargo clippy -p image-shot --all-targets --all-features -- -D warnings
```

The crate has unit tests for the grammar, containment, transform order, alpha flattening, the pixel
cap, statistics and diff. `tests/cli.rs` drives the built binary against generated fixtures for
`info`, `crop`, `diff` and the two output-containment refusals. There is no browser integration
test: a `browser-shot` capture is an ordinary PNG, and so is a test fixture.

## Why not a `browser-shot` sub-DSL

A browser capture is already an image on disk. Folding this grammar into `browser-shot`'s scenario
file would couple a page-driving tool to an image tool: `browser-shot` would have to know every
image axis, and an image-only review could not run without a browser. The seam is the artifacts
directory, which both tools already own and which a reader can inspect without either tool.

The planned integration is additive and one-directional: `browser-shot` writes a typed `captures`
array naming each capture's peer, step, path and sidecar in `run.json`; `image-shot` learns a
`source` step that resolves a capture by that name. Until then, a scenario repeats the capture's
path, and that is enough to review a screen.

## Not in this build

A scenario DSL (`run --scenario`) that composes several operations, writes `run.txt` / `run.json`,
and carries values between steps is designed but not implemented. The per-image verbs above are the
operations it will call; no grammar is promised here before it exists.
