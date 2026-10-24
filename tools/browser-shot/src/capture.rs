//! Capture the viewport or a selector, and read back the real pixel dimensions.

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use headless_chrome::browser::tab::Tab;
use headless_chrome::protocol::cdp::Page;
use headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption;

#[derive(Clone)]
pub struct Captured {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub fn viewport(tab: &Tab) -> Result<Captured> {
    let bytes = tab
        .capture_screenshot(CaptureScreenshotFormatOption::Png, None, None, true)
        .context("capture the viewport")?;
    let (width, height) = png_dimensions(&bytes).ok_or_else(|| anyhow!("capture was not a PNG"))?;
    Ok(Captured {
        bytes,
        width,
        height,
    })
}

pub fn element(tab: &Tab, selector: &str, timeout: Duration) -> Result<(Captured, Page::Viewport)> {
    let element = tab
        .wait_for_element_with_custom_timeout(selector, timeout)
        .with_context(|| format!("find {selector}"))?;
    let measured = element
        .get_box_model()
        .with_context(|| format!("measure {selector}"))?
        .border_viewport();

    if measured.width <= 0.0 || measured.height <= 0.0 {
        bail!(
            "{selector} has an empty border box ({}x{})",
            measured.width,
            measured.height
        );
    }

    // Chromium floors a fractional clip before scaling it by the DPR, so a 389.99997 CSS box
    // at DPR 3 captures 1167 px instead of 1170. Expand to whole CSS pixels.
    let clip = Page::Viewport {
        x: measured.x.floor(),
        y: measured.y.floor(),
        width: (measured.x + measured.width).ceil() - measured.x.floor(),
        height: (measured.y + measured.height).ceil() - measured.y.floor(),
        scale: measured.scale,
    };

    let data = tab
        .call_method(Page::CaptureScreenshot {
            format: Some(CaptureScreenshotFormatOption::Png),
            quality: None,
            clip: Some(clip),
            from_surface: Some(true),
            capture_beyond_viewport: Some(true),
            optimize_for_speed: None,
        })
        .with_context(|| format!("capture {selector}"))?
        .data;
    let bytes = BASE64.decode(data).context("decode the screenshot")?;
    let (width, height) = png_dimensions(&bytes).ok_or_else(|| anyhow!("capture was not a PNG"))?;

    Ok((
        Captured {
            bytes,
            width,
            height,
        },
        measured,
    ))
}

/// A cheap overflow check: content larger than the element's own client box can be clipped by
/// an ancestor with `overflow: hidden`, and the capture then shows less than the element holds.
pub fn overflow(tab: &Tab, selector: &str, timeout: Duration) -> Option<(u32, u32, u32, u32)> {
    let element = tab
        .wait_for_element_with_custom_timeout(selector, timeout)
        .ok()?;
    let object = element
        .call_js_fn(
            "function () { return [this.scrollWidth, this.clientWidth, this.scrollHeight, this.clientHeight].join(','); }",
            vec![],
            false,
        )
        .ok()?;
    let text = object.value?.as_str()?.to_owned();
    let values = text
        .split(',')
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if values.len() != 4 {
        return None;
    }
    Some((values[0], values[1], values[2], values[3]))
}

pub fn canvas_backing(tab: &Tab, selector: &str, timeout: Duration) -> Option<(u32, u32)> {
    let element = tab
        .wait_for_element_with_custom_timeout(selector, timeout)
        .ok()?;
    let object = element
        .call_js_fn(
            "function () { return this.tagName === 'CANVAS' ? this.width + ',' + this.height : ''; }",
            vec![],
            false,
        )
        .ok()?;
    let text = object.value?.as_str()?.to_owned();
    let values = text
        .split(',')
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if values.len() != 2 {
        return None;
    }
    Some((values[0], values[1]))
}

pub fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    Some((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_png_dimensions_from_the_ihdr() {
        let mut png = Vec::new();
        png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&1170u32.to_be_bytes());
        png.extend_from_slice(&2532u32.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0]);
        assert_eq!(png_dimensions(&png), Some((1170, 2532)));
    }

    #[test]
    fn rejects_a_non_png() {
        assert_eq!(png_dimensions(b"not a png at all, not even close"), None);
    }
}
