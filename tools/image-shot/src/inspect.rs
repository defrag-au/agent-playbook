//! The observation half: header information, pixel statistics, and comparison.
//!
//! None of these is a IIIF axis. `info` is the local `info.json` — what the file is and how big
//! it is; `stats` answers questions the eye cannot; `diff` answers what changed.

use std::collections::HashSet;
use std::io::{Cursor, Read};

use anyhow::{bail, Context, Result};
use image::{DynamicImage, ImageDecoder, ImageFormat, Rgba, RgbaImage};

use crate::artifact::display;
use crate::op::Format;
use crate::source::{unsupported, Source};

/// Header-only information. No pixel data is decoded.
pub fn info(paths: &[String], cwd: &std::path::Path) -> Result<i32> {
    if paths.is_empty() {
        bail!("info needs at least one source");
    }
    for given in paths {
        let path = if std::path::Path::new(given).is_absolute() {
            std::path::PathBuf::from(given)
        } else {
            cwd.join(given)
        };
        let label = display(&path, cwd);
        let file = std::fs::File::open(&path).with_context(|| format!("open {label}"))?;
        let byte_len = file
            .metadata()
            .with_context(|| format!("read the metadata of {label}"))?
            .len();
        let size = bytes_size(byte_len);
        let mut head = [0_u8; 64];
        let read = std::io::BufReader::new(file)
            .read(&mut head)
            .with_context(|| format!("read the header of {label}"))?;
        let head = &head[..read];

        let Some(format) = Format::from_magic(head) else {
            println!(
                "{label} · {} · {size} · dimensions unavailable — this build decodes png, jpeg, gif and webp",
                unsupported(head)
            );
            continue;
        };

        let reader = image::ImageReader::open(&path)
            .with_context(|| format!("open {label}"))?
            .with_guessed_format()
            .with_context(|| format!("detect the format of {label}"))?;
        match reader.into_decoder() {
            Ok(decoder) => {
                let (width, height) = decoder.dimensions();
                let color = decoder.color_type();
                let alpha = if color.has_alpha() { " · alpha" } else { "" };
                println!("{label} · {format} · {width}x{height} · {color:?} · {size}{alpha}");
            }
            Err(error) => {
                println!("{label} · {format} · {size} · dimensions unavailable: {error}");
            }
        };
    }
    Ok(0)
}

#[derive(Debug, Clone)]
pub struct Stats {
    pub width: u32,
    pub height: u32,
    pub color: image::ColorType,
    pub mean: [f64; 4],
    pub sd: [f64; 4],
    pub alpha_fraction: f64,
    pub colours: usize,
    pub colours_capped: bool,
    pub ink: Option<(u32, u32, u32, u32)>,
    pub sharpness: f64,
}

pub fn stats(source: &Source) -> Result<Stats> {
    Ok(stats_image(&source.decode()?))
}

/// Measure an image already in memory, for the scenario runner.
pub fn stats_image(image: &DynamicImage) -> Stats {
    let color = image.color();
    let rgba = image.to_rgba8();
    measure(&rgba, color)
}

fn measure(rgba: &RgbaImage, color: image::ColorType) -> Stats {
    let (width, height) = rgba.dimensions();
    let total = u64::from(width) * u64::from(height);
    let mut sum = [0_f64; 4];
    let mut sum_sq = [0_f64; 4];
    let mut alpha_pixels = 0_u64;
    let mut colours = HashSet::new();
    let mut colours_capped = false;

    let reference = rgba.get_pixel(0, 0).0;
    let mut ink: Option<(u32, u32, u32, u32)> = None;
    let mut transparent_bbox = false;
    for (_, _, pixel) in rgba.enumerate_pixels() {
        if pixel[3] < 255 {
            transparent_bbox = true;
            break;
        }
    }

    for (x, y, pixel) in rgba.enumerate_pixels() {
        for channel in 0..4 {
            let value = f64::from(pixel[channel]);
            sum[channel] += value;
            sum_sq[channel] += value * value;
        }
        if pixel[3] < 255 {
            alpha_pixels += 1;
        }
        if !colours_capped {
            colours.insert([pixel[0], pixel[1], pixel[2], pixel[3]]);
            if colours.len() >= 4096 {
                colours_capped = true;
            }
        }

        let is_ink = if transparent_bbox {
            pixel[3] > 0
        } else {
            let delta = (0..3)
                .map(|channel| i32::from(pixel[channel]).abs_diff(i32::from(reference[channel])))
                .max()
                .unwrap_or(0);
            delta > 8
        };
        if is_ink {
            ink = Some(match ink {
                None => (x, y, x, y),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
            });
        }
    }

    let mut mean = [0_f64; 4];
    let mut sd = [0_f64; 4];
    for channel in 0..4 {
        let count = total.max(1) as f64;
        mean[channel] = sum[channel] / count;
        let variance = (sum_sq[channel] / count) - mean[channel] * mean[channel];
        sd[channel] = variance.max(0.0).sqrt();
    }

    let alpha_fraction = if total == 0 {
        0.0
    } else {
        alpha_pixels as f64 * 100.0 / total as f64
    };

    Stats {
        width,
        height,
        color,
        mean,
        sd,
        alpha_fraction,
        colours: colours.len(),
        colours_capped,
        ink,
        sharpness: sharpness(rgba),
    }
}

