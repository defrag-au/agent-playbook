//! The scenario runner: N isolated browser processes, steps that name their peers, and the
//! network/loss/keyboard surface the two-peer spike needs.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use chrono::{SecondsFormat, Utc};
use headless_chrome::browser::tab::Tab;
use headless_chrome::protocol::cdp::{Network, Page};
use headless_chrome::{Browser, LaunchOptions};
use serde::Serialize;

use crate::capture;
use crate::collect::{self, Collector};
use crate::emulation::{self, NetworkSpec as EmulationNetwork};
use crate::input::{self, Action, Pointer};
use crate::keys;
use crate::probe;
use crate::report::{
    Console, CssBox, Expect, FrameRate, Image, Peer, Predicate, Ready, Renderer, Report, Requests,
    Viewport,
};
use crate::scenario::{
    parse_viewport, KeyState, LossHookSpec, PeerSpec, Scenario, Step, DEFAULT_SEED, MAX_PEERS,
};

const DEFAULT_BROWSER: &str = "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser";

#[cfg(unix)]
static PEER_PIDS: [AtomicU32; MAX_PEERS] = [const { AtomicU32::new(0) }; MAX_PEERS];
#[cfg(unix)]
static SIGNALS_INSTALLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn handle_peer_signal(_: libc::c_int) {
    for slot in &PEER_PIDS {
        let pid = slot.load(Ordering::SeqCst);
        if pid != 0 {
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }
    }
    unsafe { libc::_exit(130) };
}

#[cfg(unix)]
fn install_peer_signal(index: usize, pid: u32) {
    PEER_PIDS[index.min(PEER_PIDS.len() - 1)].store(pid, Ordering::SeqCst);
    if !SIGNALS_INSTALLED.swap(true, Ordering::SeqCst) {
        unsafe {
            libc::signal(
                libc::SIGINT,
                handle_peer_signal as *const () as libc::sighandler_t,
            );
            libc::signal(
                libc::SIGTERM,
                handle_peer_signal as *const () as libc::sighandler_t,
            );
        }
    }
}

#[cfg(not(unix))]
fn install_peer_signal(_index: usize, _pid: u32) {}

#[derive(Clone)]
enum WaitKind {
    Predicate(String),
    Frames(u32),
    NetworkIdle(u64),
}

#[derive(Clone)]
struct CaptureInfo {
    width: u32,
    height: u32,
    scope: String,
    css: Option<CssBox>,
    screenshot: PathBuf,
    sidecar: PathBuf,
}

#[derive(Clone, Serialize)]
struct AssertResult {
    peer: String,
    expression: String,
    value: String,
    passed: bool,
    message: Option<String>,
}

#[derive(Serialize)]
struct RunReport {
    scenario: String,
    artifacts: String,
    processes: usize,
    passed: bool,
    peers: Vec<Report>,
    checks: Vec<AssertResult>,
}

struct RunProfile {
    path: PathBuf,
}

