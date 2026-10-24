//! `browser-shot` — one screenshot from a browser the tool launches itself.
//!
//! The spike behind `tools/browser-shot/README.md`. It proves the four properties the Node
//! loop leaves to prose: the tool owns the browser process, the viewport is a real layout
//! viewport, the wait is condition-based instead of a fixed sleep, and every run writes a
//! report beside the image.

use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use headless_chrome::browser::tab::point::Point;
use headless_chrome::browser::tab::Tab;
use headless_chrome::protocol::cdp::types::Event;
use headless_chrome::protocol::cdp::Emulation;
use headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption;
use headless_chrome::protocol::cdp::Runtime::RemoteObject;
use headless_chrome::{Browser, LaunchOptions};

const DEFAULT_BRAVE: &str = "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser";
const SAMPLE_MS: u64 = 250;
const STABLE_SAMPLES: u32 = 2;
const MAX_CONSOLE_LINES: usize = 50;
const KNOWN_NOISE: &[&str] = &["GPU stall due to ReadPixels"];

#[derive(Debug, Clone, Copy)]
enum Action {
    Click(f64, f64),
    Hover(f64, f64),
}

struct Options {
    url: String,
    width: u32,
    height: u32,
    out: PathBuf,
    report: PathBuf,
    brave: PathBuf,
    settle_ms: u64,
    timeout_ms: u64,
    no_settle: bool,
    keep_profile: bool,
    actions: Vec<Action>,
}

struct Settle {
    outcome: Outcome,
    elapsed: Duration,
    samples: u32,
    identical: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Stable,
    Unstable,
    Skipped,
}

#[derive(Debug, PartialEq, Eq)]
enum Decision {
    Continue,
    Settled,
    TimedOut,
}

struct Stability {
    previous: Vec<u8>,
    samples: u32,
    identical: u32,
}

impl Stability {
    fn new(first: Vec<u8>) -> Self {
        Self {
            previous: first,
            samples: 1,
            identical: 0,
        }
    }

    fn observe(
        &mut self,
        frame: Vec<u8>,
        elapsed: Duration,
        min_settle: Duration,
        timeout: Duration,
    ) -> Decision {
        self.samples += 1;
        if frame == self.previous {
            self.identical += 1;
        } else {
            self.identical = 0;
        }
        self.previous = frame;

        if self.identical >= STABLE_SAMPLES && elapsed >= min_settle {
            Decision::Settled
        } else if elapsed >= timeout {
            Decision::TimedOut
        } else {
            Decision::Continue
        }
    }

    fn into_frame(self) -> Vec<u8> {
        self.previous
    }
}

struct Profile {
    path: PathBuf,
    keep: bool,
}

impl Profile {
    fn new(keep: bool) -> Result<Self> {
        let path = std::env::current_dir()?
            .join(".tmp")
            .join(format!("browser-shot-{}", std::process::id()));
        fs::create_dir_all(&path)
            .with_context(|| format!("create profile directory {}", path.display()))?;
        Ok(Self { path, keep })
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn main() -> Result<()> {
    run(parse_args()?)
}

fn parse_args() -> Result<Options> {
    let mut positional = Vec::new();
    let mut actions = Vec::new();
    let mut brave = PathBuf::from(DEFAULT_BRAVE);
    let mut report = None;
    let mut settle_ms = 1200;
    let mut timeout_ms = 30_000;
    let mut no_settle = false;
    let mut keep_profile = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--click" => {
                let value = take(&mut args, "--click")?;
                let (x, y) = parse_point(&value)?;
                actions.push(Action::Click(x, y));
            }
            "--hover" => {
                let value = take(&mut args, "--hover")?;
                let (x, y) = parse_point(&value)?;
                actions.push(Action::Hover(x, y));
            }
            "--settle-ms" => settle_ms = take(&mut args, "--settle-ms")?.parse()?,
            "--timeout-ms" => timeout_ms = take(&mut args, "--timeout-ms")?.parse()?,
            "--report" => report = Some(PathBuf::from(take(&mut args, "--report")?)),
            "--brave" => brave = PathBuf::from(take(&mut args, "--brave")?),
            "--keep-profile" => keep_profile = true,
            "--no-settle" => no_settle = true,
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            other if other.starts_with("--") => bail!("unknown option {other}; try --help"),
            other => positional.push(other.to_owned()),
        }
    }

    if positional.len() != 4 {
        print_usage();
        bail!(
            "expected <url> <width> <height> <out.png>, got {} argument(s)",
            positional.len()
        );
    }

    let width: u32 = positional[1].parse().context("width")?;
    let height: u32 = positional[2].parse().context("height")?;
    if width == 0 || height == 0 {
        bail!("width and height must be greater than zero");
    }

    let out = PathBuf::from(&positional[3]);
    let report = report.unwrap_or_else(|| out.with_extension("txt"));

    Ok(Options {
        url: positional[0].clone(),
        width,
        height,
        out,
        report,
        brave,
        settle_ms,
        timeout_ms,
        no_settle,
        keep_profile,
        actions,
    })
}

