//! The five IIIF Image API axes, localised.
//!
//! A request is a point in a five-dimensional space applied in one fixed order:
//!
//! ```text
//! region → size → rotation → quality → format
//! ```
//!
//! The grammar is the worker's (`cnft.dev-workers/workers/iiif/src/params.rs`) without the asset
//! key and cache keys: `full|square|x,y,w,h|pct:x,y,w,h`, then
//! `max|^max|w,|,h|w,h|^w,h|pct:n`, then `n|!n`, then
//! `default|color|gray|bitonal`, then `jpg|png|webp|gif|auto`.
//!
//! Keeping the canonical string is the point: it is what the report prints, what the derived
//! filename hashes, and what a developer can paste into a IIIF URL.

use std::fmt;

use anyhow::{bail, Result};

/// Output encoding. `Auto` defers to the tool; in this build it resolves to PNG, because a
/// derived view is for inspection and lossless is the useful default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    WebP,
    Gif,
    Auto,
}

impl Format {
    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "png" => Ok(Format::Png),
            "jpg" | "jpeg" => Ok(Format::Jpeg),
            "webp" => Ok(Format::WebP),
            "gif" => Ok(Format::Gif),
            "auto" => Ok(Format::Auto),
            other => bail!("format: `{other}` is not one of png, jpg, webp, gif, auto"),
        }
    }

    /// The extension this format writes when it has resolved. `Auto` is resolved before this is
    /// called.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Png | Format::Auto => "png",
            Format::Jpeg => "jpg",
            Format::WebP => "webp",
            Format::Gif => "gif",
        }
    }

    pub fn resolve(self) -> Format {
        match self {
            Format::Auto => Format::Png,
            explicit => explicit,
        }
    }

    /// Detect the container from its magic bytes. Only the formats this build decodes.
    pub fn from_magic(bytes: &[u8]) -> Option<Format> {
        if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
            return Some(Format::Png);
        }
        if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            return Some(Format::Jpeg);
        }
        if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            return Some(Format::Gif);
        }
        if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
            return Some(Format::WebP);
        }
        None
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Format::Png => "png",
            Format::Jpeg => "jpg",
            Format::WebP => "webp",
            Format::Gif => "gif",
            Format::Auto => "auto",
        })
    }
}

/// What to extract from the source.
#[derive(Debug, Clone, PartialEq)]
pub enum Region {
    Full,
    /// Pixel coordinates: x, y, width, height.
    Absolute {
        x: u32,
        y: u32,
        w: u32,
        h: u32,
    },
    /// Percentages of the source: x%, y%, width%, height%.
    Percentage {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    },
    /// The centred square of the source's shorter side.
    Square,
}

impl Region {
    pub fn parse(text: &str) -> Result<Self> {
        if text == "full" {
            return Ok(Region::Full);
        }
        if text == "square" {
            return Ok(Region::Square);
        }

        if let Some(rest) = text.strip_prefix("pct:") {
            let parts: Vec<&str> = rest.split(',').collect();
            if parts.len() != 4 {
                bail!("region: `{text}` needs four comma-separated percentages");
            }
            let number = |part: &str| -> Result<f32> {
                part.parse::<f32>()
                    .map_err(|_| anyhow::anyhow!("region: `{part}` is not a percentage"))
            };
            let (x, y, w, h) = (
                number(parts[0])?,
                number(parts[1])?,
                number(parts[2])?,
                number(parts[3])?,
            );
            if !(0.0..=100.0).contains(&x)
                || !(0.0..=100.0).contains(&y)
                || w <= 0.0
                || w > 100.0
                || h <= 0.0
                || h > 100.0
            {
                bail!("region: percentages must be between 0 and 100, with a positive area");
            }
            if x + w > 100.0 || y + h > 100.0 {
                bail!("region: `{text}` extends past the image");
            }
            return Ok(Region::Percentage { x, y, w, h });
        }

        let parts: Vec<&str> = text.split(',').collect();
        if parts.len() != 4 {
            bail!("region: `{text}` is not full, square, x,y,w,h or pct:x,y,w,h");
        }
        let number = |part: &str| -> Result<u32> {
            part.parse::<u32>()
                .map_err(|_| anyhow::anyhow!("region: `{part}` is not a pixel coordinate"))
        };
        let (x, y, w, h) = (
            number(parts[0])?,
            number(parts[1])?,
            number(parts[2])?,
            number(parts[3])?,
        );
        if w == 0 || h == 0 {
            bail!("region: width and height must be positive");
        }
        Ok(Region::Absolute { x, y, w, h })
    }