/// Variance of the 3×3 Laplacian on luma. A single number for "how much edge is here"; it is a
/// review aid, not a focus score.
fn sharpness(rgba: &RgbaImage) -> f64 {
    let (width, height) = rgba.dimensions();
    if width < 3 || height < 3 {
        return 0.0;
    }
    let luma: Vec<f64> = rgba
        .pixels()
        .map(|pixel| {
            0.2126 * f64::from(pixel[0])
                + 0.7152 * f64::from(pixel[1])
                + 0.0722 * f64::from(pixel[2])
        })
        .collect();
    let at = |x: u32, y: u32| luma[(y * width + x) as usize];
    let mut sum = 0_f64;
    let mut count = 0_u64;
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let value = 4.0 * at(x, y) - at(x - 1, y) - at(x + 1, y) - at(x, y - 1) - at(x, y + 1);
            // A flat field can leave a residue around 1e-29 from the luma weights; treat that as
            // no edge so a caller comparing the printed value sees zero.
            let value = if value.abs() < 1e-9 { 0.0 } else { value };
            sum += value * value;
            count += 1;
        }
    }
    if count == 0 {
        0.0
    } else {
        sum / count as f64
    }
}

pub fn print_stats(label: &str, stats: &Stats) {
    let bbox = stats.ink.map_or_else(
        || "none".to_owned(),
        |(x0, y0, x1, y1)| format!("{x0},{y0}–{x1},{y1}"),
    );
    let capped = if stats.colours_capped {
        " (capped)"
    } else {
        ""
    };
    println!(
        "{label} · {}x{} · {:?}",
        stats.width, stats.height, stats.color
    );
    println!(
        "mean   {:.1} {:.1} {:.1} {:.1} · sd {:.1} {:.1} {:.1} {:.1}",
        stats.mean[0],
        stats.mean[1],
        stats.mean[2],
        stats.mean[3],
        stats.sd[0],
        stats.sd[1],
        stats.sd[2],
        stats.sd[3]
    );
    println!(
        "alpha  {:.1}% · colours {}{} · ink {bbox}",
        stats.alpha_fraction, stats.colours, capped
    );
    println!("sharp  {:.1} (variance of Laplacian)", stats.sharpness);
}

#[derive(Debug, Clone)]
pub struct Diff {
    pub changed: u64,
    pub total: u64,
    pub max_delta: u8,
    pub mean_delta: f64,
    pub bbox: Option<(u32, u32, u32, u32)>,
}

impl Diff {
    pub fn fraction(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.changed as f64 * 100.0 / self.total as f64
        }
    }
}

pub fn diff(a: &DynamicImage, b: &DynamicImage, tolerance: u8) -> Result<Diff> {
    if (a.width(), a.height()) != (b.width(), b.height()) {
        bail!(
            "diff: dimensions differ — a is {}x{}, b is {}x{}; crop or fit them to the same size first",
            a.width(),
            a.height(),
            b.width(),
            b.height()
        );
    }
    let (width, height) = (a.width(), a.height());
    let left = a.to_rgba8();
    let right = b.to_rgba8();
    let total = u64::from(width) * u64::from(height);
    let mut changed = 0_u64;
    let mut max_delta = 0_u8;
    let mut sum_delta = 0_u64;
    let mut bbox: Option<(u32, u32, u32, u32)> = None;

    for (x, y, pixel) in left.enumerate_pixels() {
        let other = right.get_pixel(x, y);
        let delta = (0..4)
            .map(|channel| pixel[channel].abs_diff(other[channel]))
            .max()
            .unwrap_or(0);
        sum_delta += u64::from(delta);
        if delta > tolerance {
            changed += 1;
            max_delta = max_delta.max(delta);
            bbox = Some(match bbox {
                None => (x, y, x, y),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
            });
        }
    }

    let mean_delta = if total == 0 {
        0.0
    } else {
        sum_delta as f64 / total as f64
    };

    Ok(Diff {
        changed,
        total,
        max_delta,
        mean_delta,
        bbox,
    })
}

