//! `browser-shot` — one screenshot from a browser the tool launches itself, with the sidecar
//! contract from `docs/browser-automation.md`.
//!
//! Single peer only. The two-peer, network-emulation and loss-hook requirements are deliberately
//! out of scope until a process needs them.

mod capture;
mod collect;
mod emulation;
mod input;
mod keys;
mod probe;
mod report;
mod runner;

use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
#[cfg(unix)]
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use chrono::{SecondsFormat, Utc};
use headless_chrome::browser::tab::Tab;
use headless_chrome::protocol::cdp::{Network, Page};
use headless_chrome::{Browser, LaunchOptions};

use capture::Captured;
use collect::Snapshot;
use input::{Action, Pointer};
use report::{
    Console, CssBox, Expect, Image, Peer, Predicate, Ready, Renderer, Report, Requests, Viewport,
};

const DEFAULT_BROWSER: &str = "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser";
const SAMPLE_MS: u64 = 250;
const STABLE_SAMPLES: u32 = 2;

struct Options {
    url: String,
    width: u32,
    height: u32,
    dpr: f64,
    touch: bool,
    user_agent: Option<String>,
    select: Option<String>,
    out: PathBuf,
    report: PathBuf,
    browser: PathBuf,
    settle_ms: u64,
    timeout_ms: u64,
    no_settle: bool,
    keep_profile: bool,
    headed: bool,
    preload: Option<PathBuf>,
    ready: Option<String>,
    frames: Option<u32>,
    frame_counter: String,
    network_idle_ms: Option<u64>,
    expect: Option<String>,
    json: bool,
    steps: u32,
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
}

#[cfg(unix)]
static CHILD_PID: AtomicU32 = AtomicU32::new(0);

#[cfg(unix)]
extern "C" fn handle_signal(_: libc::c_int) {
    let pid = CHILD_PID.load(Ordering::SeqCst);
    if pid != 0 {
        // Async-signal-safe: the browser is the only process this tool started.
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
    }
    unsafe { libc::_exit(130) };
}

#[cfg(unix)]
fn install_signal_handlers(pid: u32) {
    CHILD_PID.store(pid, Ordering::SeqCst);
    unsafe {
        libc::signal(
            libc::SIGINT,
            handle_signal as *const () as libc::sighandler_t,
        );
        libc::signal(
            libc::SIGTERM,
            handle_signal as *const () as libc::sighandler_t,
        );
    }
}

#[cfg(not(unix))]
fn install_signal_handlers(_pid: u32) {}

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

fn main() {
    if std::env::args().nth(1).as_deref() == Some("run") {
        let args: Vec<String> = std::env::args().skip(2).collect();
        let code = match runner::run(args) {
            Ok(true) => 0,
            Ok(false) => 2,
            Err(error) => {
                eprintln!("browser-shot run: {error:#}");
                1
            }
        };
        std::process::exit(code);
    }

    let code = match parse_args().and_then(run) {
        Ok(true) => 0,
        Ok(false) => 2,
        Err(error) => {
            eprintln!("browser-shot: {error:#}");
            1
        }
    };
    std::process::exit(code);
}

