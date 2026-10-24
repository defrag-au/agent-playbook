//! Readiness predicates. Every wait is a condition with a timeout, and a timeout names the
//! predicate and the value it last saw.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use headless_chrome::browser::tab::Tab;
use serde_json::Value;

use crate::collect::{remote_text, Collector};
use crate::report::{FrameRate, Predicate};

const POLL: Duration = Duration::from_millis(50);

pub const RENDERER_JS: &str = r#"(() => {
  const canvases = Array.from(document.querySelectorAll('canvas'));
  let gl = null;
  for (const canvas of canvases) {
    gl = canvas.getContext('webgl2') || canvas.getContext('webgl');
    if (gl) break;
  }
  if (!gl) {
    const canvas = document.createElement('canvas');
    gl = canvas.getContext('webgl2') || canvas.getContext('webgl');
  }
  if (!gl) return 'no WebGL context';
  const extension = gl.getExtension('WEBGL_debug_renderer_info');
  const renderer = extension ? gl.getParameter(extension.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER);
  return gl.getParameter(gl.VERSION) + ' (' + renderer + ')';
})()"#;

pub fn renderer(tab: &Tab) -> String {
    evaluate_text(tab, RENDERER_JS)
}

pub fn is_software_renderer(renderer: &str) -> bool {
    let renderer = renderer.to_ascii_lowercase();
    ["swiftshader", "llvmpipe", "softpipe", "software"]
        .iter()
        .any(|marker| renderer.contains(marker))
}

pub fn number(tab: &Tab, expression: &str) -> Option<f64> {
    evaluate_labeled(tab, expression)
        .ok()
        .and_then(|(value, _)| value.as_f64())
}

/// How many frames a CDP round trip costs on this page. Calibrated once, before any timed
/// input, so a frame hold can compensate for the command latency instead of overshooting.
pub fn eval_latency_frames(tab: &Tab, counter: &str) -> Option<f64> {
    let before = number(tab, counter)?;
    let after = number(tab, counter)?;
    Some((after - before).max(0.0))
}

/// Measure the page's actual frame rate from its own counter and clock, so a report can say
/// what it observed instead of repeating the display's nominal refresh rate.
pub fn measure_frame_rate(tab: &Tab, counter: &str) -> Option<FrameRate> {
    let (first_at, first_frame) = frame_sample(tab, counter)?;
    std::thread::sleep(Duration::from_secs(1));
    let (last_at, last_frame) = frame_sample(tab, counter)?;
    let elapsed_ms = last_at - first_at;
    let frames = last_frame - first_frame;
    if !(elapsed_ms.is_finite() && elapsed_ms > 0.0 && frames.is_finite() && frames >= 0.0) {
        return None;
    }
    Some(FrameRate {
        fps: frames * 1000.0 / elapsed_ms,
        first_frame,
        last_frame,
        window_ms: elapsed_ms.round() as u64,
    })
}

fn frame_sample(tab: &Tab, counter: &str) -> Option<(f64, f64)> {
    let expression = format!("JSON.stringify([performance.now(), Number({counter})])");
    let object = tab.evaluate(&expression, false).ok()?;
    let text = object.value?.as_str()?.to_owned();
    let sample: Vec<f64> = serde_json::from_str(&text).ok()?;
    match sample.as_slice() {
        [at, frame] if at.is_finite() && frame.is_finite() => Some((*at, *frame)),
        _ => None,
    }
}

pub fn evaluate_labeled(tab: &Tab, expression: &str) -> Result<(Value, String)> {
    let object = tab.evaluate(expression, false)?;
    Ok((
        object.value.clone().unwrap_or(Value::Null),
        remote_text(&object),
    ))
}

pub fn evaluate_text(tab: &Tab, expression: &str) -> String {
    match tab.evaluate(expression, false) {
        Ok(object) => remote_text(&object),
        Err(error) => format!("error: {error}"),
    }
}

pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_some_and(|value| value != 0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
    }
}

