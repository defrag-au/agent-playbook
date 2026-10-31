//! `image-shot` — derive, inspect and compose image views.
//!
//! One-shot verbs are the single-operation entry point; `run --scenario` will be the composition
//! layer on top of the same operations. This build has the one-shot half.

mod artifact;
mod dsl;
mod inspect;
mod op;
mod scenario;
mod source;
mod transform;

use std::path::Path;

use anyhow::{bail, Context, Result};

use artifact::Artifacts;
use op::{Format, Op, Quality, Region, Rotation, Size};
use source::Source;
use transform::{Filter, RenderOptions};

const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The ceiling `max` and `^max` fit into. Explicit sizes are bounded by the pixel cap instead.
pub(crate) const MAX_DIMENSION: u32 = 4096;
pub(crate) const DEFAULT_MAX_PIXELS: u64 = 64_000_000;

const HELP: &str = r#"image-shot — derive, inspect and compose image views

USAGE
  image-shot <verb> [options]

OBSERVE
  info   <src>...                         header only: format, dimensions, colour type, size
  stats  <src>...                         mean/sd, alpha, colours, ink bbox, sharpness
  diff   <a> <b> [--out <path>] [--tolerance N]
                                          0 identical · 2 different · 1 error

DERIVE
  render <src> <op> [--out <path>]        the full IIIF operation grammar
  fit    <src> --edge N [--upscale]       scale down to fit N px, preserving aspect ratio
  crop   <src> --box X,Y,W,H [--zoom N]   crop, then zoom by an integer factor
  cover  <src> --size W,H [--upscale]     centre-crop to the target aspect, then exact size
  square <src> --size N [--upscale]       centred square region, N px

COMPOSE
  run    --scenario <file> [--from DIR] [--json]
                                          run a scenario; writes run.txt and run.json
  check  --scenario <file>                parse and validate; reads no image
  fmt    --scenario <file> [--write|--check]
                                          print, rewrite, or check the canonical form

COMMON
  --artifacts DIR        where derived views are written (default .tmp/image-shot)
  --max-pixels N         decode/encode cap (default 64000000)

DERIVE OPTIONS
  --format F             png (default), jpg, webp or gif
  --filter F             nearest, lanczos3 or catmullrom
  --smooth               crop --zoom uses a smooth filter instead of nearest
  --compression N        JPEG quality, 1-100 (default 85)
  --background RRGGBB    required when a source with alpha is written as JPEG

THE OPERATION STRING
  region/size/rotation/quality.format, applied in that order:
    region    full | square | x,y,w,h | pct:x,y,w,h
    size      max | ^max | w, | ,h | w,h | ^w,h | pct:n
    rotation  0-360, prefix ! to mirror first
    quality   default | color | gray | bitonal      (colour, not compression)
    format    png | jpg | webp | gif | auto

  Trailing segments may be omitted: 0,0,400,200/800, expands to
  0,0,400,200/800,/0/default.png and the report always prints the full form.
  max and ^max stop at 4096 px; an explicit size is bounded by --max-pixels.

EXAMPLES
  image-shot info .tmp/settled.png
  image-shot crop .tmp/ui-390.png --box 0,0,780,200 --zoom 2
  image-shot fit .tmp/vell.png --edge 24
  image-shot render .tmp/shot.png full/400,/0/gray.png
  image-shot diff .tmp/before.png .tmp/after.png --tolerance 2
  image-shot run --scenario .tmp/review.dsl
  image-shot check --scenario .tmp/review.dsl

  --out is relative to the artifacts directory and may not contain `..`.
  With no --out, the name is derived from the source bytes and the canonical op.
  A scenario's actions and fields are in SCENARIO.md beside this tool.
"#;

fn main() {
    let code = match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("image-shot: {error:#}");
            1
        }
    };
    std::process::exit(code);
}

