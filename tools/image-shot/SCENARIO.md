# image-shot scenario DSL

`run`, `check` and `fmt` accept a line-oriented scenario. It is the same shape as `browser-shot`
and `tui-shot`: one action per line, positional names first, then `key=value` fields. The whole
file is parsed and every name is checked before an image is read, so a typo fails before anything is
written.

A scenario is data, not a script. There is no shell, no expression language and no network action.

```sh
image-shot run --scenario .tmp/review.dsl
image-shot check --scenario .tmp/review.dsl
image-shot fmt --scenario .tmp/review.dsl --write
```

## Grammar

```text
file      = { line }
line      = blank | comment | run | step
run       = "run" { field }
step      = source | render | fit | crop | cover | square | trim | sheet | overlay | stats | diff | assert
source    = "source" name "from=" value
render    = "render" name "op=" value { field }
fit       = "fit" name "edge=" value [ "upscale" ] { field }
crop      = "crop" name "box=" value [ "zoom=" value ] [ "smooth" ] { field }
cover     = "cover" name "size=" value [ "upscale" ] { field }
square    = "square" name "size=" value [ "upscale" ] { field }
trim      = "trim" name [ "tolerance=" value ] { field }
sheet     = "sheet" "cols=" value "cell=" value "from=" names { field }
overlay   = "overlay" name "layer=" name [ "x=" value ] [ "y=" value ] { field }
stats     = "stats" name
diff      = "diff" name name [ "out=" value ] [ "tolerance=" value ]
assert    = "assert" assertion { "message=" value }
assertion = "size" name "eq=" size
          | "alpha" name ( "gt=" n | "lt=" n | "eq=" n )
          | "same" name name "max=" fraction [ "tolerance=" value ]
          | "different" name name "min=" fraction [ "tolerance=" value ]
          | "colours" name ( "gt=" n | "lt=" n | "eq=" n )
field     = key "=" value
```

The `run` line is first and appears exactly once. A scenario has at least one step. A name is ASCII
letters, digits, `_` or `-`, and must be defined by an earlier step before it is referenced.

Values may be single- or double-quoted; a quoted value may contain spaces, `#` and `=`. Escapes inside
a quoted value are `\n`, `\t`, `\\`, `\"` and `\'`; an unknown escape is an error. A `#` at the start
of an unquoted token begins a comment; a `#` inside a token is kept, so a URL fragment is safe.

Sizes are `WxH` (or `W,H`). A region is `X,Y,W,H`, top-left origin. Fractions are `0`–`1`; alpha is a
percentage `0`–`100`. `max-pixels` accepts a suffix: `64M`, `64m`, `64000000`.

## `run`

| Field | Meaning |
| --- | --- |
| `artifacts` | output directory, relative to the working directory (default `.tmp/image-shot`) |
| `from` | source directory, relative to the working directory (default the working directory) |
| `max-pixels` | decode/output cap for every image (default `64M`) |

The CLI can override all three: `--artifacts`, `--from`, `--max-pixels`. A relative `source from=`
is resolved against `from`; every `out=` is resolved against `artifacts`.

## Output fields

These fields are accepted by `render`, `fit`, `crop`, `cover`, `square`, `trim`, `sheet` and
`overlay`:

| Field | Meaning |
| --- | --- |
| `as` | bind the result to this name for a later step |
| `out` | write the result under the artifacts directory |
| `format` | `png` (default), `jpg`, `webp` or `gif`; `render` takes its format from `op=` instead |
| `filter` | `nearest`, `lanczos3` (default) or `catmullrom` |
| `compression` | JPEG quality, 1–100 (default 85) |
| `background` | `RRGGBB` or `#RRGGBB`; required to write an alpha image as JPEG |

At least one of `as` or `out` is required. `out` may not contain `..`, may not name a source, and
may not be `run.txt` or `run.json`. `as` may rebind an existing name.

## Actions

### `source`

```text
source home from=page/home.png
```

Reads and decodes the image once. The report carries its format, dimensions and byte size.

### `render`

```text
render home op=0,0,390,120/780,240/90/gray.png as=top out=crops/top.png
```

The full IIIF operation string from the one-shot `render` verb: `region/size/rotation/quality.format`.
`format=` is rejected here because the operation already names the output format.

### `fit`

```text
fit home edge=24 as=small out=crops/small.png
```

Fits the longest side within `edge`, preserving aspect ratio. Without `upscale`, an image already
within `edge` is left at its original size; with it, the image grows.

### `crop`

```text
crop home box=0,0,390,120 zoom=2 as=top out=crops/top.png
```

Crops `X,Y,W,H`, then scales by an integer `zoom` (default 1). `zoom` above 1 uses nearest-neighbour
by default so pixels stay blocks; pass `smooth` or an explicit `filter=` for a smooth scale.

### `cover`

```text
cover home size=1200x630 as=cover out=covers/home.png
```

Centre-crops to the target aspect ratio, then writes exactly `size`. Without `upscale`, a target
larger than the cropped region is refused.

