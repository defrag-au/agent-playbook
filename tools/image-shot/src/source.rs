//! Reading a source once: bytes, container, header dimensions, and the decode budget.
//!
//! `image` decodes a file into one buffer, so the guard is before the decode: read the header,
//! refuse anything over the pixel cap, and only then allocate. That is the whole large-image
//! story in this build — a clear refusal instead of an out-of-memory kill.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use image::DynamicImage;

use crate::artifact::display;
use crate::op::Format;

pub struct Source {
    /// Canonical path, for comparing an output against its input.
    pub path: PathBuf,
    /// The path as the caller wrote it, relative to the working directory.
    pub label: String,
    pub bytes: Vec<u8>,
    pub format: Format,
    pub width: u32,
    pub height: u32,
}

impl Source {
    pub fn read(given: &str, cwd: &Path, max_pixels: u64) -> Result<Self> {
        if given.is_empty() {
            bail!("source path is empty");
        }
        let path = if Path::new(given).is_absolute() {
            PathBuf::from(given)
        } else {
            cwd.join(given)
        };
        let label = display(&path, cwd);
        let bytes = std::fs::read(&path).with_context(|| format!("read {label}"))?;
        let path = std::fs::canonicalize(&path).with_context(|| format!("resolve {label}"))?;

        let format = Format::from_magic(&bytes).ok_or_else(|| {
            anyhow::anyhow!(
                "{label} is {}; this build decodes png, jpeg, gif and webp. \
                 Convert it first (sips, cwebp or a repo pipeline) and read the converted file.",
                unsupported(&bytes)
            )
        })?;

        let reader = image::ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .with_context(|| format!("detect the format of {label}"))?;
        let (width, height) = reader
            .into_dimensions()
            .with_context(|| format!("read the header of {label}"))?;

        let pixels = u64::from(width) * u64::from(height);
        if pixels > max_pixels {
            bail!(
                "{label} is {width}x{height} ({:.1} MP), above the {:.1} MP decode cap. \
                 Nothing was written. Use a smaller source, or pass --max-pixels if this is intended.",
                pixels as f64 / 1_000_000.0,
                max_pixels as f64 / 1_000_000.0
            );
        }

        Ok(Self {
            path,
            label,
            bytes,
            format,
            width,
            height,
        })
    }

    pub fn decode(&self) -> Result<DynamicImage> {
        let image = image::load_from_memory(&self.bytes)
            .with_context(|| format!("decode {}", self.label))?;
        let (width, height) = (image.width(), image.height());
        if (width, height) != (self.width, self.height) {
            bail!(
                "{}: the decoder returned {width}x{height}, but the header said {}x{}",
                self.label,
                self.width,
                self.height
            );
        }
        Ok(image)
    }

    /// Refuse an `--out` that names the source itself.
    pub fn is_same_file(&self, candidate: &Path) -> bool {
        std::fs::canonicalize(candidate)
            .map(|candidate| candidate == self.path)
            .unwrap_or(false)
    }
}

/// A short name for a container this build does not decode.
pub(crate) fn unsupported(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"BM") {
        return "a BMP";
    }
    if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        return "a TIFF";
    }
    let head = bytes
        .iter()
        .skip_while(|byte| byte.is_ascii_whitespace())
        .copied()
        .collect::<Vec<_>>();
    if head.starts_with(b"<svg") || head.starts_with(b"<?xml") {
        return "an SVG";
    }
    if bytes.len() >= 12 && &bytes[4..12] == b"ftypavif" {
        return "an AVIF";
    }
    if bytes.len() >= 12
        && matches!(
            &bytes[4..12],
            b"ftypheic" | b"ftypheix" | b"ftypmif1" | b"ftypmsf1"
        )
    {
        return "a HEIC";
    }
    if bytes.starts_with(b"%PDF") {
        return "a PDF";
    }
    "an unrecognised format"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_formats_it_refuses() {
        assert_eq!(unsupported(b"BM...."), "a BMP");
        assert_eq!(unsupported(b"II*\0...."), "a TIFF");
        assert_eq!(unsupported(b"  <svg></svg>"), "an SVG");
        assert_eq!(unsupported(b"\0\0\0\0ftypheic...."), "a HEIC");
        assert_eq!(unsupported(b"%PDF-1.7"), "a PDF");
        assert_eq!(unsupported(b"nonsense"), "an unrecognised format");
    }
}