fn run() -> Result<i32> {
    let cwd = std::env::current_dir().context("read the working directory")?;
    let cwd = std::fs::canonicalize(&cwd).context("resolve the working directory")?;

    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        print!("{HELP}");
        return Ok(0);
    }
    if take_switch(&mut args, "--help") || take_switch(&mut args, "-h") {
        print!("{HELP}");
        return Ok(0);
    }
    if take_switch(&mut args, "--version") {
        println!("image-shot {VERSION}");
        return Ok(0);
    }

    let artifacts_dir = take_value(&mut args, "--artifacts")?;
    let max_pixels_override = match take_value(&mut args, "--max-pixels")? {
        Some(value) => Some(parse_pixels(&value)?),
        None => None,
    };
    let max_pixels = max_pixels_override.unwrap_or(DEFAULT_MAX_PIXELS);
    if max_pixels == 0 {
        bail!("--max-pixels must be positive");
    }

    let Some(verb) = args.first().cloned() else {
        bail!("no verb; run `image-shot --help`");
    };
    args.remove(0);

    match verb.as_str() {
        "info" => inspect::info(&args, &cwd),
        "stats" => stats_cmd(&args, &cwd, max_pixels),
        "diff" => diff_cmd(&args, &cwd, max_pixels, artifacts_dir.as_deref()),
        "render" => render_cmd(&args, &cwd, max_pixels, artifacts_dir.as_deref()),
        "fit" => fit_cmd(&args, &cwd, max_pixels, artifacts_dir.as_deref()),
        "crop" => crop_cmd(&args, &cwd, max_pixels, artifacts_dir.as_deref()),
        "cover" => cover_cmd(&args, &cwd, max_pixels, artifacts_dir.as_deref()),
        "square" => square_cmd(&args, &cwd, max_pixels, artifacts_dir.as_deref()),
        "run" => scenario::run_cmd(&args, &cwd, artifacts_dir.as_deref(), max_pixels_override),
        "check" => scenario::check_cmd(&args, &cwd),
        "fmt" => scenario::fmt_cmd(&args, &cwd),
        "help" => {
            print!("{HELP}");
            Ok(0)
        }
        other => bail!("unknown verb `{other}`; run `image-shot --help`"),
    }
}

fn stats_cmd(args: &[String], cwd: &Path, max_pixels: u64) -> Result<i32> {
    if args.is_empty() {
        bail!("stats needs at least one source");
    }
    for (index, given) in args.iter().enumerate() {
        if index > 0 {
            println!();
        }
        let source = Source::read(given, cwd, max_pixels)?;
        let stats = inspect::stats(&source)?;
        inspect::print_stats(&source.label, &stats);
    }
    Ok(0)
}

fn diff_cmd(
    args: &[String],
    cwd: &Path,
    max_pixels: u64,
    artifacts_dir: Option<&str>,
) -> Result<i32> {
    let mut args = args.to_vec();
    let out = take_value(&mut args, "--out")?;
    let tolerance = match take_value(&mut args, "--tolerance")? {
        Some(value) => value
            .parse::<u8>()
            .map_err(|_| anyhow::anyhow!("--tolerance: `{value}` is not 0-255"))?,
        None => 0,
    };
    if args.len() != 2 {
        bail!("diff needs exactly two sources");
    }

    let a = Source::read(&args[0], cwd, max_pixels)?;
    let b = Source::read(&args[1], cwd, max_pixels)?;
    let a_image = a.decode()?;
    let b_image = b.decode()?;
    let result = inspect::diff(&a_image, &b_image, tolerance)?;

    println!("a      {} · {}x{}", a.label, a.width, a.height);
    println!("b      {} · {}x{}", b.label, b.width, b.height);
    if result.changed == 0 {
        println!(
            "result identical · 0 px differ · mean Δ {:.2}",
            result.mean_delta
        );
    } else {
        let bbox = result.bbox.map_or_else(
            || "none".to_owned(),
            |(x0, y0, x1, y1)| format!("{x0},{y0}–{x1},{y1}"),
        );
        println!(
            "result different · {} px ({:.2}%) · max Δ {} · mean Δ {:.2}",
            result.changed,
            result.fraction(),
            result.max_delta,
            result.mean_delta
        );
        println!("bbox   {bbox}");
    }

    if let Some(out) = out {
        if result.changed == 0 {
            println!("out    not written — the images are identical");
        } else {
            let artifacts = Artifacts::new(artifacts_dir, cwd)?;
            let derived = artifact::derived_name(Path::new(&args[0]), "diff", "png", &a.bytes);
            let destination = artifacts.resolve(Some(&out), &derived)?;
            if a.is_same_file(&destination) || b.is_same_file(&destination) {
                bail!("--out names an input; a diff view is a new file");
            }
            let view = inspect::diff_view(&a_image, &b_image, tolerance)?;
            std::fs::write(&destination, view)
                .with_context(|| format!("write {}", destination.display()))?;
            println!("out    {}", artifacts.display(&destination));
        }
    }

    Ok(if result.changed == 0 { 0 } else { 2 })
}