impl Drop for RunProfile {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct PeerState {
    spec: PeerSpec,
    index: usize,
    of: usize,
    _browser: Browser,
    tab: Arc<Tab>,
    profile: RunProfile,
    collector: Arc<Mutex<Collector>>,
    version: String,
    pid: Option<u32>,
    pointer: Pointer,
    modifiers: u32,
    inputs: Vec<String>,
    focus: String,
    scroll: String,
    locator: String,
    predicates: Vec<Predicate>,
    ready_waited: Duration,
    frame_latency: f64,
    frame_rate: Option<FrameRate>,
    last_capture: Option<CaptureInfo>,
    expect: Option<Expect>,
    warns: Vec<String>,
    asserts: Vec<AssertResult>,
    network: String,
}

pub fn run(args: Vec<String>) -> Result<bool> {
    let mut scenario_path = None;
    let mut artifacts = None;
    let mut json = false;
    let mut browser = PathBuf::from(DEFAULT_BROWSER);

    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenario" => scenario_path = Some(PathBuf::from(next(&mut args, "--scenario")?)),
            "--artifacts" => artifacts = Some(PathBuf::from(next(&mut args, "--artifacts")?)),
            "--browser" => browser = PathBuf::from(next(&mut args, "--browser")?),
            "--json" => json = true,
            "-h" | "--help" => {
                println!(
                    "browser-shot run --scenario <file.dsl|file.toml> [--artifacts <dir>] [--json] [--browser <path>]"
                );
                std::process::exit(0);
            }
            other => bail!("unknown option {other}; try --help"),
        }
    }

    let scenario_path = scenario_path.ok_or_else(|| anyhow!("--scenario is required"))?;
    let scenario = Scenario::load(&scenario_path)?;
    let names = scenario
        .peer
        .iter()
        .enumerate()
        .map(|(index, peer)| (peer.name.clone(), index))
        .collect::<HashMap<_, _>>();

    let run_id = format!("browser-run-{}", std::process::id());
    let run_dir = std::env::current_dir()?.join(".tmp").join(run_id);
    fs::create_dir_all(&run_dir)?;
    let artifacts = artifacts
        .or(scenario.run.artifacts.clone())
        .unwrap_or_else(|| run_dir.join("artifacts"));
    fs::create_dir_all(&artifacts)?;

    let timeout = Duration::from_millis(scenario.run.timeout_ms.unwrap_or(30_000));
    let specs = scenario.peer.clone();
    let of = specs.len();
    let launched: Vec<Result<PeerState>> = std::thread::scope(|scope| {
        let handles = specs
            .iter()
            .enumerate()
            .map(|(index, spec)| {
                let spec = spec.clone();
                let browser = browser.clone();
                let run_dir = run_dir.clone();
                scope.spawn(move || PeerState::launch(spec, index, of, &run_dir, &browser, timeout))
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("peer launch thread"))
            .collect()
    });

    let mut peers = Vec::new();
    for peer in launched {
        peers.push(peer?);
    }

    let mut captures: HashMap<String, String> = HashMap::new();
    let mut checks = Vec::new();

    for step in &scenario.step {
        match step {
            Step::Wait {
                peer,
                peers: selected,
                predicate,
                frames,
                network_idle,
                timeout_ms,
            } => {
                let indices = selected_indices(peer, selected, &names, peers.len())?;
                let kinds = wait_kinds(predicate, frames, network_idle)?;
                let step_timeout = timeout_ms.map_or(timeout, Duration::from_millis);
                let tasks = indices
                    .iter()
                    .map(|&index| {
                        (
                            index,
                            peers[index].spec.frame_counter(),
                            Arc::clone(&peers[index].tab),
                            Arc::clone(&peers[index].collector),
                            kinds.clone(),
                            step_timeout,
                        )
                    })
                    .collect::<Vec<_>>();

                let results = std::thread::scope(|scope| {
                    let handles = tasks
                        .into_iter()
                        .map(|(index, counter, tab, collector, kinds, timeout)| {
                            scope.spawn(move || {
                                let mut predicates = Vec::new();
                                for kind in kinds {
                                    predicates.push(match kind {
                                        WaitKind::Predicate(expression) => {
                                            probe::wait_for_predicate(&tab, &expression, timeout)
                                        }
                                        WaitKind::Frames(frames) => {
                                            probe::wait_for_frames(&tab, &counter, frames, timeout)
                                        }
                                        WaitKind::NetworkIdle(window) => {
                                            probe::wait_for_network_idle(
                                                &collector,
                                                Duration::from_millis(window),
                                                timeout,
                                            )
                                        }
                                    });
                                }
                                (index, predicates)
                            })
                        })
                        .collect::<Vec<_>>();
                    handles
                        .into_iter()
                        .map(|handle| handle.join().expect("wait thread"))
                        .collect::<Vec<_>>()
                });

                for (index, predicates) in results {
                    peers[index].add_predicates(predicates);
                }
            }
            Step::Eval {
                peer,
                expression,
                await_promise,
                capture,
            } => {
                let index = peer_index(peer, &names)?;
                let expression = substitute(expression, &captures);
                let object = peers[index]
                    .tab
                    .evaluate(&expression, *await_promise)
                    .with_context(|| format!("evaluate on {}", peer))?;
                let value = collect::remote_text(&object);
                if let Some(name) = capture {
                    captures.insert(name.clone(), value);
                }
            }
            Step::Key {
                peer,
                code,
                state,
                hold_frames,
            } => {
                let index = peer_index(peer, &names)?;
                let peer_state = &mut peers[index];
                let down = matches!(state, KeyState::Down);
                let counter = peer_state.spec.frame_counter();

                if let Some(hold) = hold_frames {
                    if !down {
                        bail!("hold_frames belongs on a key-down step");
                    }
                    // Read the counter *before* the keydown. Every CDP call costs one round trip
                    // on this transport (headless_chrome holds its websocket lock for up to
                    // 100 ms), so a hold measured from an event read after the dispatch would be
                    // late by two of those round trips. The read before the keydown and the
                    // response from the in-page wait cancel, leaving `target - origin` frames
                    // between the two events.
                    let origin = probe::number(&peer_state.tab, &counter).ok_or_else(|| {
                        anyhow!("frame counter {counter:?} is unavailable; cannot hold for frames")
                    })?;
                    let target = origin + f64::from(*hold);
                    keys::key(&peer_state.tab, code, true, &mut peer_state.modifiers)?;
                    probe::wait_until_frame(&peer_state.tab, &counter, target, timeout)?;
                    keys::key(&peer_state.tab, code, false, &mut peer_state.modifiers)?;

                    let down_event = keys::last_event(&peer_state.tab, code, true);
                    let up_event = keys::last_event(&peer_state.tab, code, false);
                    if let Some(event) = &down_event {
                        peer_state.record_key_event(code, true, event, None);
                    }
                    let held = match (&down_event, &up_event) {
                        (Some(down_event), Some(up_event)) => down_event
                            .frame
                            .zip(up_event.frame)
                            .map(|(down, up)| up - down),
                        _ => None,
                    };
                    if let Some(up_event) = &up_event {
                        let held_label = held.map(|held| format!("{held:.0} frames"));
                        peer_state.record_key_event(code, false, up_event, held_label);
                    }
                    if let Some(held) = held {
                        if (held - f64::from(*hold)).abs() > 0.5 {
                            peer_state.warns.push(format!(
                                "key hold requested {hold} frames, measured {held:.0}; a CDP round trip is ~{:.0} frames on this transport",
                                peer_state.frame_latency
                            ));
                        }
                    }
                } else {
                    keys::key(&peer_state.tab, code, down, &mut peer_state.modifiers)?;
                    let event = keys::last_event(&peer_state.tab, code, down);
                    if let Some(event) = &event {
                        peer_state.record_key_event(code, down, event, None);
                    }
                }
            }
            Step::Pointer {
                peer,
                kind,
                x,
                y,
                selector,
                steps,
            } => {
                let index = peer_index(peer, &names)?;
                let peer_state = &mut peers[index];
                let action = match (kind.as_str(), selector, x, y) {
                    ("click", Some(selector), _, _) => Action::ClickSelector(selector.clone()),
                    ("hover", Some(selector), _, _) => Action::HoverSelector(selector.clone()),
                    ("click", None, Some(x), Some(y)) => Action::Click { x: *x, y: *y },
                    ("hover", None, Some(x), Some(y)) => Action::Hover { x: *x, y: *y },
                    _ => bail!("pointer needs click|hover with x,y or a selector"),
                };
                let record = input::dispatch(
                    &peer_state.tab,
                    &action,
                    steps.unwrap_or(1),
                    &mut peer_state.pointer,
                    timeout,
                )?;
                peer_state.inputs.push(record.description);
                peer_state.focus = record.focus;
                peer_state.scroll = record.scroll;
                peer_state.locator = record.locator;
            }
            Step::Capture { peer, out, select } => {
                let index = peer_index(peer, &names)?;
                let peer_state = &mut peers[index];
                let info = peer_state.capture(&artifacts, out, select.as_deref(), timeout)?;
                peer_state.last_capture = Some(info);
            }
            Step::Assert {
                peer,
                expression,
                message,
            } => {
                let index = peer_index(peer, &names)?;
                let peer_state = &mut peers[index];
                let expression = substitute(expression, &captures);
                let (value, passed) = match probe::evaluate_labeled(&peer_state.tab, &expression) {
                    Ok((value, label)) => (label, probe::truthy(&value)),
                    Err(error) => (format!("error: {error}"), false),
                };
                let result = AssertResult {
                    peer: peer.clone(),
                    expression: expression.clone(),
                    value: value.clone(),
                    passed,
                    message: message.clone(),
                };
                if !passed {
                    peer_state
                        .warns
                        .push(format!("assertion failed: {expression} = {value}"));
                }
                peer_state.expect = Some(Expect {
                    expression,
                    value,
                    passed,
                });
                peer_state.asserts.push(result.clone());
                checks.push(result);
            }
            Step::Network {
                peer,
                latency_ms,
                download_kbps,
                upload_kbps,
                loss_percent,
            } => {
                let index = peer_index(peer, &names)?;
                let peer_state = &mut peers[index];
                if latency_ms.is_none()
                    && download_kbps.is_none()
                    && upload_kbps.is_none()
                    && loss_percent.is_none()
                {
                    bail!(
                        "network needs at least one of latency_ms, download_kbps, upload_kbps or loss_percent"
                    );
                }
                let spec = EmulationNetwork {
                    latency_ms: *latency_ms,
                    download_kbps: *download_kbps,
                    upload_kbps: *upload_kbps,
                    loss_percent: *loss_percent,
                    jitter_ms: None,
                };
                emulation::emulate_network(&peer_state.tab, &spec)?;
                peer_state.network = spec.label();
            }
        }
    }

    for peer in &mut peers {
        if peer.last_capture.is_none() {
            let info = peer.capture(
                &artifacts,
                &format!("{}.png", peer.spec.name),
                None,
                timeout,
            )?;
            peer.last_capture = Some(info);
        }
    }

    let reports = peers
        .iter()
        .map(|peer| {
            let passed = peer.passed();
            let info = peer.last_capture.as_ref().expect("a final capture");
            peer.report(info, passed)
        })
        .collect::<Vec<_>>();
    let passed = peers.iter().all(PeerState::passed);
    let processes = peers.len();
    drop(peers);
    let run_report = RunReport {
        scenario: scenario_path.display().to_string(),
        artifacts: artifacts.display().to_string(),
        processes,
        passed,
        peers: reports,
        checks,
    };

    fs::write(
        artifacts.join("run.json"),
        serde_json::to_string_pretty(&run_report)?,
    )?;
    fs::write(artifacts.join("run.txt"), render_run_text(&run_report))?;
    // The profile directories are removed by their guards when the peers are dropped; the
    // directory that held them is not. Leave it alone when it holds the default artifact tree,
    // and remove it when it is empty.
    let _ = fs::remove_dir(&run_dir);

    if json {
        println!("{}", serde_json::to_string_pretty(&run_report)?);
    } else {
        println!("{}", render_run_text(&run_report));
    }

    Ok(passed)
}

