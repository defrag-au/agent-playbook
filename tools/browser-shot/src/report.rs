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
    pub mode: String,
    pub renderer: Renderer,
    pub viewport: Viewport,
    pub image: Image,
    pub ready: Ready,
    pub console: Console,
    pub requests: Requests,
    pub network: String,
    pub overlay: String,
    pub input: Vec<String>,
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

#[derive(Debug, Serialize)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub scope: String,
    pub css: Option<CssBox>,
}

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
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
        line(&mut out, "mode", &self.mode);
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
