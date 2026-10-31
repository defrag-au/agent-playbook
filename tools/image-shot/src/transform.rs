//! Applying the five axes in order, then encoding once.
//!
//! The pipeline is the worker's: region, size, rotation, quality, format. There is no arbitrary
//! operation list; the axes are independent and the order is fixed, which is what makes a request
//! reproducible and a canonical string meaningful.

use std::io::Cursor;

use anyhow::{bail, Context, Result};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ExtendedColorType, GrayImage, ImageFormat, Rgb, RgbImage};

use crate::op::{Format, Op, Quality, Size};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    Nearest,
    Lanczos3,
    CatmullRom,
}

impl Filter {
    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "nearest" => Ok(Filter::Nearest),
            "lanczos3" | "lanczos" => Ok(Filter::Lanczos3),
            "catmullrom" | "catmull" => Ok(Filter::CatmullRom),
            other => bail!("filter: `{other}` is not nearest, lanczos3 or catmullrom"),
        }
    }

    pub fn image(self) -> FilterType {
        match self {
            Filter::Nearest => FilterType::Nearest,
            Filter::Lanczos3 => FilterType::Lanczos3,
            Filter::CatmullRom => FilterType::CatmullRom,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Filter::Nearest => "nearest",
            Filter::Lanczos3 => "lanczos3",
            Filter::CatmullRom => "catmullrom",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RenderOptions {
    pub filter: Filter,
    /// JPEG quality, 1–100. The IIIF `quality` axis is colour, not compression.
    pub compression: u8,
    /// Required when a source with alpha is written as JPEG.
    pub background: Option<[u8; 3]>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            filter: Filter::Lanczos3,
            compression: 85,
            background: None,
        }
    }
}

pub struct Rendered {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub format: Format,
    pub filter: Filter,
    pub notes: Vec<String>,
}

pub fn render(
    mut image: DynamicImage,
    op: &Op,
    options: &RenderOptions,
    max_dimension: u32,
    max_pixels: u64,
) -> Result<Rendered> {
    let mut notes = Vec::new();

    // ── region
    let (x, y, w, h) = op.region.to_absolute(image.width(), image.height());
    if x >= image.width() || y >= image.height() {
        bail!(
            "region: {x},{y} starts outside the {}x{} source",
            image.width(),
            image.height()
        );
    }
    let actual_w = w.min(image.width() - x);
    let actual_h = h.min(image.height() - y);
    if actual_w == 0 || actual_h == 0 {
        bail!("region: the extracted area is empty");
    }
    if actual_w != w || actual_h != h {
        notes.push(format!("region clamped to {actual_w}x{actual_h}"));
    }
    image = image.crop_imm(x, y, actual_w, actual_h);

    // ── size
    let before = (image.width(), image.height());
    let (target_w, target_h) = op.size.dimensions(before.0, before.1, max_dimension)?;
    if u64::from(target_w) * u64::from(target_h) > max_pixels {
        bail!(
            "size: {target_w}x{target_h} ({:.1} MP) is above the {:.1} MP output cap",
            (u64::from(target_w) * u64::from(target_h)) as f64 / 1_000_000.0,
            max_pixels as f64 / 1_000_000.0
        );
    }
    if (target_w, target_h) != before {
        if matches!(op.size, Size::Exact { .. }) {
            let source_ratio = before.0 as f64 / before.1 as f64;
            let target_ratio = target_w as f64 / target_h as f64;
            if (source_ratio - target_ratio).abs() > 0.01 {
                notes.push("exact size distorts the aspect ratio".to_owned());
            }
        }
        image = image.resize_exact(target_w, target_h, options.filter.image());
    }

    // ── rotation
    if op.rotation.mirror {
        image = image.fliph();
        notes.push("mirrored horizontally".to_owned());
    }
    let Some(angle) = op.rotation.right_angle() else {
        bail!(
            "rotation: {}° is not supported in this build; use 0, 90, 180 or 270 \
             (or the IIIF worker for arbitrary angles)",
            op.rotation.degrees
        );
    };
    image = match angle {
        0 => image,
        90 => image.rotate90(),
        180 => image.rotate180(),
        270 => image.rotate270(),
        _ => unreachable!("right_angle returns 0, 90, 180 or 270"),
    };
    if angle != 0 {
        notes.push(format!("rotated {angle}°"));
    }

    // ── quality
    match op.quality {
        Quality::Default | Quality::Color => {}
        Quality::Gray => {
            image = DynamicImage::ImageLuma8(image.to_luma8());
            notes.push("converted to grayscale".to_owned());
        }
        Quality::Bitonal => {
            let gray = image.to_luma8();
            let mut bitonal = GrayImage::new(gray.width(), gray.height());
            for (x, y, pixel) in gray.enumerate_pixels() {
                bitonal.put_pixel(x, y, image::Luma([if pixel[0] > 128 { 255 } else { 0 }]));
            }
            image = DynamicImage::ImageLuma8(bitonal);
            notes.push("converted to bitonal at threshold 128".to_owned());
        }
    }

    // ── format
    let format = op.format.resolve();
    let bytes = encode(&image, format, options, &mut notes)?;

    Ok(Rendered {
        bytes,
        width: image.width(),
        height: image.height(),
        format,
        filter: options.filter,
        notes,
    })
}

