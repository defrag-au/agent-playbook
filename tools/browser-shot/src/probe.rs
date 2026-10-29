//! Readiness predicates. Every wait is a condition with a timeout, and a timeout names the
//! predicate and the value it last saw.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use headless_chrome::browser::tab::Tab;
use serde_json::Value;

use crate::collect::{remote_text, Collector};
use crate::report::{FrameRate, Layout, Predicate};

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

pub fn layout(tab: &Tab) -> Option<Layout> {
    let text = evaluate_text(
        tab,
        "JSON.stringify([window.innerWidth, window.innerHeight])",
    );
    let values: Vec<f64> = serde_json::from_str(&text).ok()?;
    match values.as_slice() {
        [width, height] if width.is_finite() && height.is_finite() => Some(Layout {
            width: width.round().max(0.0) as u32,
            height: height.round().max(0.0) as u32,
        }),
        _ => None,
    }
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
///
/// The measurement is taken when a report is built, not at launch: a wasm app defines
/// `window.__frames` only once it has started, so a launch-time read on a real bundle fails and
/// says nothing about the rate during the run.
pub fn measure_frame_rate(tab: &Tab, counter: &str) -> Result<FrameRate> {
    let (first_at, first_frame) = frame_sample(tab, counter)
        .ok_or_else(|| anyhow::anyhow!("frame counter {counter:?} is not a finite number"))?;
    std::thread::sleep(Duration::from_secs(1));
    let (last_at, last_frame) = frame_sample(tab, counter).ok_or_else(|| {
        anyhow::anyhow!("frame counter {counter:?} stopped being a finite number")
    })?;
    let elapsed_ms = last_at - first_at;
    if !(elapsed_ms.is_finite() && elapsed_ms > 0.0) {
        bail!("performance.now() did not advance while sampling {counter:?}");
    }
    let frames = last_frame - first_frame;
    if !frames.is_finite() || frames < 0.0 {
        bail!("frame counter {counter:?} went backwards ({first_frame} -> {last_frame})");
    }
    Ok(FrameRate {
        fps: frames * 1000.0 / elapsed_ms,
        first_frame,
        last_frame,
        window_ms: elapsed_ms.round() as u64,
    })
}

/// A rate below this makes a capture or an interaction unrepresentative of what a player sees.
pub fn low_rate_warning(rate: &FrameRate) -> Option<String> {
    (rate.fps > 0.0 && rate.fps < 20.0).then(|| {
        format!(
            "measured {:.1} Hz: under 20 Hz a capture or interaction is not representative; animation and two-peer runs need --headed",
            rate.fps
        )
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

/// Evaluate an expression that returns a promise, and fail if it has not settled in time.
///
/// The timeout is a page-side `Promise.race`, not a deadline around the CDP call: blocking the
/// call is what an unsettled promise already does, so the page has to be the one that gives up.
pub fn evaluate_settled(tab: &Tab, expression: &str, timeout: Duration) -> Result<(Value, String)> {
    let script = format!(
        "Promise.race([(async () => await ({expression}))(), \
         new Promise((_, reject) => setTimeout(() => reject(new Error('browser-shot eval timeout')), {ms}))])",
        ms = timeout.as_millis()
    );
    let object = tab.evaluate(&script, true).map_err(|error| {
        if error.to_string().contains("browser-shot eval timeout") {
            anyhow::anyhow!("eval did not settle within {} ms", timeout.as_millis())
        } else {
            anyhow::anyhow!("{error}")
        }
    })?;
    if let Some(error) = error_of(&object) {
        if error.contains("browser-shot eval timeout") {
            anyhow::bail!("eval did not settle within {} ms", timeout.as_millis());
        }
        anyhow::bail!("{error}");
    }
    Ok((
        object.value.clone().unwrap_or(Value::Null),
        remote_text(&object),
    ))
}

/// A rejected promise or a thrown exception is returned as a successful `Runtime.evaluate`
/// response carrying an error object. Treat it as the failure it is.
pub fn error_of(object: &headless_chrome::protocol::cdp::Runtime::RemoteObject) -> Option<String> {
    matches!(
        object.subtype,
        Some(headless_chrome::protocol::cdp::Runtime::RemoteObjectSubtype::Error)
    )
    .then(|| remote_text(object))
}

/// Evaluate an expression and return its value as JSON text, at the precision the page produced.
///
/// `JSON.stringify` alone would silently turn a `Map` into `{}`; the page-side walk rejects that,
/// a cyclic object, a `BigInt`, a function, `undefined` and a non-finite number with the type and
/// path that could not be encoded. The JSON is returned as the page produced it rather than being
/// re-serialised in Rust, so no digits are reinterpreted on the way to disk.
pub fn evaluate_json(tab: &Tab, expression: &str, await_promise: bool) -> Result<String> {
    let call = if await_promise {
        format!(
            "return (async () => {{ try {{ return finish(await ({expression})); }} \
             catch (error) {{ return capture(error); }} }})();"
        )
    } else {
        format!(
            "try {{ return finish(({expression})); }} catch (error) {{ return capture(error); }}"
        )
    };
    let script = format!(
        r#"(() => {{
  const seen = new WeakSet();
  const fail = (what, path) => {{ throw new Error('cannot encode ' + what + ' at ' + path); }};
  const walk = (value, path) => {{
    if (value === null) return;
    const type = typeof value;
    if (type === 'bigint') fail('BigInt', path);
    if (type === 'function') fail('function', path);
    if (type === 'symbol') fail('symbol', path);
    if (type === 'undefined') fail('undefined', path);
    if (type === 'number') {{
      if (!Number.isFinite(value)) fail('non-finite number', path);
      return;
    }}
    if (type !== 'object') return;
    if (seen.has(value)) fail('a cyclic object', path);
    seen.add(value);
    if (Array.isArray(value)) {{
      for (let index = 0; index < value.length; index++) {{
        walk(value[index], path + '[' + index + ']');
      }}
    }} else {{
      const name = value.constructor && value.constructor.name;
      if (name && name !== 'Object') fail(name, path);
      for (const key of Object.keys(value)) walk(value[key], path + '.' + key);
    }}
    seen.delete(value);
  }};
  const finish = (value) => {{
    walk(value, '$');
    return JSON.stringify({{ ok: true, json: JSON.stringify(value) }});
  }};
  const capture = (error) => JSON.stringify({{ ok: false, error: String((error && error.message) || error) }});
  {call}
}})()"#
    );

    let object = tab
        .evaluate(&script, await_promise)
        .context("evaluate the expression for out=")?;
    if let Some(error) = error_of(&object) {
        bail!("{error}");
    }
    let text = remote_text(&object);
    let encoded: EncodedValue =
        serde_json::from_str(&text).with_context(|| format!("decode the encoded value: {text}"))?;
    if !encoded.ok {
        bail!("{}", encoded.error);
    }
    Ok(encoded.json)
}

#[derive(serde::Deserialize)]
struct EncodedValue {
    ok: bool,
    #[serde(default)]
    json: String,
    #[serde(default)]
    error: String,
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

/// Wait until the page sustains at least `minimum_fps` over a one-second sample.
///
/// A rate is a condition like any other: it names itself and the last value it saw when it times
/// out, so a scenario can require 30 fps rather than asserting a frame count that happens to imply
/// one on this machine.
pub fn wait_for_rate(tab: &Tab, counter: &str, minimum_fps: f64, timeout: Duration) -> Predicate {
    let started = Instant::now();
    let mut last;

    loop {
        match measure_frame_rate(tab, counter) {
            Ok(rate) => {
                last = format!("{:.1} Hz (needs {minimum_fps:.1})", rate.fps);
                if rate.fps >= minimum_fps {
                    return Predicate::passed("frame.rate", format!("{:.1} Hz", rate.fps), started);
                }
            }
            Err(error) => last = format!("error: {error}"),
        }
        if started.elapsed() >= timeout {
            return Predicate::failed("frame.rate", last, started);
        }
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

/// How a CSS selector is matched by a scenario wait (and by a pointer auto-wait).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorState {
    Present,
    Visible,
    Hidden,
    Gone,
    Enabled,
}

impl SelectorState {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "present" => Some(Self::Present),
            "visible" => Some(Self::Visible),
            "hidden" => Some(Self::Hidden),
            "gone" => Some(Self::Gone),
            "enabled" => Some(Self::Enabled),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Visible => "visible",
            Self::Hidden => "hidden",
            Self::Gone => "gone",
            Self::Enabled => "enabled",
        }
    }
}

/// Inspect one element for a selector wait. Returns a short last-observed label and whether
/// the wanted state holds. `visible` is a non-empty box, not `display:none` /
/// `visibility:hidden`, and opacity &gt; 0. `enabled` is visible and not disabled/aria-disabled.
const SELECTOR_STATE_JS: &str = r#"((selector, want) => {
  const el = document.querySelector(selector);
  if (!el) {
    return JSON.stringify({ ok: want === 'gone', last: 'absent', want });
  }
  if (want === 'gone') {
    return JSON.stringify({ ok: false, last: 'present', want });
  }
  const style = window.getComputedStyle(el);
  const rect = el.getBoundingClientRect();
  const opacity = Number(style.opacity);
  const visible = rect.width > 0 && rect.height > 0
    && style.display !== 'none'
    && style.visibility !== 'hidden'
    && Number.isFinite(opacity) && opacity > 0;
  const disabled = el.disabled === true
    || el.getAttribute('aria-disabled') === 'true'
    || el.getAttribute('disabled') !== null;
  let ok = false;
  let last = visible ? 'visible' : 'hidden';
  if (want === 'present') { ok = true; last = 'present'; }
  else if (want === 'visible') { ok = visible; }
  else if (want === 'hidden') { ok = !visible; last = visible ? 'visible' : 'hidden'; }
  else if (want === 'enabled') {
    ok = visible && !disabled;
    last = !visible ? 'hidden' : (disabled ? 'disabled' : 'enabled');
  }
  return JSON.stringify({ ok, last, want });
})"#;