fn render_cmd(
    args: &[String],
    cwd: &Path,
    max_pixels: u64,
    artifacts_dir: Option<&str>,
) -> Result<i32> {
    let mut args = args.to_vec();
    let out = take_value(&mut args, "--out")?;
    let filter = take_filter(&mut args)?.unwrap_or(Filter::Lanczos3);
    let compression = compression_option(&mut args)?;
    let background = background_option(&mut args)?;
    if args.len() != 2 {
        bail!("render needs <source> <region/size/rotation/quality.format>");
    }
    let op = Op::parse(&args[1])?;
    render_source(RenderRequest {
        given: &args[0],
        op: &op,
        out: out.as_deref(),
        filter,
        compression,
        background,
        cwd,
        max_pixels,
        artifacts_dir,
    })
}

fn fit_cmd(
    args: &[String],
    cwd: &Path,
    max_pixels: u64,
    artifacts_dir: Option<&str>,
) -> Result<i32> {
    let mut args = args.to_vec();
    let out = take_value(&mut args, "--out")?;
    let edge = take_value(&mut args, "--edge")?
        .ok_or_else(|| anyhow::anyhow!("fit needs --edge N"))?
        .parse::<u32>()
        .map_err(|_| anyhow::anyhow!("--edge: not a number"))?;
    if edge == 0 {
        bail!("--edge must be positive");
    }
    let upscale = take_switch(&mut args, "--upscale");
    let format = format_option(&mut args)?;
    let filter = take_filter(&mut args)?.unwrap_or(Filter::Lanczos3);
    let compression = compression_option(&mut args)?;
    let background = background_option(&mut args)?;
    if args.len() != 1 {
        bail!("fit needs exactly one source");
    }

    let source = Source::read(&args[0], cwd, max_pixels)?;
    let longest = source.width.max(source.height);
    if !upscale && edge >= longest {
        println!(
            "in     {} · {}x{}",
            source.label, source.width, source.height
        );
        println!("note   already within {edge}px; nothing written (pass --upscale to grow it)");
        return Ok(0);
    }

    let (width, height) = fit_dimensions(source.width, source.height, edge);
    let size = if source.width >= source.height {
        Size::Width(width)
    } else {
        Size::Height(height)
    };
    let op = Op {
        region: Region::Full,
        size,
        rotation: Rotation {
            mirror: false,
            degrees: 0.0,
        },
        quality: Quality::Default,
        format,
    };
    render_source(RenderRequest {
        given: &args[0],
        op: &op,
        out: out.as_deref(),
        filter,
        compression,
        background,
        cwd,
        max_pixels,
        artifacts_dir,
    })
}

fn crop_cmd(
    args: &[String],
    cwd: &Path,
    max_pixels: u64,
    artifacts_dir: Option<&str>,
) -> Result<i32> {
    let mut args = args.to_vec();
    let out = take_value(&mut args, "--out")?;
    let box_spec = take_value(&mut args, "--box")?
        .ok_or_else(|| anyhow::anyhow!("crop needs --box X,Y,W,H"))?;
    let (x, y, width, height) = parse_box(&box_spec)?;
    let zoom = match take_value(&mut args, "--zoom")? {
        Some(value) => value
            .parse::<u32>()
            .map_err(|_| anyhow::anyhow!("--zoom: `{value}` is not a positive integer"))?,
        None => 1,
    };
    if zoom == 0 {
        bail!("--zoom must be at least 1");
    }
    let smooth = take_switch(&mut args, "--smooth");
    let format = format_option(&mut args)?;
    let explicit_filter = take_filter(&mut args)?;
    let compression = compression_option(&mut args)?;
    let background = background_option(&mut args)?;
    if args.len() != 1 {
        bail!("crop needs exactly one source");
    }

    let source = Source::read(&args[0], cwd, max_pixels)?;
    if x >= source.width || y >= source.height {
        bail!(
            "--box: {x},{y} starts outside the {}x{} source",
            source.width,
            source.height
        );
    }
    if width > source.width - x || height > source.height - y {
        bail!(
            "--box: {}x{} at {x},{y} extends past the {}x{} source",
            width,
            height,
            source.width,
            source.height
        );
    }
    let out_width = width
        .checked_mul(zoom)
        .ok_or_else(|| anyhow::anyhow!("--zoom: the output dimensions overflow"))?;
    let out_height = height
        .checked_mul(zoom)
        .ok_or_else(|| anyhow::anyhow!("--zoom: the output dimensions overflow"))?;

    let filter = explicit_filter.unwrap_or(if zoom > 1 && !smooth {
        Filter::Nearest
    } else {
        Filter::Lanczos3
    });
    let op = Op {
        region: Region::Absolute {
            x,
            y,
            w: width,
            h: height,
        },
        size: Size::Exact {
            w: out_width,
            h: out_height,
        },
        rotation: Rotation {
            mirror: false,
            degrees: 0.0,
        },
        quality: Quality::Default,
        format,
    };
    render_source(RenderRequest {
        given: &args[0],
        op: &op,
        out: out.as_deref(),
        filter,
        compression,
        background,
        cwd,
        max_pixels,
        artifacts_dir,
    })
}