fn parse_args() -> Result<Options> {
    let mut positional = Vec::new();
    let mut actions = Vec::new();
    let mut browser = PathBuf::from(DEFAULT_BROWSER);
    let mut report = None;
    let mut settle_ms = 1200;
    let mut timeout_ms = 30_000;
    let mut no_settle = false;
    let mut keep_profile = false;
    let mut dpr = 1.0;
    let mut touch = false;
    let mut user_agent = None;
    let mut select = None;
    let mut headed = false;
    let mut preload = None;
    let mut ready = None;
    let mut frames = None;
    let mut frame_counter = String::from("window.__frames");
    let mut network_idle_ms = None;
    let mut expect = None;
    let mut json = false;
    let mut steps = 1;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--click" => {
                let value = take(&mut args, "--click")?;
                let (x, y) = parse_point(&value)?;
                actions.push(Action::Click { x, y });
            }
            "--hover" => {
                let value = take(&mut args, "--hover")?;
                let (x, y) = parse_point(&value)?;
                actions.push(Action::Hover { x, y });
            }
            "--click-selector" => {
                actions.push(Action::ClickSelector(take(&mut args, "--click-selector")?));
            }
            "--hover-selector" => {
                actions.push(Action::HoverSelector(take(&mut args, "--hover-selector")?));
            }
            "--steps" => steps = take(&mut args, "--steps")?.parse()?,
            "--dpr" => dpr = take(&mut args, "--dpr")?.parse()?,
            "--touch" => touch = true,
            "--user-agent" => user_agent = Some(take(&mut args, "--user-agent")?),
            "--select" => select = Some(take(&mut args, "--select")?),
            "--settle-ms" => settle_ms = take(&mut args, "--settle-ms")?.parse()?,
            "--timeout-ms" => timeout_ms = take(&mut args, "--timeout-ms")?.parse()?,
            "--report" => report = Some(PathBuf::from(take(&mut args, "--report")?)),
            "--browser" => browser = PathBuf::from(take(&mut args, "--browser")?),
            "--keep-profile" => keep_profile = true,
            "--no-settle" => no_settle = true,
            "--headed" => headed = true,
            "--preload" => preload = Some(PathBuf::from(take(&mut args, "--preload")?)),
            "--ready" => ready = Some(take(&mut args, "--ready")?),
            "--frames" => frames = Some(take(&mut args, "--frames")?.parse()?),
            "--frame-counter" => frame_counter = take(&mut args, "--frame-counter")?,
            "--network-idle" => network_idle_ms = Some(take(&mut args, "--network-idle")?.parse()?),
            "--expect" => expect = Some(take(&mut args, "--expect")?),
            "--json" => json = true,
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
    if dpr <= 0.0 {
        bail!("--dpr must be greater than zero");
    }
    if steps == 0 {
        bail!("--steps must be at least one");
    }

    let out = PathBuf::from(&positional[3]);
    let report = report.unwrap_or_else(|| out.with_extension("txt"));

    Ok(Options {
        url: positional[0].clone(),
        width,
        height,
        dpr,
        touch,
        user_agent,
        select,
        out,
        report,
        browser,
        settle_ms,
        timeout_ms,
        no_settle,
        keep_profile,
        headed,
        preload,
        ready,
        frames,
        frame_counter,
        network_idle_ms,
        expect,
        json,
        steps,
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

  --dpr N                 device scale factor (default 1)
  --touch                 enable touch emulation (with mobile metrics)
  --user-agent UA         override the user agent
  --select CSS            capture this element instead of the viewport
  --ready EXPR            wait until this JS expression is truthy
  --frames N              wait for N more frames of --frame-counter
  --frame-counter EXPR    frame counter expression (default window.__frames)
  --network-idle MS       wait for no in-flight requests for MS
  --preload PATH          run this JS before the document's own scripts
  --expect EXPR           assert this JS expression after readiness
  --click x,y             move there and click (repeatable)
  --hover x,y             move there without clicking (repeatable)
  --click-selector CSS    click the centre of this element (repeatable)
  --hover-selector CSS    hover the centre of this element (repeatable)
  --steps N               interpolate each pointer move over N steps (default 1)
  --settle-ms N           minimum time before a frame may be accepted (default 1200)
  --timeout-ms N          timeout for every wait (default 30000)
  --no-settle             skip the stability loop; wait --settle-ms, then capture once
  --headed                run with a window instead of headless
  --report PATH           sidecar path (default: the image with a .txt extension)
  --browser PATH          browser executable (default: {DEFAULT_BROWSER})
  --keep-profile          leave .tmp/browser-shot-<pid> behind
  --json                  print the typed report to stdout
  -h, --help              print this

Exit codes: 0 pass, 2 artifact written but an assertion failed, 1 operational error."
    );
}