pub fn wait_for_selector(
    tab: &Tab,
    selector: &str,
    state: SelectorState,
    timeout: Duration,
) -> Predicate {
    let name = format!("selector {selector:?} {}", state.as_str());
    let started = Instant::now();
    let mut last;
    let script = format!(
        "({SELECTOR_STATE_JS})({selector}, {want})",
        selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".to_owned()),
        want = serde_json::to_string(state.as_str()).unwrap_or_else(|_| "\"\"".to_owned()),
    );

    loop {
        match evaluate_labeled(tab, &script) {
            Ok((_, label)) => {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&label) {
                    last = value
                        .get("last")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&label)
                        .to_owned();
                    if value.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                        return Predicate::passed(name, last, started);
                    }
                } else {
                    last = label;
                }
            }
            Err(error) => last = format!("error: {error}"),
        }
        if started.elapsed() >= timeout {
            return Predicate::failed(name, last, started);
        }
        std::thread::sleep(POLL);
    }
}

/// Wait until visible text contains `text`. When `selector` is set, only that element's
/// `innerText` is searched; otherwise the document body is.
pub fn wait_for_text(
    tab: &Tab,
    text: &str,
    selector: Option<&str>,
    timeout: Duration,
) -> Predicate {
    let name = match selector {
        Some(selector) => format!("text {text:?} in {selector:?}"),
        None => format!("text {text:?}"),
    };
    let started = Instant::now();
    let mut last;
    let script = format!(
        r#"((needle, selector) => {{
  const root = selector ? document.querySelector(selector) : document.body;
  if (!root) return JSON.stringify({{ ok: false, last: selector ? 'scope absent' : 'no body' }});
  const haystack = root.innerText || '';
  const ok = haystack.includes(needle);
  const preview = haystack.length > 120 ? haystack.slice(0, 117) + '…' : haystack;
  return JSON.stringify({{ ok, last: ok ? 'matched' : (preview || '(empty)') }});
}})({needle}, {selector})"#,
        needle = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned()),
        selector = selector
            .map(|s| serde_json::to_string(s).unwrap_or_else(|_| "null".to_owned()))
            .unwrap_or_else(|| "null".to_owned()),
    );

    loop {
        match evaluate_labeled(tab, &script) {
            Ok((_, label)) => {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&label) {
                    last = value
                        .get("last")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&label)
                        .to_owned();
                    if value.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                        return Predicate::passed(name, last, started);
                    }
                } else {
                    last = label;
                }
            }
            Err(error) => last = format!("error: {error}"),
        }
        if started.elapsed() >= timeout {
            return Predicate::failed(name, last, started);
        }
        std::thread::sleep(POLL);
    }
}