pub fn wait_for_fonts(tab: &Tab, timeout: Duration) -> Predicate {
    let started = Instant::now();
    let mut status;

    loop {
        status = evaluate_text(tab, "document.fonts ? document.fonts.status : 'loaded'");
        if status == "loaded" {
            // One frame after the swap, not just the promise resolving.
            let _ = tab.evaluate(
                "new Promise(r => requestAnimationFrame(() => requestAnimationFrame(() => r(true))))",
                true,
            );
            return Predicate::passed("fonts.ready", status, started);
        }
        if started.elapsed() >= timeout {
            return Predicate::failed("fonts.ready", status, started);
        }
        std::thread::sleep(POLL);
    }
}

pub fn wait_for_predicate(tab: &Tab, expression: &str, timeout: Duration) -> Predicate {
    let started = Instant::now();
    let mut last;

    loop {
        match evaluate_labeled(tab, expression) {
            Ok((value, label)) => {
                last = label;
                if truthy(&value) {
                    return Predicate::passed(expression, last, started);
                }
            }
            Err(error) => last = format!("error: {error}"),
        }
        if started.elapsed() >= timeout {
            return Predicate::failed(expression, last, started);
        }
        std::thread::sleep(POLL);
    }
}

pub fn wait_for_frames(tab: &Tab, counter: &str, frames: u32, timeout: Duration) -> Predicate {
    let started = Instant::now();
    let mut last;
    let mut origin: Option<f64> = None;

    loop {
        match evaluate_labeled(tab, counter) {
            Ok((value, label)) => {
                last = label;
                if let Some(current) = value.as_f64() {
                    let origin = *origin.get_or_insert(current);
                    if frames == 0 || current >= origin + f64::from(frames) {
                        return Predicate::passed(
                            counter,
                            format!("{current} (waited {frames})"),
                            started,
                        );
                    }
                    last = format!("{current} (needs {})", origin + f64::from(frames));
                }
            }
            Err(error) => last = format!("error: {error}"),
        }
        if started.elapsed() >= timeout {
            return Predicate::failed(counter, last, started);
        }
        std::thread::sleep(POLL);
    }
}

pub fn wait_until_frame(tab: &Tab, counter: &str, target: f64, timeout: Duration) -> Result<()> {
    let script = format!(
        r#"(async () => {{
            const now = Number({counter});
            if (!Number.isFinite(now)) return 'missing';
            const deadline = performance.now() + {ms};
            while (Number({counter}) < {target}) {{
                if (performance.now() > deadline) return 'timeout';
                await new Promise(requestAnimationFrame);
            }}
            return 'ok';
        }})()"#,
        counter = counter,
        target = target,
        ms = timeout.as_millis()
    );
    let object = tab.evaluate(&script, true)?;
    let result = object
        .value
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default();
    match result.as_str() {
        "ok" => Ok(()),
        "timeout" => bail!(
            "frame counter {counter} did not reach {target} within {} ms",
            timeout.as_millis()
        ),
        "missing" => bail!("frame counter {counter} is unavailable"),
        other => bail!("frame wait returned {other:?}"),
    }
}

pub fn wait_for_network_idle(
    collector: &Arc<Mutex<Collector>>,
    window: Duration,
    timeout: Duration,
) -> Predicate {
    let started = Instant::now();
    let mut last;

    loop {
        let Ok(collector) = collector.lock() else {
            return Predicate::failed("network.idle", "collector poisoned", started);
        };

        if collector.idle_for(window) {
            return Predicate::passed(
                "network.idle",
                format!("0 in flight for {} ms", window.as_millis()),
                started,
            );
        }
        last = format!("{} in flight", collector.in_flight());
        drop(collector);

        if started.elapsed() >= timeout {
            return Predicate::failed("network.idle", last, started);
        }
        std::thread::sleep(POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_software_renderers() {
        assert!(is_software_renderer(
            "WebGL 2.0 (Google SwiftShader) — SOFTWARE"
        ));
        assert!(is_software_renderer("WebGL 1.0 (Mesa/llvmpipe)"));
        assert!(!is_software_renderer("WebGL 2.0 (Apple M3 Pro)"));
    }

    #[test]
    fn truthiness_matches_javascript_expectations() {
        assert!(!truthy(&Value::Null));
        assert!(!truthy(&Value::Bool(false)));
        assert!(!truthy(&Value::from(0)));
        assert!(!truthy(&Value::from("")));
        assert!(truthy(&Value::Bool(true)));
        assert!(truthy(&Value::from(1)));
        assert!(truthy(&Value::from("ready")));
    }
}