impl PeerState {
    fn launch(
        spec: PeerSpec,
        index: usize,
        of: usize,
        run_dir: &Path,
        browser_path: &Path,
        timeout: Duration,
    ) -> Result<Self> {
        let (width, height) = parse_viewport(&spec.viewport)?;
        let dpr = spec.dpr.unwrap_or(1.0);
        let headed = spec.headed.unwrap_or(false);
        let headless = !headed;
        let profile_path = run_dir.join(format!("peer-{index}"));
        fs::create_dir_all(&profile_path)?;
        let profile = RunProfile {
            path: profile_path.clone(),
        };

        let mut args = vec![
            std::ffi::OsStr::new("--hide-scrollbars"),
            std::ffi::OsStr::new("--disable-component-update"),
            std::ffi::OsStr::new("--no-first-run"),
        ];
        if headless {
            args.push(std::ffi::OsStr::new("--use-gl=swiftshader"));
            args.push(std::ffi::OsStr::new("--enable-unsafe-swiftshader"));
        }

        let launch = LaunchOptions::default_builder()
            .path(Some(
                spec.browser
                    .clone()
                    .unwrap_or_else(|| browser_path.to_path_buf()),
            ))
            .user_data_dir(Some(profile_path))
            .headless(headless)
            .enable_gpu(!headless)
            .ignore_certificate_errors(false)
            .idle_browser_timeout(Duration::from_secs(300))
            .args(args)
            .build()
            .map_err(|error| anyhow!("invalid launch options for {}: {error}", spec.name))?;

        let browser = Browser::new(launch)
            .with_context(|| format!("launch the browser for {}", spec.name))?;
        let pid = browser.get_process_id();
        if let Some(pid) = pid {
            install_peer_signal(index, pid);
        }
        let tab = browser
            .new_tab()
            .with_context(|| format!("open a tab for {}", spec.name))?;
        let version = browser
            .get_version()
            .map(|version| version.product)
            .unwrap_or_else(|_| "unknown".to_owned());

        tab.enable_log()?.enable_runtime()?;
        tab.call_method(Network::Enable {
            max_total_buffer_size: None,
            max_resource_buffer_size: None,
            max_post_data_size: None,
            report_direct_socket_traffic: None,
            enable_durable_messages: None,
        })?;

        let collector = collect::shared(Instant::now());
        {
            let sink = Arc::clone(&collector);
            tab.add_event_listener(Arc::new(
                move |event: &headless_chrome::protocol::cdp::types::Event| {
                    if let Ok(mut collector) = sink.lock() {
                        collector.handle(event);
                    }
                },
            ))?;
        }

        emulation::set_metrics(&tab, width, height, dpr, spec.touch.unwrap_or(false))?;
        if spec.touch.unwrap_or(false) {
            emulation::enable_touch(&tab)?;
        }
        if let Some(user_agent) = &spec.user_agent {
            emulation::set_user_agent(&tab, user_agent)?;
        }

        let network = if let Some(spec) = &spec.network {
            let emulated = EmulationNetwork {
                latency_ms: spec.latency_ms,
                download_kbps: spec.download_kbps,
                upload_kbps: spec.upload_kbps,
                loss_percent: spec.loss_percent,
                jitter_ms: spec.jitter_ms,
            };
            emulation::emulate_network(&tab, &emulated)?;
            emulated.label()
        } else {
            "not emulated".to_owned()
        };

        let mut source = key_log_script(&spec.frame_counter());
        if let Some(path) = &spec.preload {
            source.push_str(
                &fs::read_to_string(path)
                    .with_context(|| format!("read preload script {}", path.display()))?,
            );
            source.push('\n');
        }
        if let Some(hook) = &spec.loss_hook {
            source.push_str(&loss_hook_script(hook));
        }
        if !source.is_empty() {
            tab.call_method(Page::AddScriptToEvaluateOnNewDocument {
                source,
                world_name: None,
                include_command_line_api: None,
                run_immediately: Some(true),
            })?;
        }

        tab.navigate_to(&spec.url)
            .with_context(|| format!("navigate {} to {}", spec.name, spec.url))?;
        tab.wait_until_navigated()
            .with_context(|| format!("wait for {} to navigate", spec.name))?;

        let fonts = probe::wait_for_fonts(&tab, timeout);
        let ready_waited = Duration::from_millis(fonts.waited_ms);
        let frame_latency = probe::eval_latency_frames(&tab, &spec.frame_counter()).unwrap_or(0.0);
        let frame_rate = probe::measure_frame_rate(&tab, &spec.frame_counter());
        let mut warns = Vec::new();
        if !fonts.passed {
            warns.push(format!(
                "readiness failed: {} = {}",
                fonts.name, fonts.value
            ));
        }

        Ok(Self {
            spec,
            index,
            of,
            _browser: browser,
            tab,
            profile,
            collector,
            version,
            pid,
            pointer: Pointer::default(),
            modifiers: 0,
            inputs: Vec::new(),
            focus: "none".to_owned(),
            scroll: "none".to_owned(),
            locator: "none".to_owned(),
            predicates: vec![fonts],
            ready_waited,
            frame_latency,
            frame_rate,
            last_capture: None,
            expect: None,
            warns,
            asserts: Vec::new(),
            network,
        })
    }