fn take(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String> {
    args.next().ok_or_else(|| anyhow!("{flag} needs a value"))
}

fn parse_point(value: &str) -> Result<(f64, f64)> {
    let (x, y) = value
        .split_once(',')
        .ok_or_else(|| anyhow!("expected x,y, got {value:?}"))?;
    let x = x
        .trim()
        .parse()
        .with_context(|| format!("x in {value:?}"))?;
    let y = y
        .trim()
        .parse()
        .with_context(|| format!("y in {value:?}"))?;
    Ok((x, y))
}

fn print_usage() {
    println!(
        "browser-shot <url> <width> <height> <out.png> [options]

  --click x,y         move the pointer there and click (repeatable)
  --hover x,y         move the pointer there without clicking (repeatable)
  --settle-ms N       minimum time before a frame may be accepted (default 1200)
  --timeout-ms N      give up waiting for a stable frame after N ms (default 30000)
  --no-settle         wait --settle-ms, then capture once
  --report PATH       report path (default: the image with a .txt extension)
  --brave PATH        browser executable (default: {DEFAULT_BRAVE})
  --keep-profile      leave .tmp/browser-shot-<pid> behind for inspection
  -h, --help          print this"
    );
}

fn run(options: Options) -> Result<()> {
    if !options.brave.exists() {
        bail!(
            "browser not found at {}; pass --brave <path>",
            options.brave.display()
        );
    }

    // Declared before the browser so it drops after the child has been reaped.
    let profile = Profile::new(options.keep_profile)?;

    let launch = LaunchOptions::default_builder()
        .path(Some(options.brave.clone()))
        .user_data_dir(Some(profile.path.clone()))
        .headless(true)
        .ignore_certificate_errors(false)
        .idle_browser_timeout(Duration::from_secs(300))
        .args(vec![
            OsStr::new("--use-gl=swiftshader"),
            OsStr::new("--enable-unsafe-swiftshader"),
            OsStr::new("--hide-scrollbars"),
            OsStr::new("--disable-component-update"),
            OsStr::new("--no-first-run"),
        ])
        .build()
        .map_err(|error| anyhow!("invalid launch options: {error}"))?;

    let browser = Browser::new(launch).context("launch the browser")?;
    let tab = browser.new_tab().context("open a tab")?;

    tab.enable_log()
        .context("enable the Log domain")?
        .enable_runtime()
        .context("enable the Runtime domain")?;

    let console: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let sink = Arc::clone(&console);
        tab.add_event_listener(Arc::new(move |event: &Event| match event {
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
                    push_bounded(&sink, format!("[console {level}] {text}"));
                }
            }
            Event::LogEntryAdded(event) => {
                let entry = &event.params.entry;
                let level = format!("{:?}", entry.level).to_ascii_lowercase();
                if level == "error" || level == "warning" {
                    let source = format!("{:?}", entry.source).to_ascii_lowercase();
                    if !KNOWN_NOISE.iter().any(|noise| entry.text.contains(noise)) {
                        push_bounded(&sink, format!("[{level} {source}] {}", entry.text));
                    }
                }
            }
            Event::RuntimeExceptionThrown(event) => {
                let details = &event.params.exception_details;
                let text = details
                    .exception
                    .as_ref()
                    .and_then(|exception| exception.description.clone())
                    .unwrap_or_else(|| details.text.clone());
                push_bounded(&sink, format!("[uncaught] {text}"));
            }
            _ => {}
        }))
        .context("subscribe to console events")?;
    }

    tab.call_method(Emulation::SetDeviceMetricsOverride {
        width: options.width,
        height: options.height,
        device_scale_factor: 1.0,
        mobile: true,
        scale: None,
        screen_width: None,
        screen_height: None,
        position_x: None,
        position_y: None,
        dont_set_visible_size: None,
        screen_orientation: None,
        viewport: None,
        display_feature: None,
        device_posture: None,
    })
    .context("set the viewport")?;

    tab.navigate_to(&options.url)
        .with_context(|| format!("navigate to {}", options.url))?;
    tab.wait_until_navigated().context("wait for navigation")?;

    for action in &options.actions {
        match action {
            Action::Click(x, y) => {
                tab.click_point(Point { x: *x, y: *y }).context("click")?;
                std::thread::sleep(Duration::from_millis(800));
            }
            Action::Hover(x, y) => {
                tab.move_mouse_to_point(Point { x: *x, y: *y })
                    .context("hover")?;
                std::thread::sleep(Duration::from_millis(2500));
            }
        }
    }

    let started = Instant::now();
    let (png, settle) = if options.no_settle {
        std::thread::sleep(Duration::from_millis(options.settle_ms));
        let png = capture(&tab)?;
        (
            png,
            Settle {
                outcome: Outcome::Skipped,
                elapsed: started.elapsed(),
                samples: 1,
                identical: 0,
            },
        )
    } else {
        wait_for_stable(&tab, options.settle_ms, options.timeout_ms)?
    };

    if let Some(parent) = options.out.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&options.out, &png).with_context(|| format!("write {}", options.out.display()))?;

    if let Some(parent) = options.report.parent() {
        fs::create_dir_all(parent)?;
    }
    let report = render_report(&options, &profile, &tab, &png, &settle, &console);
    fs::write(&options.report, report)
        .with_context(|| format!("write {}", options.report.display()))?;

    println!(
        "{} @ {}x{} ({})",
        options.out.display(),
        options.width,
        options.height,
        match settle.outcome {
            Outcome::Stable => "stable",
            Outcome::Unstable => "UNSETTLED",
            Outcome::Skipped => "settle skipped",
        }
    );

    if settle.outcome == Outcome::Unstable {
        bail!(
            "no stable frame within {} ms; the image and report were still written",
            settle.elapsed.as_millis()
        );
    }

    Ok(())
}

