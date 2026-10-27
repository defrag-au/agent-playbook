//! Console, exception and network-failure collection, plus the in-flight count that makes
//! `wait_for_network_idle` a condition rather than a sleep.
//!
//! It also follows the main frame's lifecycle and the renderer's health, so a navigation that
//! never completes can say whether the page crashed, hung, or is still loading.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use headless_chrome::protocol::cdp::types::Event;
use headless_chrome::protocol::cdp::Runtime::RemoteObject;

const MAX_LINES: usize = 50;
const KNOWN_NOISE: &[&str] = &["GPU stall due to ReadPixels"];

#[derive(Debug, Default)]
pub struct Snapshot {
    pub console_errors: u32,
    pub console_warnings: u32,
    pub uncaught: u32,
    pub console_lines: Vec<String>,
    pub request_lines: Vec<String>,
    pub requests_total: u32,
    pub requests_failed: u32,
}

#[derive(Debug)]
pub struct Collector {
    started: Instant,
    console_errors: u32,
    console_warnings: u32,
    uncaught: u32,
    console_lines: Vec<String>,
    request_lines: Vec<String>,
    requests_total: u32,
    requests_failed: u32,
    in_flight: HashMap<String, String>,
    last_activity: Instant,
    main_frame: Option<String>,
    navigating: bool,
    lifecycle: Vec<String>,
    painted: bool,
    crash: Option<String>,
}

impl Collector {
    pub fn new(started: Instant) -> Self {
        Self {
            started,
            console_errors: 0,
            console_warnings: 0,
            uncaught: 0,
            console_lines: Vec::new(),
            request_lines: Vec::new(),
            requests_total: 0,
            requests_failed: 0,
            in_flight: HashMap::new(),
            last_activity: started,
            main_frame: None,
            navigating: false,
            lifecycle: Vec::new(),
            painted: false,
            crash: None,
        }
    }

    /// Start following a navigation of `main_frame`. In Chromium a page target's main frame id is
    /// its target id, so lifecycle events from iframes are ignored.
    ///
    /// The navigation is complete at the main frame's `networkAlmostIdle`, the same event
    /// `headless_chrome`'s `wait_until_navigated` waits for.
    pub fn begin_navigation(&mut self, main_frame: &str) {
        self.main_frame = Some(main_frame.to_owned());
        self.navigating = true;
        self.lifecycle.clear();
    }

    pub fn navigating(&self) -> bool {
        self.navigating
    }

    /// The main frame's lifecycle events since the navigation began, in arrival order.
    pub fn lifecycle(&self) -> &[String] {
        &self.lifecycle
    }

    /// Whether the main frame reported a first paint, i.e. a screenshot may show something.
    pub fn painted(&self) -> bool {
        self.painted
    }

    /// Why the renderer is gone, if it is.
    pub fn crash(&self) -> Option<&str> {
        self.crash.as_deref()
    }

    /// URLs of requests that started and have not finished, sorted for a stable report.
    pub fn in_flight_urls(&self) -> Vec<String> {
        let mut urls = self.in_flight.values().cloned().collect::<Vec<_>>();
        urls.sort();
        urls
    }