    fn record_key_event(
        &mut self,
        code: &str,
        down: bool,
        event: &keys::KeyEvent,
        held: Option<String>,
    ) {
        let frame = event
            .frame
            .map_or_else(|| "unavailable".to_owned(), |frame| format!("{frame:.0}"));
        let held = held.map_or_else(String::new, |held| format!(" (held {held})"));
        self.inputs.push(format!(
            "{} {} @ frame {frame}{held} · focus {} · scroll {}",
            code,
            if down { "down" } else { "up" },
            event.focus,
            event.scroll
        ));
        self.focus = event.focus.clone();
        self.scroll = event.scroll.clone();
        if event.focus.eq_ignore_ascii_case("body") {
            self.warns.push(format!(
                "{} {} went to body; focus a canvas before sending keys",
                code,
                if down { "down" } else { "up" }
            ));
        }
    }

    fn add_predicates(&mut self, predicates: Vec<Predicate>) {
        self.ready_waited += predicates
            .iter()
            .map(|predicate| Duration::from_millis(predicate.waited_ms))
            .sum::<Duration>();
        for predicate in &predicates {
            if !predicate.passed {
                self.warns.push(format!(
                    "readiness failed: {} = {}",
                    predicate.name, predicate.value
                ));
            }
        }
        self.predicates.extend(predicates);
    }