fn capture(tab: &Tab) -> Result<Vec<u8>> {
    tab.capture_screenshot(CaptureScreenshotFormatOption::Png, None, None, true)
        .context("capture a screenshot")
}

fn wait_for_stable(tab: &Tab, min_settle_ms: u64, timeout_ms: u64) -> Result<(Vec<u8>, Settle)> {
    let started = Instant::now();
    let mut stability = Stability::new(capture(tab)?);
    let min_settle = Duration::from_millis(min_settle_ms);
    let timeout = Duration::from_millis(timeout_ms);

    loop {
        std::thread::sleep(Duration::from_millis(SAMPLE_MS));
        let current = capture(tab)?;
        let elapsed = started.elapsed();
        let decision = stability.observe(current, elapsed, min_settle, timeout);

        if decision == Decision::Continue {
            continue;
        }

        let settle = Settle {
            outcome: if decision == Decision::Settled {
                Outcome::Stable
            } else {
                Outcome::Unstable
            },
            elapsed,
            samples: stability.samples,
            identical: stability.identical,
        };
        return Ok((stability.into_frame(), settle));
    }
}

fn remote_text(object: &RemoteObject) -> String {
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
    format!("{:?}", object.Type)
}

fn push_bounded(sink: &Arc<Mutex<Vec<String>>>, line: String) {
    if let Ok(mut lines) = sink.lock() {
        if lines.len() < MAX_CONSOLE_LINES {
            lines.push(line);
        }
    }
}

fn render_report(
    options: &Options,
    profile: &Profile,
    tab: &Tab,
    png: &[u8],
    settle: &Settle,
    console: &Arc<Mutex<Vec<String>>>,
) -> String {
    let mut report = String::new();
    let title = tab
        .get_title()
        .unwrap_or_else(|_| "<unavailable>".to_owned());

    let _ = writeln!(report, "browser-shot · {}", options.url);
    let _ = writeln!(
        report,
        "viewport: {}x{} @ 1.0 (mobile emulation)",
        options.width, options.height
    );
    let _ = writeln!(report, "title: {title}");
    let _ = writeln!(report, "url: {}", tab.get_url());
    let _ = writeln!(
        report,
        "settled: {} after {} ms ({} samples, {} consecutive identical)",
        match settle.outcome {
            Outcome::Stable => "yes",
            Outcome::Unstable => "NO",
            Outcome::Skipped => "skipped (--no-settle)",
        },
        settle.elapsed.as_millis(),
        settle.samples,
        settle.identical
    );
    let _ = writeln!(
        report,
        "screenshot: {} · {} bytes",
        options.out.display(),
        png.len()
    );
    if png.is_empty() {
        let _ = writeln!(report, "warning: the screenshot is empty");
    }
    let _ = writeln!(
        report,
        "profile: {} ({})",
        profile.path.display(),
        if profile.keep {
            "kept"
        } else {
            "removed on exit"
        }
    );

    let lines = console
        .lock()
        .map(|lines| lines.clone())
        .unwrap_or_default();
    if lines.is_empty() {
        let _ = writeln!(report, "console: no errors or warnings");
    } else {
        let _ = writeln!(report, "console: {} line(s)", lines.len());
        for line in &lines {
            let _ = writeln!(report, "  {line}");
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observe(stability: &mut Stability, frame: u8, ms: u64) -> Decision {
        stability.observe(
            vec![frame],
            Duration::from_millis(ms),
            Duration::from_millis(1200),
            Duration::from_millis(5000),
        )
    }

    #[test]
    fn an_unchanged_frame_settles_only_after_the_minimum() {
        let mut stability = Stability::new(vec![1]);
        assert_eq!(observe(&mut stability, 1, 250), Decision::Continue);
        assert_eq!(observe(&mut stability, 1, 500), Decision::Continue);
        assert_eq!(observe(&mut stability, 1, 1200), Decision::Settled);
    }

    #[test]
    fn a_changing_frame_times_out() {
        let mut stability = Stability::new(vec![1]);
        assert_eq!(observe(&mut stability, 2, 250), Decision::Continue);
        assert_eq!(observe(&mut stability, 3, 5000), Decision::TimedOut);
    }

    #[test]
    fn parses_a_point() {
        assert_eq!(parse_point("12.5, -3").unwrap(), (12.5, -3.0));
    }

    #[test]
    fn rejects_a_malformed_point() {
        assert!(parse_point("12").is_err());
    }
}