### `square`

```text
square home size=256 as=square out=out/covers/home-square.png
```

Takes the centred square of the shorter side, then scales to `size`. Without `upscale`, a size larger
than the shorter side is refused.

### `trim`

```text
trim home tolerance=8 as=trimmed out=out/trimmed.png
```

Removes a border matching the top-left pixel within `tolerance` (default 8) on every channel. An image
with no border is passed through and the report says `nothing to trim`.

### `sheet`

```text
sheet cols=2 cell=320x180 from=top240,icon as=sheet out=sheets/both.png
```

A contact sheet. Each source is scaled to fit a `cell` without upscaling and centred; the canvas is
`cols × ceil(n/cols)` cells. The canvas is transparent by default; pass `background=` for an opaque
sheet, and it is required if `format=jpg`.

### `overlay`

```text
overlay home layer=top x=40 y=40 as=marked out=out/marked.png
```

Composites `layer` over `base` at `x,y` (default 0,0), blending alpha. Negative offsets are allowed
and clip at the base edges.

### `stats`

```text
stats trimmed
```

Measures the image and puts the full statistics in `run.json`: mean/sd per channel, alpha percent,
unique colours (capped at 4096), ink bounding box and sharpness. `run.txt` carries the summary.

### `diff`

```text
diff home marked out=diffs/marked.png tolerance=2
```

Compares two images of the same dimensions. With `out=`, writes the review image (dimmed source,
changed pixels in red). A dimension mismatch is an operational error; use `assert same` when the
mismatch should be a failed check.

### `assert`

```text
assert size top eq=1200x600
assert alpha home gt=0
assert same home marked max=0.001 message="the header did not move"
assert different home marked min=0.001
assert colours home lt=5000
```

| Check | Meaning |
| --- | --- |
| `size NAME eq=WxH` | exact dimensions |
| `alpha NAME gt=n\|lt=n\|eq=n` | percentage of pixels with alpha below 255 |
| `same A B max=fraction` | at most this fraction of pixels changed |
| `different A B min=fraction` | at least this fraction changed |
| `colours NAME gt=n\|lt=n\|eq=n` | unique colours; `4096` means the counter was capped |

`tolerance=` on `same`/`different` ignores per-pixel channel deltas up to that value. A failed
assertion does not stop the run; it is recorded and every later step still runs. `run` exits `2`.

## Report

`run` writes two files under the artifacts directory:

| File | Contents |
| --- | --- |
| `run.txt` | one line per step, the failure reason and notes, then the result |
| `run.json` | the same data typed: scenario, artifacts, from, max-pixels, result, steps, outputs, failures |

Each step records its line, action, bound name, output, status, dimensions, notes, and any stats or
diff measurement. `outputs` names every `out=` path in order. `failures` repeats the failed checks
with their line and reason. A failed step's `message` is the scenario's `message=` label; the
machine-readable reason is in `failure`.

`run --json` prints `run.json` to stdout instead of `run.txt`; the files are written either way.
Progress lines go to stderr.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | every step ran and every assertion held |
| `2` | a scenario parsed and ran, but an assertion failed (`fmt --check` also uses `2` for "would change") |
| `1` | operational error: bad grammar, unreadable source, failed write |

A parse error happens before the artifacts directory is touched. An operational error during a run
still writes the partial report.

## Checking and formatting

`check` parses the whole file and validates every name, then prints `ok N steps`. It reads no image
and writes nothing. `fmt` prints the canonical form; `--write` rewrites the file; `--check` exits `2`
if it would change. Formatting drops comments and blank lines, writes fields in a fixed order, and
omits defaults (`zoom=1`, trim `tolerance=8`, diff `tolerance=0`, zero offsets).

## Complete example

```text
run artifacts=.tmp/review-run from=.tmp/capture-run max-pixels=64M

source home from=page/home.png
source base from=page/base.png
crop  home box=0,0,390,120 as=top out=crops/top.png
fit   top edge=24 as=top24 out=crops/top-24.png
sheet cols=2 cell=64x64 from=top24,base out=sheets/home.png
overlay home layer=top x=10 y=20 as=marked out=out/marked.png
trim  marked as=trimmed out=out/trimmed.png
stats trimmed
diff  home marked out=diffs/marked.png
assert size top eq=390x120
assert same home base max=0.001 message="the header did not move"
assert different home marked min=0.001
```

This writes `crops/`, `sheets/`, `out/` and `diffs/` under `.tmp/review-run`, plus `run.txt` and
`run.json`.

## Not in this build

- `annotate`: text rendering needs a font dependency or a hand-built bitmap font.
- Arbitrary rotation: `rotation` accepts 0, 90, 180 and 270 only.
- Lossy WebP, AVIF, HEIC, PDF and animated GIF.
- Reading a `browser-shot` `run.json` by capture name. A scenario repeats the capture's path today;
  `source` from `page/home.png` is the seam that lookup will use.