    fn passed(&self) -> bool {
        self.predicates.iter().all(|predicate| predicate.passed)
            && self.asserts.iter().all(|assert| assert.passed)
            && self
                .collector
                .lock()
                .map(|collector| collector.snapshot().uncaught == 0)
                .unwrap_or(false)
    }

    fn capture(
        &self,
        artifacts: &Path,
        out: &str,
        select: Option<&str>,
        timeout: Duration,
    ) -> Result<CaptureInfo> {
        let (captured, css, scope) = if let Some(selector) = select {
            let (captured, viewport) = capture::element(&self.tab, selector, timeout)?;
            let css = CssBox {
                x: round_to(viewport.x, 2),
                y: round_to(viewport.y, 2),
                width: round_to(viewport.width, 2),
                height: round_to(viewport.height, 2),
            };
            (captured, Some(css), format!("element {selector}"))
        } else {
            (capture::viewport(&self.tab)?, None, "viewport".to_owned())
        };

        let screenshot = resolve_path(artifacts, out);
        if let Some(parent) = screenshot.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&screenshot, &captured.bytes)
            .with_context(|| format!("write {}", screenshot.display()))?;
        let sidecar = screenshot.with_extension("txt");

        let info = CaptureInfo {
            width: captured.width,
            height: captured.height,
            scope,
            css,
            screenshot,
            sidecar,
        };
        let passed = self.passed();
        fs::write(&info.sidecar, self.report(&info, passed).render_text())
            .with_context(|| format!("write {}", info.sidecar.display()))?;
        Ok(info)
    }

    fn report(&self, capture: &CaptureInfo, passed: bool) -> Report {
        let snapshot = self
            .collector
            .lock()
            .map(|collector| collector.snapshot())
            .unwrap_or_default();
        let renderer = probe::renderer(&self.tab);
        let software = probe::is_software_renderer(&renderer);
        let (width, height) = parse_viewport(&self.spec.viewport).unwrap_or((0, 0));
        let overlay = match (&self.spec.loss_hook, loss_snapshot(&self.tab)) {
            (Some(hook), Some((rate, 0, 0))) => format!(
                "drop {:.2}% hook installed in send() (seed {}, no sends observed)",
                rate * 100.0,
                hook.seed.unwrap_or(DEFAULT_SEED)
            ),
            (Some(hook), Some((rate, sent, dropped))) => format!(
                "drop {:.2}% injected in send() (seed {}, {dropped}/{sent} dropped)",
                rate * 100.0,
                hook.seed.unwrap_or(DEFAULT_SEED)
            ),
            (Some(hook), None) => format!(
                "drop {:.2}% requested for send() (hook not observed)",
                hook.rate * 100.0
            ),
            (None, _) => "none".to_owned(),
        };

        let mut warnings = self.warns.clone();
        if software {
            warnings.push(format!("renderer is software: {renderer}"));
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
        if let Some(expect) = self.expect.as_ref().filter(|expect| !expect.passed) {
            warnings.push(format!(
                "expectation failed: {} = {}",
                expect.expression, expect.value
            ));
        }

        Report {
            url: self.tab.get_url(),
            captured: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            peer: Peer {
                index: self.index as u32,
                of: self.of as u32,
            },
            browser: self.version.clone(),
            pid: self.pid,
            mode: if self.spec.headed.unwrap_or(false) {
                "headed"
            } else {
                "headless (new)"
            }
            .to_owned(),
            renderer: Renderer {
                name: renderer,
                software,
            },
            viewport: Viewport {
                width,
                height,
                dpr: self.spec.dpr.unwrap_or(1.0),
                touch: self.spec.touch.unwrap_or(false),
                user_agent: self.spec.user_agent.clone(),
            },
            image: Image {
                width: capture.width,
                height: capture.height,
                scope: capture.scope.clone(),
                css: capture.css.clone(),
            },
            ready: Ready {
                predicates: self.predicates.clone(),
                waited_ms: self.ready_waited.as_millis() as u64,
            },
            frame_rate: self.frame_rate.clone(),
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
            network: self.network.clone(),
            overlay,
            input: self.inputs.clone(),
            focus: self.focus.clone(),
            scroll: self.scroll.clone(),
            locator: self.locator.clone(),
            expect: self.expect.clone(),
            warnings,
            profile: format!("{} (removed on exit)", self.profile.path.display()),
            screenshot: capture.screenshot.display().to_string(),
            sidecar: capture.sidecar.display().to_string(),
            passed,
        }
    }
}