fn cover_cmd(
    args: &[String],
    cwd: &Path,
    max_pixels: u64,
    artifacts_dir: Option<&str>,
) -> Result<i32> {
    let mut args = args.to_vec();
    let out = take_value(&mut args, "--out")?;
    let size_spec = take_value(&mut args, "--size")?
        .ok_or_else(|| anyhow::anyhow!("cover needs --size W,H"))?;
    let (target_w, target_h) = parse_size_pair(&size_spec)?;
    let upscale = take_switch(&mut args, "--upscale");
    let format = format_option(&mut args)?;
    let filter = take_filter(&mut args)?.unwrap_or(Filter::Lanczos3);
    let compression = compression_option(&mut args)?;
    let background = background_option(&mut args)?;
    if args.len() != 1 {
        bail!("cover needs exactly one source");
    }

    let source = Source::read(&args[0], cwd, max_pixels)?;
    let (x, y, width, height) = cover_region(source.width, source.height, target_w, target_h);
    if !upscale && (target_w > width || target_h > height) {
        bail!(
            "cover: {width}x{height} would be upscaled to {target_w}x{target_h}; \
             pass --upscale if that is intended"
        );
    }
    let op = Op {
        region: Region::Absolute {
            x,
            y,
            w: width,
            h: height,
        },
        size: Size::Exact {
            w: target_w,
            h: target_h,
        },
        rotation: Rotation {
            mirror: false,
            degrees: 0.0,
        },
        quality: Quality::Default,
        format,
    };
    render_source(RenderRequest {
        given: &args[0],
        op: &op,
        out: out.as_deref(),
        filter,
        compression,
        background,
        cwd,
        max_pixels,
        artifacts_dir,
    })
}

fn square_cmd(
    args: &[String],
    cwd: &Path,
    max_pixels: u64,
    artifacts_dir: Option<&str>,
) -> Result<i32> {
    let mut args = args.to_vec();
    let out = take_value(&mut args, "--out")?;
    let size = take_value(&mut args, "--size")?
        .ok_or_else(|| anyhow::anyhow!("square needs --size N"))?
        .parse::<u32>()
        .map_err(|_| anyhow::anyhow!("--size: not a number"))?;
    if size == 0 {
        bail!("--size must be positive");
    }
    let upscale = take_switch(&mut args, "--upscale");
    let format = format_option(&mut args)?;
    let filter = take_filter(&mut args)?.unwrap_or(Filter::Lanczos3);
    let compression = compression_option(&mut args)?;
    let background = background_option(&mut args)?;
    if args.len() != 1 {
        bail!("square needs exactly one source");
    }

    let source = Source::read(&args[0], cwd, max_pixels)?;
    let shorter = source.width.min(source.height);
    if !upscale && size > shorter {
        bail!(
            "square: the source's shorter side is {shorter}px; {size}px would upscale it. \
             Pass --upscale if that is intended"
        );
    }
    let op = Op {
        region: Region::Square,
        size: Size::Width(size),
        rotation: Rotation {
            mirror: false,
            degrees: 0.0,
        },
        quality: Quality::Default,
        format,
    };
    render_source(RenderRequest {
        given: &args[0],
        op: &op,
        out: out.as_deref(),
        filter,
        compression,
        background,
        cwd,
        max_pixels,
        artifacts_dir,
    })
}