    /// The absolute box this region names, given the source dimensions.
    pub fn to_absolute(&self, width: u32, height: u32) -> (u32, u32, u32, u32) {
        match self {
            Region::Full => (0, 0, width, height),
            Region::Absolute { x, y, w, h } => (*x, *y, *w, *h),
            Region::Percentage { x, y, w, h } => (
                ((*x / 100.0) * width as f32).round() as u32,
                ((*y / 100.0) * height as f32).round() as u32,
                ((*w / 100.0) * width as f32).round() as u32,
                ((*h / 100.0) * height as f32).round() as u32,
            ),
            Region::Square => {
                let side = width.min(height);
                ((width - side) / 2, (height - side) / 2, side, side)
            }
        }
    }
}

impl fmt::Display for Region {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Region::Full => f.write_str("full"),
            Region::Absolute { x, y, w, h } => write!(f, "{x},{y},{w},{h}"),
            Region::Percentage { x, y, w, h } => write!(f, "pct:{x},{y},{w},{h}"),
            Region::Square => f.write_str("square"),
        }
    }
}

/// How much of the extracted region to return.
#[derive(Debug, Clone, PartialEq)]
pub enum Size {
    /// Fit within the tool's maximum dimension, never upscale.
    Max,
    /// Fit within the tool's maximum dimension, upscaling if needed.
    MaxUpscale,
    /// Exact width, height by aspect ratio.
    Width(u32),
    /// Exact height, width by aspect ratio.
    Height(u32),
    /// Percentage of the region.
    Percentage(f32),
    /// Exact width and height, which may distort.
    Exact { w: u32, h: u32 },
    /// Best fit within a box, preserving the aspect ratio.
    BestFit { w: u32, h: u32 },
}

impl Size {
    pub fn parse(text: &str) -> Result<Self> {
        if text == "max" {
            return Ok(Size::Max);
        }
        if text == "^max" {
            return Ok(Size::MaxUpscale);
        }
        if let Some(rest) = text.strip_prefix("pct:") {
            let pct = rest
                .parse::<f32>()
                .map_err(|_| anyhow::anyhow!("size: `{rest}` is not a percentage"))?;
            if pct <= 0.0 {
                bail!("size: percentage must be positive");
            }
            return Ok(Size::Percentage(pct));
        }
        if let Some(rest) = text.strip_prefix('^') {
            let parts: Vec<&str> = rest.split(',').collect();
            if parts.len() != 2 {
                bail!("size: `{text}` needs ^width,height");
            }
            let w = parts[0]
                .parse::<u32>()
                .map_err(|_| anyhow::anyhow!("size: `{}` is not a width", parts[0]))?;
            let h = parts[1]
                .parse::<u32>()
                .map_err(|_| anyhow::anyhow!("size: `{}` is not a height", parts[1]))?;
            if w == 0 || h == 0 {
                bail!("size: dimensions must be positive");
            }
            return Ok(Size::BestFit { w, h });
        }
        if let Some(rest) = text.strip_suffix(',') {
            let w = rest
                .parse::<u32>()
                .map_err(|_| anyhow::anyhow!("size: `{rest}` is not a width"))?;
            if w == 0 {
                bail!("size: width must be positive");
            }
            return Ok(Size::Width(w));
        }
        if let Some(rest) = text.strip_prefix(',') {
            let h = rest
                .parse::<u32>()
                .map_err(|_| anyhow::anyhow!("size: `{rest}` is not a height"))?;
            if h == 0 {
                bail!("size: height must be positive");
            }
            return Ok(Size::Height(h));
        }
        let parts: Vec<&str> = text.split(',').collect();
        if parts.len() != 2 {
            bail!("size: `{text}` is not max, ^max, w,, ,h, w,h, ^w,h or pct:n");
        }
        let w = parts[0]
            .parse::<u32>()
            .map_err(|_| anyhow::anyhow!("size: `{}` is not a width", parts[0]))?;
        let h = parts[1]
            .parse::<u32>()
            .map_err(|_| anyhow::anyhow!("size: `{}` is not a height", parts[1]))?;
        if w == 0 || h == 0 {
            bail!("size: dimensions must be positive");
        }
        Ok(Size::Exact { w, h })
    }