fn key_log_script(counter: &str) -> String {
    format!(
        r#"(() => {{
  const counter = () => {{ try {{ return Number({counter}); }} catch (error) {{ return null; }} }};
  const log = [];
  window.__inputLog = log;
  const record = (type) => (event) => {{
    const active = document.activeElement;
    const entry = {{
      type,
      code: event.code,
      key: event.key,
      frame: counter(),
      focus: active ? (active.id || active.tagName) : 'none',
      scrollY: window.scrollY,
      afterScrollY: null,
    }};
    log.push(entry);
    requestAnimationFrame(() => {{ entry.afterScrollY = window.scrollY; }});
  }};
  window.addEventListener('keydown', record('keydown'), true);
  window.addEventListener('keyup', record('keyup'), true);
}})();"#
    )
}

fn loss_hook_script(spec: &LossHookSpec) -> String {
    let rate = spec.rate;
    let seed = spec.seed.unwrap_or(DEFAULT_SEED);
    format!(
        r#"(() => {{
  if (typeof RTCDataChannel === 'undefined') return;
  const rate = {rate};
  let state = {seed} >>> 0;
  const next = () => {{
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    return state / 4294967296;
  }};
  const original = RTCDataChannel.prototype.send;
  const stats = {{ rate, seed: {seed}, sent: 0, dropped: 0 }};
  window.__lossHook = stats;
  RTCDataChannel.prototype.send = function (...args) {{
    stats.sent += 1;
    if (next() < stats.rate) {{
      stats.dropped += 1;
      return;
    }}
    return original.apply(this, args);
  }};
}})();"#
    )
}