struct RenderRequest<'a> {
    given: &'a str,
    op: &'a Op,
    out: Option<&'a str>,
    filter: Filter,
    compression: u8,
    background: Option<[u8; 3]>,
    cwd: &'a Path,
    max_pixels: u64,
    artifacts_dir: Option<&'a str>,
}

fn render_source(request: RenderRequest<'_>) -> Result<i32> {
    let source = Source::read(request.given, request.cwd, request.max_pixels)?;
    let format = request.op.format.resolve();

    let identity = request.op.is_identity()
        && format == source.format
        && source.width <= MAX_DIMENSION
        && source.height <= MAX_DIMENSION;
    if identity {
        println!(
            "in     {} · {}x{}",
            source.label, source.width, source.height
        );
        println!("op     {}", request.op.canonical());
        println!("note   nothing to do: the source already matches this request");
        return Ok(0);
    }

    let image = source.decode()?;
    let rendered = transform::render(
        image,
        request.op,
        &RenderOptions {
            filter: request.filter,
            compression: request.compression,
            background: request.background,
        },
        MAX_DIMENSION,
        request.max_pixels,
    )?;

    let artifacts = Artifacts::new(request.artifacts_dir, request.cwd)?;
    let derived = artifact::derived_name(
        &source.path,
        &request.op.canonical(),
        rendered.format.extension(),
        &source.bytes,
    );
    let destination = artifacts.resolve(request.out, &derived)?;
    if source.is_same_file(&destination) {
        bail!("--out names the source; a derived view is a new file");
    }
    std::fs::write(&destination, &rendered.bytes)
        .with_context(|| format!("write {}", destination.display()))?;

    println!(
        "in     {} · {} · {}x{} · {}",
        source.label,
        source.format,
        source.width,
        source.height,
        inspect::bytes_size(source.bytes.len() as u64)
    );
    println!("op     {}", request.op.canonical());
    println!(
        "out    {} · {} · {}x{} · {}",
        artifacts.display(&destination),
        rendered.format,
        rendered.width,
        rendered.height,
        inspect::bytes_size(rendered.bytes.len() as u64)
    );
    let mut notes = rendered.notes;
    notes.push(format!("filter {}", rendered.filter.as_str()));
    notes.push("source unchanged".to_owned());
    notes.push("metadata dropped (no EXIF/ICC preservation)".to_owned());
    println!("notes  {}", notes.join(" · "));
    Ok(0)
}

pub(crate) fn fit_dimensions(width: u32, height: u32, edge: u32) -> (u32, u32) {
    let scale = f64::from(edge) / f64::from(width.max(height));
    (
        ((f64::from(width) * scale).round() as u32).max(1),
        ((f64::from(height) * scale).round() as u32).max(1),
    )
}

pub(crate) fn cover_region(
    width: u32,
    height: u32,
    target_w: u32,
    target_h: u32,
) -> (u32, u32, u32, u32) {
    let source_ratio = f64::from(width) / f64::from(height);
    let target_ratio = f64::from(target_w) / f64::from(target_h);
    if source_ratio > target_ratio {
        let region_h = height;
        let region_w = ((f64::from(height) * target_ratio).round() as u32).clamp(1, width);
        ((width - region_w) / 2, 0, region_w, region_h)
    } else {
        let region_w = width;
        let region_h = ((f64::from(width) / target_ratio).round() as u32).clamp(1, height);
        (0, (height - region_h) / 2, region_w, region_h)
    }
}

pub(crate) fn parse_box(text: &str) -> Result<(u32, u32, u32, u32)> {
    let parts: Vec<&str> = text.split(',').collect();
    if parts.len() != 4 {
        bail!("--box: `{text}` is not X,Y,W,H");
    }
    let number = |part: &str| -> Result<u32> {
        part.parse::<u32>()
            .map_err(|_| anyhow::anyhow!("--box: `{part}` is not a pixel value"))
    };
    let (x, y, width, height) = (
        number(parts[0])?,
        number(parts[1])?,
        number(parts[2])?,
        number(parts[3])?,
    );
    if width == 0 || height == 0 {
        bail!("--box: width and height must be positive");
    }
    Ok((x, y, width, height))
}