    pub fn handle(&mut self, event: &Event) {
        let at = self.started.elapsed().as_millis();

        match event {
            Event::RuntimeConsoleAPICalled(event) => {
                let level = format!("{:?}", event.params.Type).to_ascii_lowercase();
                if level == "error" || level == "warning" {
                    let text = event
                        .params
                        .args
                        .iter()
                        .map(remote_text)
                        .collect::<Vec<_>>()
                        .join(" ");
                    if level == "error" {
                        self.console_errors += 1;
                    } else {
                        self.console_warnings += 1;
                    }
                    push(
                        &mut self.console_lines,
                        format!("[+{at}ms] console {level}: {text}"),
                    );
                }
            }
            Event::LogEntryAdded(event) => {
                let entry = &event.params.entry;
                let level = format!("{:?}", entry.level).to_ascii_lowercase();
                if (level == "error" || level == "warning")
                    && !KNOWN_NOISE.iter().any(|noise| entry.text.contains(noise))
                {
                    let source = format!("{:?}", entry.source).to_ascii_lowercase();
                    if level == "error" {
                        self.console_errors += 1;
                    } else {
                        self.console_warnings += 1;
                    }
                    push(
                        &mut self.console_lines,
                        format!("[+{at}ms] {source} {level}: {}", entry.text),
                    );
                }
            }
            Event::RuntimeExceptionThrown(event) => {
                self.uncaught += 1;
                let details = &event.params.exception_details;
                let text = details
                    .exception
                    .as_ref()
                    .and_then(|exception| exception.description.clone())
                    .unwrap_or_else(|| details.text.clone());
                push(
                    &mut self.console_lines,
                    format!("[+{at}ms] uncaught: {text}"),
                );
            }
            Event::NetworkRequestWillBeSent(event) => {
                let id = event.params.request_id.clone();
                let url = event.params.request.url.clone();
                self.in_flight.insert(id, url);
                self.requests_total += 1;
                self.last_activity = Instant::now();
            }
            Event::NetworkLoadingFinished(event) => {
                self.in_flight.remove(&event.params.request_id);
                self.last_activity = Instant::now();
            }
            Event::NetworkLoadingFailed(event) => {
                let id = &event.params.request_id;
                let url = self.in_flight.remove(id).unwrap_or_default();
                self.last_activity = Instant::now();
                if event.params.canceled != Some(true) {
                    self.requests_failed += 1;
                    push(
                        &mut self.request_lines,
                        format!("[+{at}ms] failed {url} — {}", event.params.error_text),
                    );
                }
            }
            Event::NetworkResponseReceived(event) => {
                self.last_activity = Instant::now();
                let status = event.params.response.status;
                if status >= 400 {
                    self.requests_failed += 1;
                    push(
                        &mut self.request_lines,
                        format!("[+{at}ms] HTTP {status} {}", event.params.response.url),
                    );
                }
            }
            Event::PageLifecycleEvent(event) => {
                let params = &event.params;
                let main = self
                    .main_frame
                    .as_deref()
                    .is_none_or(|frame| frame == params.frame_id);
                if main {
                    match params.name.as_str() {
                        "init" => {
                            self.navigating = true;
                            self.lifecycle.clear();
                        }
                        "networkAlmostIdle" => self.navigating = false,
                        "firstPaint" | "firstContentfulPaint" | "firstMeaningfulPaint" => {
                            self.painted = true;
                        }
                        _ => {}
                    }
                    if self.lifecycle.len() < MAX_LINES {
                        self.lifecycle.push(format!("{}@{at}ms", params.name));
                    }
                }
            }
            Event::InspectorTargetCrashed(_) => {
                self.crash = Some("renderer crashed (Inspector.targetCrashed)".to_owned());
                push(
                    &mut self.console_lines,
                    format!("[+{at}ms] renderer crashed (Inspector.targetCrashed)"),
                );
            }
            Event::InspectorDetached(event) => {
                if self.crash.is_none() {
                    self.crash = Some(format!(
                        "inspector detached (Inspector.detached: {})",
                        event.params.reason
                    ));
                }
                push(
                    &mut self.console_lines,
                    format!("[+{at}ms] inspector detached: {}", event.params.reason),
                );
            }
            _ => {}
        }
    }

    pub fn idle_for(&self, window: Duration) -> bool {
        self.in_flight.is_empty() && self.last_activity.elapsed() >= window
    }

    pub fn in_flight(&self) -> usize {
        self.in_flight.len()
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            console_errors: self.console_errors,
            console_warnings: self.console_warnings,
            uncaught: self.uncaught,
            console_lines: self.console_lines.clone(),
            request_lines: self.request_lines.clone(),
            requests_total: self.requests_total,
            requests_failed: self.requests_failed,
        }
    }
}

pub fn shared(started: Instant) -> Arc<Mutex<Collector>> {
    Arc::new(Mutex::new(Collector::new(started)))
}

fn push(lines: &mut Vec<String>, line: String) {
    if lines.len() < MAX_LINES {
        lines.push(line);
    }
}

pub(crate) fn remote_text(object: &RemoteObject) -> String {
    if let Some(description) = &object.description {
        return description.clone();
    }
    if let Some(value) = &object.value {
        return match value {
            serde_json::Value::String(text) => text.clone(),
            other => other.to_string(),
        };
    }
    if let Some(value) = &object.unserializable_value {
        return format!("{value:?}");
    }
    format!("{:?}", object.Type).to_ascii_lowercase()
}