    /// Resolve to output dimensions.
    ///
    /// `max_dimension` bounds only `max` and `^max`; an explicit size is bounded by the caller's
    /// pixel cap instead. That is the one deliberate divergence from the worker, where 4096 is a
    /// hard ceiling on every size.
    pub fn dimensions(&self, width: u32, height: u32, max_dimension: u32) -> Result<(u32, u32)> {
        let width = width.max(1) as f64;
        let height = height.max(1) as f64;
        let (w, h) = match *self {
            Size::Max => {
                let scale = (max_dimension as f64 / width.max(height)).min(1.0);
                (
                    (width * scale).round() as u32,
                    (height * scale).round() as u32,
                )
            }
            Size::MaxUpscale => {
                let scale = max_dimension as f64 / width.max(height);
                (
                    (width * scale).round() as u32,
                    (height * scale).round() as u32,
                )
            }
            Size::Width(target) => {
                let w = target;
                let h = (w as f64 * height / width).round() as u32;
                (w, h)
            }
            Size::Height(target) => {
                let h = target;
                let w = (h as f64 * width / height).round() as u32;
                (w, h)
            }
            Size::Percentage(pct) => (
                (width * pct as f64 / 100.0).round() as u32,
                (height * pct as f64 / 100.0).round() as u32,
            ),
            Size::Exact { w, h } => (w, h),
            Size::BestFit { w, h } => {
                let scale = (w as f64 / width).min(h as f64 / height);
                (
                    (width * scale).round() as u32,
                    (height * scale).round() as u32,
                )
            }
        };
        Ok((w.max(1), h.max(1)))
    }
}

impl fmt::Display for Size {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Size::Max => f.write_str("max"),
            Size::MaxUpscale => f.write_str("^max"),
            Size::Width(w) => write!(f, "{w},"),
            Size::Height(h) => write!(f, ",{h}"),
            Size::Percentage(pct) => write!(f, "pct:{pct}"),
            Size::Exact { w, h } => write!(f, "{w},{h}"),
            Size::BestFit { w, h } => write!(f, "^{w},{h}"),
        }
    }
}

/// Mirroring and rotation. The light build applies right angles only; arbitrary angles are
/// refused by name rather than silently approximated.
#[derive(Debug, Clone, PartialEq)]
pub struct Rotation {
    pub mirror: bool,
    pub degrees: f32,
}

impl Rotation {
    pub fn parse(text: &str) -> Result<Self> {
        let (mirror, angle) = match text.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let degrees = angle
            .parse::<f32>()
            .map_err(|_| anyhow::anyhow!("rotation: `{text}` is not an angle"))?;
        if !(0.0..=360.0).contains(&degrees) {
            bail!("rotation: angle must be between 0 and 360");
        }
        Ok(Rotation { mirror, degrees })
    }

    pub fn is_identity(&self) -> bool {
        !self.mirror && (self.degrees == 0.0 || self.degrees == 360.0)
    }

    /// 0, 90, 180 or 270, if this is a right angle.
    pub fn right_angle(&self) -> Option<u32> {
        for angle in [0.0, 90.0, 180.0, 270.0, 360.0] {
            if (self.degrees - angle).abs() < 0.01 {
                return Some((angle as u32) % 360);
            }
        }
        None
    }
}

impl fmt::Display for Rotation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.mirror {
            write!(f, "!{}", self.degrees)
        } else {
            write!(f, "{}", self.degrees)
        }
    }
}

/// The colour axis. This is not compression quality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    Default,
    Color,
    Gray,
    Bitonal,
}