fn loss_snapshot(tab: &Tab) -> Option<(f64, u64, u64)> {
    let rate = probe::number(tab, "window.__lossHook ? window.__lossHook.rate : -1")?;
    let sent = probe::number(tab, "window.__lossHook ? window.__lossHook.sent : -1")?;
    let dropped = probe::number(tab, "window.__lossHook ? window.__lossHook.dropped : -1")?;
    if rate < 0.0 || sent < 0.0 {
        return None;
    }
    Some((rate, sent as u64, dropped as u64))
}

fn wait_kinds(
    predicate: &Option<String>,
    frames: &Option<u32>,
    network_idle: &Option<u64>,
) -> Result<Vec<WaitKind>> {
    let mut kinds = Vec::new();
    if let Some(expression) = predicate {
        kinds.push(WaitKind::Predicate(expression.clone()));
    }
    if let Some(frames) = frames {
        kinds.push(WaitKind::Frames(*frames));
    }
    if let Some(window) = network_idle {
        kinds.push(WaitKind::NetworkIdle(*window));
    }
    if kinds.is_empty() {
        bail!("wait needs predicate, frames or network_idle");
    }
    Ok(kinds)
}

fn selected_indices(
    peer: &Option<String>,
    selected: &Option<Vec<String>>,
    names: &HashMap<String, usize>,
    peers: usize,
) -> Result<Vec<usize>> {
    if let Some(name) = peer {
        return Ok(vec![peer_index(name, names)?]);
    }
    if let Some(selected) = selected {
        if selected.iter().any(|name| name == "all") {
            return Ok((0..peers).collect());
        }
        return selected
            .iter()
            .map(|name| peer_index(name, names))
            .collect();
    }
    Ok((0..peers).collect())
}