fn encode(
    image: &DynamicImage,
    format: Format,
    options: &RenderOptions,
    notes: &mut Vec<String>,
) -> Result<Vec<u8>> {
    let mut buffer = Vec::new();
    match format {
        Format::Png => {
            image
                .write_to(&mut Cursor::new(&mut buffer), ImageFormat::Png)
                .context("encode PNG")?;
        }
        Format::Gif => {
            image
                .write_to(&mut Cursor::new(&mut buffer), ImageFormat::Gif)
                .context("encode GIF")?;
            notes.push("GIF output is a single frame".to_owned());
        }
        Format::WebP => {
            image
                .write_to(&mut Cursor::new(&mut buffer), ImageFormat::WebP)
                .context("encode WebP")?;
            notes.push("WebP output is lossless in this build".to_owned());
        }
        Format::Jpeg => {
            let rgb = if image.color().has_alpha() {
                let background = options.background.ok_or_else(|| {
                    anyhow::anyhow!(
                        "format: the source has alpha and JPEG cannot hold it; \
                         pass --background RRGGBB to choose the flatten colour"
                    )
                })?;
                notes.push(format!(
                    "alpha flattened onto #{:02x}{:02x}{:02x}",
                    background[0], background[1], background[2]
                ));
                flatten(image, background)
            } else {
                image.to_rgb8()
            };
            let mut encoder = JpegEncoder::new_with_quality(&mut buffer, options.compression);
            encoder
                .encode(
                    rgb.as_raw(),
                    rgb.width(),
                    rgb.height(),
                    ExtendedColorType::Rgb8,
                )
                .context("encode JPEG")?;
        }
        Format::Auto => unreachable!("Format::Auto is resolved before encode"),
    }
    Ok(buffer)
}

/// Composite RGBA over an opaque colour. `to_rgb8()` drops alpha rather than blending, which is
/// the trap this exists to avoid.
fn flatten(image: &DynamicImage, background: [u8; 3]) -> RgbImage {
    let rgba = image.to_rgba8();
    let mut out = RgbImage::new(rgba.width(), rgba.height());
    for (x, y, pixel) in rgba.enumerate_pixels() {
        let alpha = u16::from(pixel[3]);
        let blend = |channel: u8, base: u8| -> u8 {
            ((u16::from(channel) * alpha + u16::from(base) * (255 - alpha) + 127) / 255) as u8
        };
        out.put_pixel(
            x,
            y,
            Rgb([
                blend(pixel[0], background[0]),
                blend(pixel[1], background[1]),
                blend(pixel[2], background[2]),
            ]),
        );
    }
    out
}