impl Quality {
    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "default" => Ok(Quality::Default),
            "color" => Ok(Quality::Color),
            "gray" => Ok(Quality::Gray),
            "bitonal" => Ok(Quality::Bitonal),
            other => bail!("quality: `{other}` is not default, color, gray or bitonal"),
        }
    }
}

impl fmt::Display for Quality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Quality::Default => "default",
            Quality::Color => "color",
            Quality::Gray => "gray",
            Quality::Bitonal => "bitonal",
        })
    }
}

/// A complete request, in the spec's order.
#[derive(Debug, Clone, PartialEq)]
pub struct Op {
    pub region: Region,
    pub size: Size,
    pub rotation: Rotation,
    pub quality: Quality,
    pub format: Format,
}

impl Op {
    /// Parse `region/size/rotation/quality.format`. The middle segments may be omitted and are
    /// filled with the spec's defaults, so `0,0,400,200/800,` is a valid shorthand.
    pub fn parse(text: &str) -> Result<Self> {
        let parts: Vec<&str> = text.split('/').collect();
        let (region, size, rotation, quality_format) = match parts.len() {
            2 => (parts[0], parts[1], "0", "default.png"),
            3 => (parts[0], parts[1], parts[2], "default.png"),
            4 => (parts[0], parts[1], parts[2], parts[3]),
            _ => bail!(
                "op: `{text}` is not region/size[/rotation][/quality.format] \
                 (four slash-separated segments at most)"
            ),
        };

        let (quality, format) = quality_format
            .split_once('.')
            .ok_or_else(|| anyhow::anyhow!("op: `{quality_format}` is not quality.format"))?;
        if format.contains('.') {
            bail!("op: `{quality_format}` has more than one format separator");
        }

        Ok(Op {
            region: Region::parse(region)?,
            size: Size::parse(size)?,
            rotation: Rotation::parse(rotation)?,
            quality: Quality::parse(quality)?,
            format: Format::parse(format)?,
        })
    }

    /// The full canonical string, with the defaults written out.
    pub fn canonical(&self) -> String {
        format!(
            "{}/{}/{}/{}.{}",
            self.region, self.size, self.rotation, self.quality, self.format
        )
    }