fn peer_index(name: &str, names: &HashMap<String, usize>) -> Result<usize> {
    names
        .get(name)
        .copied()
        .ok_or_else(|| anyhow!("unknown peer {name:?}"))
}

fn substitute(expression: &str, captures: &HashMap<String, String>) -> String {
    let mut result = expression.to_owned();
    for (name, value) in captures {
        let literal = serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned());
        result = result.replace(&format!("${{{name}}}"), &literal);
    }
    result
}

fn resolve_path(artifacts: &Path, out: &str) -> PathBuf {
    let path = PathBuf::from(out);
    if path.is_absolute() {
        path
    } else {
        artifacts.join(path)
    }
}

fn round_to(value: f64, places: i32) -> f64 {
    let factor = 10f64.powi(places);
    (value * factor).round() / factor
}

fn next(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String> {
    args.next().ok_or_else(|| anyhow!("{flag} needs a value"))
}

fn render_run_text(report: &RunReport) -> String {
    let mut out = String::new();
    use std::fmt::Write as _;
    let _ = writeln!(out, "scenario  {}", report.scenario);
    let _ = writeln!(out, "artifacts {}", report.artifacts);
    let _ = writeln!(out, "processes {}", report.processes);
    for peer in &report.peers {
        let rate = peer
            .frame_rate
            .as_ref()
            .map_or_else(|| "rate ?".to_owned(), |rate| format!("{:.1} Hz", rate.fps));
        let _ = writeln!(
            out,
            "peer {}  {}  {}  {}",
            peer.peer.index,
            if peer.passed { "pass" } else { "FAIL" },
            rate,
            peer.screenshot
        );
    }
    for check in &report.checks {
        let _ = writeln!(
            out,
            "check {}  {}  {} = {}",
            if check.passed { "pass" } else { "FAIL" },
            check.peer,
            check.expression,
            check.value
        );
    }
    let _ = writeln!(
        out,
        "result   {}",
        if report.passed { "pass" } else { "FAIL" }
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitutes_captures_as_json_strings() {
        let mut captures = HashMap::new();
        captures.insert("offer".to_owned(), "v=0\r\n".to_owned());
        assert_eq!(
            substitute("accept(${offer})", &captures),
            "accept(\"v=0\\r\\n\")"
        );
    }

    #[test]
    fn loss_hook_script_carries_rate_and_seed() {
        let script = loss_hook_script(&LossHookSpec {
            rate: 0.02,
            seed: Some(7),
        });
        assert!(script.contains("const rate = 0.02;"));
        assert!(script.contains("let state = 7 >>> 0;"));
        assert!(script.contains("RTCDataChannel.prototype.send"));
        assert!(
            script.contains("next() < stats.rate"),
            "the rate must be read at send time so a scenario can change it"
        );
    }

    #[test]
    fn network_labels_say_what_they_do_not_cover() {
        let spec = EmulationNetwork {
            latency_ms: Some(80),
            loss_percent: Some(2.0),
            ..EmulationNetwork::default()
        };
        assert_eq!(
            spec.label(),
            "latency 80 ms, loss 2% — HTTP/WebSocket only, NOT WebRTC"
        );
        assert_eq!(EmulationNetwork::default().label(), "not emulated");
    }
}
