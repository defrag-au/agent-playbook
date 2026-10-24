//! The artefact contract: one image, one text sidecar, and the same data as typed JSON.
//!
//! Field order is stable and matches `docs/browser-automation.md` so two runs can be diffed.

use std::fmt::Write as _;

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Report {
    pub url: String,
    pub captured: String,
    pub peer: Peer,
    pub browser: String,
    pub pid: Option<u32>,
    pub mode: String,
    pub args: Vec<String>,
    pub renderer: Renderer,
    pub viewport: Viewport,
    pub layout: Option<Layout>,
    pub image: Image,
    pub ready: Ready,
    pub frame_rate: Option<FrameRate>,
    pub console: Console,
    pub requests: Requests,
    pub network: String,
    pub overlay: String,
    pub input: Vec<String>,
    pub bursts: Vec<BurstInfo>,
    pub focus: String,
    pub scroll: String,
    pub locator: String,
    pub expect: Option<Expect>,
    pub warnings: Vec<String>,
    pub profile: String,
    pub screenshot: String,
    pub sidecar: String,
    pub passed: bool,
}

#[derive(Debug, Serialize)]
pub struct Peer {
    pub index: u32,
    pub of: u32,
}

#[derive(Debug, Serialize)]
pub struct Renderer {
    pub name: String,
    pub software: bool,
}

#[derive(Debug, Serialize)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
    pub dpr: f64,
    pub touch: bool,
    pub user_agent: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct Layout {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Serialize)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub scope: String,
    pub css: Option<CssBox>,
}

#[derive(Debug, Serialize, Clone)]
pub struct CssBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Serialize)]
pub struct Ready {
    pub predicates: Vec<Predicate>,
    pub waited_ms: u64,
}

#[derive(Debug, Serialize, Clone)]
pub struct FrameRate {
    pub fps: f64,
    pub first_frame: f64,
    pub last_frame: f64,
    pub window_ms: u64,
}

#[derive(Debug, Serialize, Clone)]
pub struct Predicate {
    pub name: String,
    pub value: String,
    pub passed: bool,
    pub waited_ms: u64,
}