pub fn wait_for_url(
    tab: &Tab,
    exact: Option<&str>,
    contains: Option<&str>,
    timeout: Duration,
) -> Predicate {
    let name = if let Some(exact) = exact {
        format!("url {exact:?}")
    } else {
        format!("url~ {:?}", contains.unwrap_or(""))
    };
    let started = Instant::now();
    let mut last;

    loop {
        match evaluate_labeled(tab, "location.href") {
            Ok((_, href)) => {
                last = href.clone();
                let ok = if let Some(exact) = exact {
                    href == exact
                } else if let Some(contains) = contains {
                    href.contains(contains)
                } else {
                    false
                };
                if ok {
                    return Predicate::passed(name, last, started);
                }
            }
            Err(error) => last = format!("error: {error}"),
        }
        if started.elapsed() >= timeout {
            return Predicate::failed(name, last, started);
        }
        std::thread::sleep(POLL);
    }
}

/// Wait until there are no DOM mutations for `quiet` under the document (or under `selector`).
pub fn wait_for_stable(
    tab: &Tab,
    quiet: Duration,
    selector: Option<&str>,
    timeout: Duration,
) -> Predicate {
    let name = match selector {
        Some(selector) => format!("stable {}ms in {selector:?}", quiet.as_millis()),
        None => format!("stable {}ms", quiet.as_millis()),
    };
    let started = Instant::now();
    let script = format!(
        r#"(async (quietMs, timeoutMs, selector) => {{
  const root = selector ? document.querySelector(selector) : document.documentElement;
  if (!root) return JSON.stringify({{ ok: false, last: 'scope absent' }});
  let last = performance.now();
  const observer = new MutationObserver(() => {{ last = performance.now(); }});
  observer.observe(root, {{ subtree: true, childList: true, attributes: true, characterData: true }});
  const deadline = performance.now() + timeoutMs;
  while (performance.now() - last < quietMs) {{
    if (performance.now() > deadline) {{
      observer.disconnect();
      return JSON.stringify({{
        ok: false,
        last: 'still mutating; last change ' + Math.round(performance.now() - last) + 'ms ago'
      }});
    }}
    await new Promise((resolve) => setTimeout(resolve, 50));
  }}
  observer.disconnect();
  return JSON.stringify({{ ok: true, last: 'quiet for ' + quietMs + 'ms' }});
}})({quiet}, {timeout}, {selector})"#,
        quiet = quiet.as_millis(),
        timeout = timeout.as_millis(),
        selector = selector
            .map(|s| serde_json::to_string(s).unwrap_or_else(|_| "null".to_owned()))
            .unwrap_or_else(|| "null".to_owned()),
    );

    match tab.evaluate(&script, true) {
        Ok(object) => {
            let label = object
                .value
                .map(|value| match value {
                    Value::String(text) => text,
                    other => other.to_string(),
                })
                .unwrap_or_else(|| "no value".to_owned());
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&label) {
                let last = value
                    .get("last")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&label)
                    .to_owned();
                if value.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                    Predicate::passed(name, last, started)
                } else {
                    Predicate::failed(name, last, started)
                }
            } else {
                Predicate::failed(name, label, started)
            }
        }
        Err(error) => Predicate::failed(name, format!("error: {error}"), started),
    }
}