/// `#rrggbb` or `rrggbb`.
pub fn parse_background(text: &str) -> Result<[u8; 3]> {
    let hex = text.strip_prefix('#').unwrap_or(text);
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("--background: `{text}` is not RRGGBB");
    }
    let channel = |index: usize| -> u8 {
        u8::from_str_radix(&hex[index..index + 2], 16).expect("validated as hex")
    };
    Ok([channel(0), channel(2), channel(4)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::op::Op;
    use image::{Rgba, RgbaImage};

    fn checker(width: u32, height: u32) -> DynamicImage {
        let mut image = RgbaImage::new(width, height);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = Rgba([
                if x % 2 == 0 { 255 } else { 0 },
                if y % 2 == 0 { 255 } else { 0 },
                64,
                255,
            ]);
        }
        DynamicImage::ImageRgba8(image)
    }

    fn run(op: &str, options: RenderOptions) -> Rendered {
        let op = Op::parse(op).unwrap();
        render(checker(4, 2), &op, &options, 4096, 64_000_000).unwrap()
    }

    #[test]
    fn crop_and_resize_happen_in_order() {
        let rendered = run("0,0,2,2/4,4/0/default.png", RenderOptions::default());
        assert_eq!((rendered.width, rendered.height), (4, 4));
        assert_eq!(rendered.format, Format::Png);
        let decoded = image::load_from_memory(&rendered.bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (4, 4));
    }

    #[test]
    fn rotation_swaps_dimensions() {
        let rendered = run("full/max/90/default.png", RenderOptions::default());
        assert_eq!((rendered.width, rendered.height), (2, 4));
        let decoded = image::load_from_memory(&rendered.bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (2, 4));
    }

    #[test]
    fn mirror_is_applied_before_rotation() {
        let rendered = run("full/max/!0/default.png", RenderOptions::default());
        assert_eq!((rendered.width, rendered.height), (4, 2));
        let decoded = image::load_from_memory(&rendered.bytes).unwrap().to_rgba8();
        // The top-left of the mirrored checker is the old top-right: x=3 was odd → red 0.
        assert_eq!(decoded.get_pixel(0, 0)[0], 0);
    }

    #[test]
    fn gray_and_bitonal_change_the_colour_type() {
        let gray = run("full/max/0/gray.png", RenderOptions::default());
        let decoded = image::load_from_memory(&gray.bytes).unwrap();
        assert!(!decoded.color().has_color());

        let bitonal = run("full/max/0/bitonal.png", RenderOptions::default());
        let decoded = image::load_from_memory(&bitonal.bytes).unwrap().to_luma8();
        for pixel in decoded.pixels() {
            assert!(pixel[0] == 0 || pixel[0] == 255);
        }
    }

    #[test]
    fn jpeg_with_alpha_needs_a_background() {
        let op = Op::parse("full/max/0/default.jpg").unwrap();
        let error = render(
            checker(4, 2),
            &op,
            &RenderOptions::default(),
            4096,
            64_000_000,
        )
        .err()
        .expect("alpha to JPEG is refused");
        assert!(error.to_string().contains("--background"));

        let options = RenderOptions {
            background: Some([0, 0, 0]),
            ..RenderOptions::default()
        };
        let rendered = render(checker(4, 2), &op, &options, 4096, 64_000_000).unwrap();
        assert_eq!(rendered.format, Format::Jpeg);
        assert!(rendered.notes.iter().any(|note| note.contains("flattened")));
    }

    #[test]
    fn output_pixel_cap_refuses_an_upscale() {
        let op = Op::parse("full/40000,40000/0/default.png").unwrap();
        let error = render(
            checker(4, 2),
            &op,
            &RenderOptions::default(),
            4096,
            64_000_000,
        )
        .err()
        .expect("an enormous output is refused");
        assert!(error.to_string().contains("output cap"));
    }

    #[test]
    fn background_parses_with_and_without_hash() {
        assert_eq!(parse_background("112233").unwrap(), [0x11, 0x22, 0x33]);
        assert_eq!(parse_background("#a0b1c2").unwrap(), [0xa0, 0xb1, 0xc2]);
        assert!(parse_background("123").is_err());
        assert!(parse_background("zzzzzz").is_err());
    }
}