pub(crate) fn parse_size_pair(text: &str) -> Result<(u32, u32)> {
    let (width, height) = text
        .split_once('x')
        .or_else(|| text.split_once(','))
        .ok_or_else(|| anyhow::anyhow!("--size: `{text}` is not W,H or WxH"))?;
    let number = |part: &str| -> Result<u32> {
        part.trim()
            .parse::<u32>()
            .map_err(|_| anyhow::anyhow!("--size: `{part}` is not a pixel value"))
    };
    let (width, height) = (number(width)?, number(height)?);
    if width == 0 || height == 0 {
        bail!("--size: dimensions must be positive");
    }
    Ok((width, height))
}

pub(crate) fn parse_pixels(text: &str) -> Result<u64> {
    let (digits, multiplier) =
        if let Some(rest) = text.strip_suffix('M').or_else(|| text.strip_suffix('m')) {
            (rest, 1_000_000_u64)
        } else if let Some(rest) = text.strip_suffix('K').or_else(|| text.strip_suffix('k')) {
            (rest, 1_000_u64)
        } else if let Some(rest) = text.strip_suffix('G').or_else(|| text.strip_suffix('g')) {
            (rest, 1_000_000_000_u64)
        } else {
            (text, 1_u64)
        };
    let value = digits
        .parse::<u64>()
        .map_err(|_| anyhow::anyhow!("--max-pixels: `{text}` is not a number"))?;
    value
        .checked_mul(multiplier)
        .filter(|value| *value > 0)
        .ok_or_else(|| anyhow::anyhow!("--max-pixels: `{text}` is not a positive size"))
}

fn format_option(args: &mut Vec<String>) -> Result<Format> {
    match take_value(args, "--format")? {
        Some(value) => Format::parse(&value),
        None => Ok(Format::Png),
    }
}

fn take_filter(args: &mut Vec<String>) -> Result<Option<Filter>> {
    take_value(args, "--filter")?
        .map(|value| Filter::parse(&value))
        .transpose()
}

fn compression_option(args: &mut Vec<String>) -> Result<u8> {
    match take_value(args, "--compression")? {
        Some(value) => {
            let quality = value
                .parse::<u8>()
                .map_err(|_| anyhow::anyhow!("--compression: `{value}` is not 1-100"))?;
            if !(1..=100).contains(&quality) {
                bail!("--compression must be between 1 and 100");
            }
            Ok(quality)
        }
        None => Ok(85),
    }
}

fn background_option(args: &mut Vec<String>) -> Result<Option<[u8; 3]>> {
    take_value(args, "--background")?
        .map(|value| transform::parse_background(&value))
        .transpose()
}

pub(crate) fn take_value(args: &mut Vec<String>, name: &str) -> Result<Option<String>> {
    let mut index = 0;
    while index < args.len() {
        if args[index] == name {
            if index + 1 >= args.len() {
                bail!("{name} needs a value");
            }
            let value = args.remove(index + 1);
            args.remove(index);
            return Ok(Some(value));
        }
        if let Some(rest) = args[index].strip_prefix(&format!("{name}=")) {
            let value = rest.to_owned();
            args.remove(index);
            return Ok(Some(value));
        }
        index += 1;
    }
    Ok(None)
}

pub(crate) fn take_switch(args: &mut Vec<String>, name: &str) -> bool {
    if let Some(index) = args.iter().position(|arg| arg == name) {
        args.remove(index);
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_keeps_the_longest_side_at_the_edge() {
        assert_eq!(fit_dimensions(1170, 2532, 24), (11, 24));
        assert_eq!(fit_dimensions(2532, 1170, 24), (24, 11));
        assert_eq!(fit_dimensions(10, 10, 24), (24, 24));
    }

    #[test]
    fn cover_crops_the_longer_axis() {
        // Portrait source, landscape target: crop the height.
        assert_eq!(cover_region(1170, 2532, 1200, 630), (0, 959, 1170, 614));
        // Landscape source, square target: crop the width.
        assert_eq!(cover_region(1000, 500, 100, 100), (250, 0, 500, 500));
    }

    #[test]
    fn box_parsing_is_unambiguous() {
        assert_eq!(parse_box("0,0,780,200").unwrap(), (0, 0, 780, 200));
        assert!(parse_box("780,200,0,0").is_err());
        assert!(parse_box("0,0,780").is_err());
    }

    #[test]
    fn size_pairs_accept_both_separators() {
        assert_eq!(parse_size_pair("1200,630").unwrap(), (1200, 630));
        assert_eq!(parse_size_pair("1200x630").unwrap(), (1200, 630));
        assert!(parse_size_pair("1200").is_err());
    }
}