    /// No axis changes the pixels. The output format still has to match the source for the
    /// result to be an actual no-op; the caller checks that.
    pub fn is_identity(&self) -> bool {
        self.region == Region::Full
            && self.size == Size::Max
            && self.rotation.is_identity()
            && matches!(self.quality, Quality::Default | Quality::Color)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_full_op() {
        let op = Op::parse("0,0,780,200/1560,400/90/gray.webp").unwrap();
        assert_eq!(
            op.region,
            Region::Absolute {
                x: 0,
                y: 0,
                w: 780,
                h: 200
            }
        );
        assert_eq!(op.size, Size::Exact { w: 1560, h: 400 });
        assert_eq!(
            op.rotation,
            Rotation {
                mirror: false,
                degrees: 90.0
            }
        );
        assert_eq!(op.quality, Quality::Gray);
        assert_eq!(op.format, Format::WebP);
    }

    #[test]
    fn fills_omitted_trailing_segments() {
        let op = Op::parse("0,0,400,200/800,").unwrap();
        assert_eq!(op.canonical(), "0,0,400,200/800,/0/default.png");
        assert_eq!(op.format, Format::Png);
    }

    #[test]
    fn parses_every_region_form() {
        assert_eq!(Region::parse("full").unwrap(), Region::Full);
        assert_eq!(Region::parse("square").unwrap(), Region::Square);
        assert_eq!(
            Region::parse("10,20,30,40").unwrap(),
            Region::Absolute {
                x: 10,
                y: 20,
                w: 30,
                h: 40
            }
        );
        assert_eq!(
            Region::parse("pct:0,0,50,50").unwrap(),
            Region::Percentage {
                x: 0.0,
                y: 0.0,
                w: 50.0,
                h: 50.0
            }
        );
        assert_eq!(
            Region::parse("pct:0,0,80,80").unwrap(),
            Region::Percentage {
                x: 0.0,
                y: 0.0,
                w: 80.0,
                h: 80.0
            }
        );
        assert!(Region::parse("pct:50,0,60,50").is_err());
        assert!(Region::parse("10,20,0,40").is_err());
    }

    #[test]
    fn square_is_centred() {
        assert_eq!(Region::Square.to_absolute(100, 60), (20, 0, 60, 60));
        assert_eq!(Region::Square.to_absolute(60, 100), (0, 20, 60, 60));
    }

    #[test]
    fn percentage_is_rounded_to_pixels() {
        assert_eq!(
            Region::Percentage {
                x: 0.0,
                y: 0.0,
                w: 50.0,
                h: 50.0
            }
            .to_absolute(100, 50),
            (0, 0, 50, 25)
        );
    }

    #[test]
    fn parses_every_size_form() {
        assert_eq!(Size::parse("max").unwrap(), Size::Max);
        assert_eq!(Size::parse("^max").unwrap(), Size::MaxUpscale);
        assert_eq!(Size::parse("400,").unwrap(), Size::Width(400));
        assert_eq!(Size::parse(",300").unwrap(), Size::Height(300));
        assert_eq!(
            Size::parse("400,300").unwrap(),
            Size::Exact { w: 400, h: 300 }
        );
        assert_eq!(
            Size::parse("^400,300").unwrap(),
            Size::BestFit { w: 400, h: 300 }
        );
        assert_eq!(Size::parse("pct:50").unwrap(), Size::Percentage(50.0));
        assert!(Size::parse("0,").is_err());
    }

    #[test]
    fn size_dimensions_preserve_or_distort_as_named() {
        assert_eq!(
            Size::Width(400).dimensions(1000, 500, 4096).unwrap(),
            (400, 200)
        );
        assert_eq!(
            Size::Height(200).dimensions(1000, 500, 4096).unwrap(),
            (400, 200)
        );
        assert_eq!(
            Size::Exact { w: 400, h: 400 }
                .dimensions(1000, 500, 4096)
                .unwrap(),
            (400, 400)
        );
        assert_eq!(
            Size::BestFit { w: 400, h: 400 }
                .dimensions(1000, 500, 4096)
                .unwrap(),
            (400, 200)
        );
        assert_eq!(
            Size::Percentage(50.0).dimensions(1000, 500, 4096).unwrap(),
            (500, 250)
        );
    }

    #[test]
    fn max_never_upscales_and_uses_the_ceiling() {
        assert_eq!(Size::Max.dimensions(1000, 500, 4096).unwrap(), (1000, 500));
        assert_eq!(
            Size::Max.dimensions(8192, 4096, 4096).unwrap(),
            (4096, 2048)
        );
        assert_eq!(
            Size::MaxUpscale.dimensions(1000, 500, 4096).unwrap(),
            (4096, 2048)
        );
    }

    #[test]
    fn parses_rotation_and_identity() {
        assert!(Rotation::parse("0").unwrap().is_identity());
        assert!(Rotation::parse("360").unwrap().is_identity());
        assert_eq!(Rotation::parse("!90").unwrap().right_angle(), Some(90));
        assert!(!Rotation::parse("!90").unwrap().is_identity());
        assert!(Rotation::parse("45").unwrap().right_angle().is_none());
        assert!(Rotation::parse("361").is_err());
    }

    #[test]
    fn canonical_round_trips() {
        for text in [
            "full/max/0/default.png",
            "square/400,/0/gray.jpg",
            "0,0,100,50/^200,100/!90/bitonal.webp",
            "pct:10,10,80,80/pct:50/270/color.gif",
        ] {
            let op = Op::parse(text).unwrap();
            assert_eq!(op.canonical(), text);
            assert_eq!(Op::parse(&op.canonical()).unwrap(), op);
        }
    }

    #[test]
    fn identity_requires_every_axis_to_be_default() {
        assert!(Op::parse("full/max/0/default.png").unwrap().is_identity());
        assert!(!Op::parse("full/max/0/gray.png").unwrap().is_identity());
        assert!(!Op::parse("square/max/0/default.png").unwrap().is_identity());
    }
}