/// A dimmed source with changed pixels in red, as a PNG.
pub fn diff_view(a: &DynamicImage, b: &DynamicImage, tolerance: u8) -> Result<Vec<u8>> {
    let left = a.to_rgba8();
    let right = b.to_rgba8();
    let mut view = RgbaImage::new(left.width(), left.height());
    for (x, y, pixel) in left.enumerate_pixels() {
        let other = right.get_pixel(x, y);
        let delta = (0..4)
            .map(|channel| pixel[channel].abs_diff(other[channel]))
            .max()
            .unwrap_or(0);
        let out = if delta > tolerance {
            Rgba([255, 0, 0, 255])
        } else {
            let luma = (0.2126 * f32::from(pixel[0])
                + 0.7152 * f32::from(pixel[1])
                + 0.0722 * f32::from(pixel[2])) as u8;
            let dim = luma / 3;
            Rgba([dim, dim, dim, 255])
        };
        view.put_pixel(x, y, out);
    }
    let mut buffer = Vec::new();
    DynamicImage::ImageRgba8(view)
        .write_to(&mut Cursor::new(&mut buffer), ImageFormat::Png)
        .context("encode the diff view")?;
    Ok(buffer)
}

pub fn bytes_size(bytes: u64) -> String {
    let bytes = bytes as f64;
    if bytes >= 1_048_576.0 {
        format!("{:.1} MiB", bytes / 1_048_576.0)
    } else if bytes >= 1024.0 {
        format!("{:.1} KiB", bytes / 1024.0)
    } else {
        format!("{} B", bytes as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn image(pixels: &[[u8; 4]], width: u32, height: u32) -> DynamicImage {
        let mut buffer = RgbaImage::new(width, height);
        for (index, pixel) in pixels.iter().enumerate() {
            let x = index as u32 % width;
            let y = index as u32 / width;
            buffer.put_pixel(x, y, Rgba(*pixel));
        }
        DynamicImage::ImageRgba8(buffer)
    }

    #[test]
    fn stats_reports_a_transparent_ink_bbox() {
        let source = image(
            &[[0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], [255, 0, 0, 255]],
            2,
            2,
        );
        let stats = measure(&source.to_rgba8(), source.color());
        assert_eq!(stats.ink, Some((1, 1, 1, 1)));
        assert!((stats.alpha_fraction - 75.0).abs() < 0.01);
    }

    #[test]
    fn stats_uses_the_background_for_opaque_ink() {
        let source = image(
            &[
                [0, 0, 0, 255],
                [0, 0, 0, 255],
                [0, 0, 0, 255],
                [255, 255, 255, 255],
            ],
            2,
            2,
        );
        let stats = measure(&source.to_rgba8(), source.color());
        assert_eq!(stats.ink, Some((1, 1, 1, 1)));
    }

    #[test]
    fn identical_images_have_no_difference() {
        let a = image(&[[1, 2, 3, 255]; 4], 2, 2);
        let b = image(&[[1, 2, 3, 255]; 4], 2, 2);
        let result = diff(&a, &b, 0).unwrap();
        assert_eq!(result.changed, 0);
        assert_eq!(result.bbox, None);
        assert_eq!(result.max_delta, 0);
    }

    #[test]
    fn a_changed_pixel_moves_the_bbox() {
        let a = image(&[[0, 0, 0, 255]; 4], 2, 2);
        let mut pixels = [[0, 0, 0, 255]; 4];
        pixels[3] = [255, 255, 255, 255];
        let b = image(&pixels, 2, 2);
        let result = diff(&a, &b, 0).unwrap();
        assert_eq!(result.changed, 1);
        assert_eq!(result.bbox, Some((1, 1, 1, 1)));
        assert_eq!(result.max_delta, 255);
    }

    #[test]
    fn dimension_mismatch_is_refused() {
        let a = image(&[[0, 0, 0, 255]; 4], 2, 2);
        let b = image(&[[0, 0, 0, 255]; 6], 3, 2);
        assert!(diff(&a, &b, 0).is_err());
    }

    #[test]
    fn sharpness_is_zero_for_a_flat_image() {
        let flat = image(&[[10, 10, 10, 255]; 9], 3, 3);
        assert_eq!(sharpness(&flat.to_rgba8()), 0.0);
    }
}