impl Predicate {
    pub fn passed(
        name: impl Into<String>,
        value: impl Into<String>,
        started: std::time::Instant,
    ) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            passed: true,
            waited_ms: started.elapsed().as_millis() as u64,
        }
    }

    pub fn failed(
        name: impl Into<String>,
        value: impl Into<String>,
        started: std::time::Instant,
    ) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            passed: false,
            waited_ms: started.elapsed().as_millis() as u64,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Console {
    pub errors: u32,
    pub warnings: u32,
    pub uncaught: u32,
    pub lines: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Requests {
    pub total: u32,
    pub failed: u32,
    pub lines: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct BurstInfo {
    pub mode: String,
    pub prefix: String,
    pub summary: String,
    pub frames: usize,
    pub requested_frames: usize,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub min_interval_ms: u64,
    pub median_interval_ms: u64,
    pub max_interval_ms: u64,
    pub min_capture_ms: u64,
    pub median_capture_ms: u64,
    pub max_capture_ms: u64,
    pub min_write_ms: u64,
    pub median_write_ms: u64,
    pub max_write_ms: u64,
}

impl BurstInfo {
    pub fn summary_line(&self) -> String {
        format!(
            "{} {} — {}/{} frames, {:.1} fps, interval {}/{}/{} ms, capture {}/{}/{} ms, write {}/{}/{} ms (min/median/max) — {}",
            self.mode,
            self.prefix,
            self.frames,
            self.requested_frames,
            self.fps,
            self.min_interval_ms,
            self.median_interval_ms,
            self.max_interval_ms,
            self.min_capture_ms,
            self.median_capture_ms,
            self.max_capture_ms,
            self.min_write_ms,
            self.median_write_ms,
            self.max_write_ms,
            self.summary
        )
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct Expect {
    pub expression: String,
    pub value: String,
    pub passed: bool,
}

impl Report {
    pub fn render_text(&self) -> String {
        let mut out = String::new();
        line(&mut out, "url", &self.url);
        line(&mut out, "captured", &self.captured);
        line(
            &mut out,
            "peer",
            &format!(
                "{} (of {} process{})",
                self.peer.index,
                self.peer.of,
                if self.peer.of == 1 { "" } else { "es" }
            ),
        );
        line(&mut out, "browser", &self.browser);
        line(
            &mut out,
            "pid",
            &self
                .pid
                .map_or_else(|| "not exposed".to_owned(), |pid| pid.to_string()),
        );
        line(&mut out, "mode", &self.mode);
        if !self.args.is_empty() {
            line(&mut out, "args", &self.args.join(" "));
        }
        line(
            &mut out,
            "renderer",
            &if self.renderer.software {
                format!("{} — SOFTWARE", self.renderer.name)
            } else {
                self.renderer.name.clone()
            },
        );
        line(
            &mut out,
            "viewport",
            &format!(
                "{}x{} dpr {}{}",
                self.viewport.width,
                self.viewport.height,
                self.viewport.dpr,
                if self.viewport.touch { " touch" } else { "" }
            ),
        );
        match &self.layout {
            Some(layout) => line(
                &mut out,
                "layout",
                &format!("{}x{}", layout.width, layout.height),
            ),
            None => line(&mut out, "layout", "not measured"),
        }
        line(
            &mut out,
            "image",
            &format!(
                "{}x{} ({})",
                self.image.width, self.image.height, self.image.scope
            ),
        );
        if let Some(css) = &self.image.css {
            line(
                &mut out,
                "css box",
                &format!("{}x{} at {},{}", css.width, css.height, css.x, css.y),
            );
        }

        let ready = self
            .ready
            .predicates
            .iter()
            .map(|predicate| {
                format!(
                    "{} = {}{}",
                    predicate.name,
                    predicate.value,
                    if predicate.passed { "" } else { " FAILED" }
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        line(
            &mut out,
            "ready",
            &if ready.is_empty() {
                "none".to_owned()
            } else {
                ready
            },
        );
        line(
            &mut out,
            "waited",
            &format!(
                "{} predicate{}, {} ms",
                self.ready.predicates.len(),
                if self.ready.predicates.len() == 1 {
                    ""
                } else {
                    "s"
                },
                self.ready.waited_ms
            ),
        );
        match &self.frame_rate {
            Some(rate) => line(
                &mut out,
                "frame rate",
                &format!(
                    "{:.1} Hz ({:.0} -> {:.0} frames in {} ms)",
                    rate.fps, rate.first_frame, rate.last_frame, rate.window_ms
                ),
            ),
            None => line(&mut out, "frame rate", "not measured"),
        }
        line(
            &mut out,
            "console",
            &format!(
                "{} error{}, {} warning{}, {} uncaught",
                self.console.errors,
                plural(self.console.errors),
                self.console.warnings,
                plural(self.console.warnings),
                self.console.uncaught
            ),
        );
        for message in &self.console.lines {
            let _ = writeln!(out, "{:12} {}", "", indent(message));
        }
        line(
            &mut out,
            "requests",
            &format!(
                "{} failed ({} total)",
                self.requests.failed, self.requests.total
            ),
        );
        for message in &self.requests.lines {
            let _ = writeln!(out, "{:12} {}", "", indent(message));
        }
        line(&mut out, "network", &self.network);
        line(&mut out, "overlay", &self.overlay);
        line(
            &mut out,
            "input",
            &if self.input.is_empty() {
                "none".to_owned()
            } else {
                self.input.join("; ")
            },
        );
        for burst in &self.bursts {
            line(&mut out, "burst", &burst.summary_line());
        }
        line(&mut out, "focus", &self.focus);
        line(&mut out, "scroll", &self.scroll);
        line(&mut out, "locator", &self.locator);
        if let Some(expect) = &self.expect {
            line(
                &mut out,
                "expect",
                &format!(
                    "{} = {} ({})",
                    expect.expression,
                    expect.value,
                    if expect.passed { "pass" } else { "FAIL" }
                ),
            );
        }
        line(
            &mut out,
            "warnings",
            &if self.warnings.is_empty() {
                "none".to_owned()
            } else {
                self.warnings.join("; ")
            },
        );
        line(&mut out, "profile", &self.profile);
        line(&mut out, "screenshot", &self.screenshot);
        line(
            &mut out,
            "result",
            if self.passed { "pass" } else { "FAIL" },
        );
        out
    }
}

fn line(out: &mut String, label: &str, value: &str) {
    let _ = writeln!(out, "{label:12} {value}");
}

fn indent(value: &str) -> String {
    value.replace('\n', "\n             ")
}

fn plural(count: u32) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}