fn run(options: Options) -> Result<bool> {
    if !options.browser.exists() {
        bail!(
            "browser not found at {}; pass --browser <path>",
            options.browser.display()
        );
    }

    let timeout = Duration::from_millis(options.timeout_ms);
    let profile = Profile::new(options.keep_profile)?;

    let headless = !options.headed;
    let mut browser_args = vec![
        OsStr::new("--hide-scrollbars"),
        OsStr::new("--disable-component-update"),
        OsStr::new("--no-first-run"),
    ];
    if headless {
        browser_args.push(OsStr::new("--use-gl=swiftshader"));
        browser_args.push(OsStr::new("--enable-unsafe-swiftshader"));
    }

    let launch = LaunchOptions::default_builder()
        .path(Some(options.browser.clone()))
        .user_data_dir(Some(profile.path.clone()))
        .headless(headless)
        .enable_gpu(!headless)
        .ignore_certificate_errors(false)
        .idle_browser_timeout(Duration::from_secs(300))
        .args(browser_args)
        .build()
        .map_err(|error| anyhow!("invalid launch options: {error}"))?;

    let browser = Browser::new(launch).context("launch the browser")?;
    let browser_pid = browser.get_process_id();
    if let Some(pid) = browser_pid {
        install_signal_handlers(pid);
    }
    let tab = browser.new_tab().context("open a tab")?;
    let browser_version = browser
        .get_version()
        .map(|version| version.product)
        .unwrap_or_else(|_| "unknown".to_owned());

    tab.enable_log()
        .context("enable the Log domain")?
        .enable_runtime()
        .context("enable the Runtime domain")?;
    tab.call_method(Network::Enable {
        max_total_buffer_size: None,
        max_resource_buffer_size: None,
        max_post_data_size: None,
        report_direct_socket_traffic: None,
        enable_durable_messages: None,
    })
    .context("enable the Network domain")?;

    let collector = collect::shared(Instant::now());
    {
        let sink = Arc::clone(&collector);
        tab.add_event_listener(Arc::new(
            move |event: &headless_chrome::protocol::cdp::types::Event| {
                if let Ok(mut collector) = sink.lock() {
                    collector.handle(event);
                }
            },
        ))
        .context("subscribe to console and network events")?;
    }

    emulation::set_metrics(
        &tab,
        options.width,
        options.height,
        options.dpr,
        options.touch,
    )?;

    if options.touch {
        emulation::enable_touch(&tab)?;
    }

    if let Some(user_agent) = &options.user_agent {
        emulation::set_user_agent(&tab, user_agent)?;
    }

    if let Some(path) = &options.preload {
        let source = fs::read_to_string(path)
            .with_context(|| format!("read preload script {}", path.display()))?;
        tab.call_method(Page::AddScriptToEvaluateOnNewDocument {
            source,
            world_name: None,
            include_command_line_api: None,
            run_immediately: Some(true),
        })
        .context("install the preload script")?;
    }

    tab.navigate_to(&options.url)
        .with_context(|| format!("navigate to {}", options.url))?;
    tab.wait_until_navigated().context("wait for navigation")?;

    let readiness_started = Instant::now();
    let mut predicates = Vec::new();
    predicates.push(probe::wait_for_fonts(&tab, timeout));
    if let Some(window) = options.network_idle_ms {
        predicates.push(probe::wait_for_network_idle(
            &collector,
            Duration::from_millis(window),
            timeout,
        ));
    }
    if let Some(expression) = &options.ready {
        predicates.push(probe::wait_for_predicate(&tab, expression, timeout));
    }
    if let Some(frames) = options.frames {
        predicates.push(probe::wait_for_frames(
            &tab,
            &options.frame_counter,
            frames,
            timeout,
        ));
    }
    let readiness_waited = readiness_started.elapsed();
    let readiness_failed = predicates.iter().any(|predicate| !predicate.passed);

    let touch_points = if options.touch {
        probe::evaluate_text(&tab, "navigator.maxTouchPoints")
            .parse::<u32>()
            .unwrap_or(0)
    } else {
        0
    };

    let mut input_records = Vec::new();
    let mut pointer = Pointer::default();
    if !readiness_failed {
        for action in &options.actions {
            let record = input::dispatch(&tab, action, options.steps, &mut pointer, timeout)?;
            input_records.push(record);
        }
    }

    let expect =
        options.expect.as_ref().map(
            |expression| match probe::evaluate_labeled(&tab, expression) {
                Ok((value, label)) => Expect {
                    expression: expression.clone(),
                    value: label,
                    passed: probe::truthy(&value),
                },
                Err(error) => Expect {
                    expression: expression.clone(),
                    value: format!("error: {error}"),
                    passed: false,
                },
            },
        );

    let grab = |tab: &Tab| -> Result<(Captured, Option<Page::Viewport>)> {
        match &options.select {
            Some(selector) => {
                let (captured, viewport) = capture::element(tab, selector, timeout)?;
                Ok((captured, Some(viewport)))
            }
            None => Ok((capture::viewport(tab)?, None)),
        }
    };

    let (captured, clip, settle) = if options.no_settle {
        let started = Instant::now();
        let (captured, clip) = grab(&tab)?;
        (
            captured,
            clip,
            Settle {
                outcome: Outcome::Skipped,
                elapsed: started.elapsed(),
                samples: 1,
                identical: 0,
            },
        )
    } else {
        wait_for_stable(&tab, &grab, options.settle_ms, options.timeout_ms)?
    };

    let renderer_name = probe::renderer(&tab);
    let software = probe::is_software_renderer(&renderer_name);

    predicates.push(Predicate {
        name: "frame.stable".to_owned(),
        value: if settle.outcome == Outcome::Skipped {
            "skipped".to_owned()
        } else {
            format!(
                "{} samples, {} identical after {} ms",
                settle.samples,
                settle.identical,
                settle.elapsed.as_millis()
            )
        },
        passed: settle.outcome != Outcome::Unstable,
        waited_ms: settle.elapsed.as_millis() as u64,
    });

    let snapshot = collector
        .lock()
        .map(|collector| collector.snapshot())
        .unwrap_or_else(|_| Snapshot::default());

    let css = clip.map(|viewport| CssBox {
        x: round_to(viewport.x, 2),
        y: round_to(viewport.y, 2),
        width: round_to(viewport.width, 2),
        height: round_to(viewport.height, 2),
    });

    let mut warnings = Vec::new();
    for predicate in predicates.iter().filter(|predicate| !predicate.passed) {
        warnings.push(format!(
            "readiness failed: {} = {}",
            predicate.name, predicate.value
        ));
    }
    if readiness_failed && !options.actions.is_empty() {
        warnings.push("input skipped because readiness did not pass".to_owned());
    }

    let expected_width = css
        .as_ref()
        .map_or(f64::from(options.width), |css| css.width);
    let expected_height = css
        .as_ref()
        .map_or(f64::from(options.height), |css| css.height);
    let dpr_ok = dimensions_match(captured.width, expected_width * options.dpr, options.dpr)
        && dimensions_match(captured.height, expected_height * options.dpr, options.dpr);
    if !dpr_ok {
        warnings.push(format!(
            "DPR mismatch: asked {}x{} px for a CSS box of {}x{}, got {}x{}",
            (expected_width * options.dpr).round(),
            (expected_height * options.dpr).round(),
            expected_width,
            expected_height,
            captured.width,
            captured.height
        ));
    }

    if software {
        warnings.push(format!("renderer is software: {renderer_name}"));
    }
    if options.touch && touch_points == 0 {
        warnings.push("touch emulation did not take: navigator.maxTouchPoints = 0".to_owned());
    }
    if settle.outcome == Outcome::Unstable {
        warnings.push(format!(
            "no stable frame within {} ms; the image was taken anyway",
            settle.elapsed.as_millis()
        ));
    }
    if settle.outcome == Outcome::Skipped {
        warnings.push("settle skipped (--no-settle)".to_owned());
    }
    if snapshot.uncaught > 0 {
        warnings.push(format!("{} uncaught exception(s)", snapshot.uncaught));
    }
    if snapshot.console_errors > 0 {
        warnings.push(format!("{} console error(s)", snapshot.console_errors));
    }
    if snapshot.requests_failed > 0 {
        warnings.push(format!("{} request(s) failed", snapshot.requests_failed));
    }
    if let Some(expect) = expect.as_ref().filter(|expect| !expect.passed) {
        warnings.push(format!(
            "expectation failed: {} = {}",
            expect.expression, expect.value
        ));
    }
    if let Some(selector) = &options.select {
        if let Some(css) = css.as_ref() {
            if let Some((backing_width, backing_height)) =
                capture::canvas_backing(&tab, selector, timeout)
            {
                let expected_width = (css.width * options.dpr).round() as u32;
                let expected_height = (css.height * options.dpr).round() as u32;
                if backing_width < expected_width || backing_height < expected_height {
                    warnings.push(format!(
                        "canvas backing store {backing_width}x{backing_height} is smaller than {expected_width}x{expected_height}; the captured pixels are an upscale"
                    ));
                }
            }
        }
        if let Some((scroll_width, client_width, scroll_height, client_height)) =
            capture::overflow(&tab, selector, timeout)
        {
            if scroll_width > client_width + 1 || scroll_height > client_height + 1 {
                warnings.push(format!(
                    "{selector} overflows its box: scroll {scroll_width}x{scroll_height} > client {client_width}x{client_height}"
                ));
            }
        }
    }

    let passed = !readiness_failed
        && settle.outcome != Outcome::Unstable
        && dpr_ok
        && expect.as_ref().is_none_or(|expect| expect.passed)
        && snapshot.uncaught == 0
        && (!options.touch || touch_points > 0);

    let last_input = input_records.last();
    let report = Report {
        url: tab.get_url(),
        captured: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        peer: Peer { index: 0, of: 1 },
        browser: browser_version,
        pid: browser_pid,
        mode: if options.headed {
            "headed"
        } else {
            "headless (new)"
        }
        .to_owned(),
        renderer: Renderer {
            name: renderer_name,
            software,
        },
        viewport: Viewport {
            width: options.width,
            height: options.height,
            dpr: options.dpr,
            touch: options.touch,
            user_agent: options.user_agent.clone(),
        },
        image: Image {
            width: captured.width,
            height: captured.height,
            scope: options.select.as_ref().map_or_else(
                || "viewport".to_owned(),
                |selector| format!("element {selector}"),
            ),
            css,
        },
        ready: Ready {
            predicates,
            waited_ms: (readiness_waited + settle.elapsed).as_millis() as u64,
        },
        frame_rate: None,
        console: Console {
            errors: snapshot.console_errors,
            warnings: snapshot.console_warnings,
            uncaught: snapshot.uncaught,
            lines: snapshot.console_lines,
        },
        requests: Requests {
            total: snapshot.requests_total,
            failed: snapshot.requests_failed,
            lines: snapshot.request_lines,
        },
        network: "not emulated".to_owned(),
        overlay: "none".to_owned(),
        input: input_records
            .iter()
            .map(|record| record.description.clone())
            .collect(),
        focus: last_input.map_or_else(|| "none".to_owned(), |record| record.focus.clone()),
        scroll: last_input.map_or_else(|| "none".to_owned(), |record| record.scroll.clone()),
        locator: last_input.map_or_else(|| "none".to_owned(), |record| record.locator.clone()),
        expect,
        warnings,
        profile: format!(
            "{} ({})",
            profile.path.display(),
            if profile.keep {
                "kept"
            } else {
                "removed on exit"
            }
        ),
        screenshot: options.out.display().to_string(),
        sidecar: options.report.display().to_string(),
        passed,
    };

    if let Some(parent) = options.out.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&options.out, &captured.bytes)
        .with_context(|| format!("write {}", options.out.display()))?;
    if let Some(parent) = options.report.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&options.report, report.render_text())
        .with_context(|| format!("write {}", options.report.display()))?;

    if options.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "{} @ {}x{} ({}) · {}",
            options.out.display(),
            captured.width,
            captured.height,
            if report.passed { "pass" } else { "FAIL" },
            options.report.display()
        );
    }

    Ok(report.passed)
}

fn wait_for_stable<F>(
    tab: &Tab,
    mut grab: F,
    min_settle_ms: u64,
    timeout_ms: u64,
) -> Result<(Captured, Option<Page::Viewport>, Settle)>
where
    F: FnMut(&Tab) -> Result<(Captured, Option<Page::Viewport>)>,
{
    let started = Instant::now();
    let (first, _) = grab(tab)?;
    let mut stability = Stability::new(first.bytes.clone());
    let min_settle = Duration::from_millis(min_settle_ms);
    let timeout = Duration::from_millis(timeout_ms);

    loop {
        std::thread::sleep(Duration::from_millis(SAMPLE_MS));
        let (current, current_clip) = grab(tab)?;
        let elapsed = started.elapsed();
        let decision = stability.observe(current.bytes.clone(), elapsed, min_settle, timeout);

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
        return Ok((current, current_clip, settle));
    }
}

fn dimensions_match(actual: u32, expected: f64, tolerance: f64) -> bool {
    (f64::from(actual) - expected).abs() <= tolerance.max(1.0)
}

fn round_to(value: f64, places: i32) -> f64 {
    let factor = 10f64.powi(places);
    (value * factor).round() / factor
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

    #[test]
    fn dpr_matching_tolerates_one_css_pixel() {
        assert!(dimensions_match(1170, 1170.0, 1.0));
        assert!(dimensions_match(1170, 1169.4, 1.0));
        assert!(!dimensions_match(1170, 1168.0, 1.0));
        assert!(dimensions_match(1170, 1169.9999, 3.0));
    }
}
