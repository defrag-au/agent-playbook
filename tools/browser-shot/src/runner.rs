//! The scenario runner: N isolated browser processes, steps that name their peers, and the
//! network/loss/keyboard surface the two-peer spike needs.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
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
    BurstInfo, Console, CssBox, Expect, FrameRate, Image, Peer, Predicate, Ready, Renderer, Report,
    Requests, Viewport,
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

struct ScreencastFrame {
    data: String,
    session_id: u32,
}

struct BurstFrame {
    path: PathBuf,
    width: u32,
    height: u32,
    start_ms: u64,
    interval_ms: u64,
    capture_ms: u64,
    write_ms: u64,
}

#[derive(Clone, Copy)]
struct BurstRequest<'a> {
    out: &'a str,
    frames: u32,
    interval_ms: Option<u64>,
    mode: &'a str,
    select: Option<&'a str>,
    timeout: Duration,
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
    bursts: Vec<BurstInfo>,
    focus: String,
    scroll: String,
    locator: String,
    predicates: Vec<Predicate>,
    user_waited: bool,
    warned_readiness: bool,
    ready_waited: Duration,
    frame_latency: f64,
    frame_rate: Option<FrameRate>,
    last_capture: Option<CaptureInfo>,
    expect: Option<Expect>,
    warns: Vec<String>,
    asserts: Vec<AssertResult>,
    network: String,
    capture_ok: bool,
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
                timeout_ms,
            } => {
                let index = peer_index(peer, &names)?;
                let expression = substitute(expression, &captures);
                let value = if *await_promise {
                    let limit = timeout_ms.map_or(timeout, Duration::from_millis);
                    probe::evaluate_settled(&peers[index].tab, &expression, limit)
                        .with_context(|| format!("evaluate on {peer}"))?
                        .1
                } else {
                    let object = peers[index]
                        .tab
                        .evaluate(&expression, false)
                        .with_context(|| format!("evaluate on {peer}"))?;
                    if let Some(error) = probe::error_of(&object) {
                        bail!("evaluate on {peer}: {error}");
                    }
                    collect::remote_text(&object)
                };
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
                button,
                from_x,
                from_y,
            } => {
                let index = peer_index(peer, &names)?;
                let peer_state = &mut peers[index];
                let kind = input::Kind::parse(kind).ok_or_else(|| {
                    anyhow!("pointer kind {kind:?} is not one of click, hover, drag, tap or swipe")
                })?;
                let target = match selector {
                    Some(selector) => input::Target::Selector(selector.clone()),
                    None => match (x, y) {
                        (Some(x), Some(y)) => input::Target::Coordinates { x: *x, y: *y },
                        _ => {
                            bail!("pointer needs click|hover|drag|tap|swipe with x,y or a selector")
                        }
                    },
                };
                let button = button
                    .as_deref()
                    .and_then(input::Button::parse)
                    .unwrap_or(input::Button::Left);
                let from = from_x.zip(*from_y);
                let action = match kind {
                    input::Kind::Click => Action::click(target, button),
                    input::Kind::Hover => Action::hover(target),
                    input::Kind::Drag => Action::drag(target, from, button),
                    input::Kind::Tap => Action::tap(target),
                    input::Kind::Swipe => Action::swipe(target, from),
                };
                let modifiers = peer_state.modifiers;
                let record = input::dispatch(
                    &peer_state.tab,
                    &action,
                    steps.unwrap_or(1),
                    &mut peer_state.pointer,
                    modifiers,
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
            Step::Burst {
                peer,
                out,
                frames,
                interval_ms,
                mode,
                select,
            } => {
                let index = peer_index(peer, &names)?;
                let peer_state = &mut peers[index];
                let mode = mode.as_deref().unwrap_or("capture");
                let info = peer_state.burst(
                    &artifacts,
                    &BurstRequest {
                        out,
                        frames: *frames,
                        interval_ms: *interval_ms,
                        mode,
                        select: select.as_deref(),
                        timeout,
                    },
                )?;
                peer_state.bursts.push(info);
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
        if let Some(extra) = &spec.args {
            args.extend(extra.iter().map(std::ffi::OsStr::new));
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

        let mut source = input_log_script(&spec.frame_counter());
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
        let touch_ok = if spec.touch.unwrap_or(false) {
            probe::number(&tab, "navigator.maxTouchPoints").unwrap_or(0.0) > 0.0
        } else {
            true
        };
        let mut warns = Vec::new();
        if !fonts.passed {
            warns.push(format!(
                "readiness failed: {} = {}",
                fonts.name, fonts.value
            ));
        }
        if !touch_ok {
            warns.push("touch emulation did not take: navigator.maxTouchPoints = 0".to_owned());
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
            bursts: Vec::new(),
            focus: "none".to_owned(),
            scroll: "none".to_owned(),
            locator: "none".to_owned(),
            predicates: vec![fonts],
            user_waited: false,
            warned_readiness: false,
            ready_waited,
            frame_latency,
            frame_rate,
            last_capture: None,
            expect: None,
            warns,
            asserts: Vec::new(),
            network,
            capture_ok: touch_ok,
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
        if !predicates.is_empty() {
            self.user_waited = true;
        }
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
        self.capture_ok
            && self.predicates.iter().all(|predicate| predicate.passed)
            && self.asserts.iter().all(|assert| assert.passed)
            && self
                .collector
                .lock()
                .map(|collector| collector.snapshot().uncaught == 0)
                .unwrap_or(false)
    }

    fn capture(
        &mut self,
        artifacts: &Path,
        out: &str,
        select: Option<&str>,
        timeout: Duration,
    ) -> Result<CaptureInfo> {
        if !self.user_waited && !self.warned_readiness {
            self.warned_readiness = true;
            self.warns.push(
                "captured without a scenario wait (predicate, frames or network_idle); the page may still have been settling"
                    .to_owned(),
            );
        }
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

        self.audit_capture(select, &captured, css.as_ref(), timeout)?;

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

    fn audit_capture(
        &mut self,
        selector: Option<&str>,
        captured: &capture::Captured,
        css: Option<&CssBox>,
        timeout: Duration,
    ) -> Result<()> {
        let dpr = self.spec.dpr.unwrap_or(1.0);
        let (expected_width, expected_height) = match css {
            Some(css) => (css.width * dpr, css.height * dpr),
            None => {
                let (width, height) = parse_viewport(&self.spec.viewport)?;
                (f64::from(width) * dpr, f64::from(height) * dpr)
            }
        };
        if !capture::dimensions_match(captured.width, expected_width, dpr)
            || !capture::dimensions_match(captured.height, expected_height, dpr)
        {
            self.capture_ok = false;
            self.warns.push(format!(
                "DPR mismatch: asked {:.0}x{:.0} px, got {}x{}",
                expected_width.round(),
                expected_height.round(),
                captured.width,
                captured.height
            ));
        }

        if let (Some(selector), Some(css)) = (selector, css) {
            if let Some((backing_width, backing_height)) =
                capture::canvas_backing(&self.tab, selector, timeout)
            {
                let expected_width = (css.width * dpr).round() as u32;
                let expected_height = (css.height * dpr).round() as u32;
                if backing_width < expected_width || backing_height < expected_height {
                    self.warns.push(format!(
                        "canvas backing store {backing_width}x{backing_height} is smaller than {expected_width}x{expected_height}; the captured pixels are an upscale"
                    ));
                }
            }
            if let Some((scroll_width, client_width, scroll_height, client_height)) =
                capture::overflow(&self.tab, selector, timeout)
            {
                if scroll_width > client_width + 1 || scroll_height > client_height + 1 {
                    self.warns.push(format!(
                        "{selector} overflows its box: scroll {scroll_width}x{scroll_height} > client {client_width}x{client_height}"
                    ));
                }
            }
            if let Some((width, height, visible_width, visible_height)) =
                capture::clipped(&self.tab, selector, timeout)
            {
                if visible_width + 1.0 < width || visible_height + 1.0 < height {
                    self.warns.push(format!(
                        "{selector} is clipped by an ancestor: visible {visible_width:.0}x{visible_height:.0} of {width:.0}x{height:.0}"
                    ));
                }
            }
        }
        Ok(())
    }

    fn burst(&mut self, artifacts: &Path, request: &BurstRequest<'_>) -> Result<BurstInfo> {
        let BurstRequest {
            out,
            frames: requested_frames,
            interval_ms,
            mode,
            select,
            timeout,
        } = *request;
        if mode == "screencast" {
            return self.burst_screencast(artifacts, request);
        }
        let prefix = resolve_path(artifacts, out);
        if let Some(parent) = prefix.parent() {
            fs::create_dir_all(parent)?;
        }
        let summary = prefix.with_extension("txt");
        let started = Instant::now();
        let mut frames = Vec::new();
        let mut last_start = None;

        for index in 0..requested_frames {
            if index > 0 && started.elapsed() >= timeout {
                self.capture_ok = false;
                self.warns.push(format!(
                    "burst stopped after {} of {requested_frames} frames at the {} ms timeout",
                    frames.len(),
                    timeout.as_millis()
                ));
                break;
            }

            let frame_started = Instant::now();
            let (captured, css) = if let Some(selector) = select {
                let (captured, viewport) = capture::element(&self.tab, selector, timeout)?;
                let css = CssBox {
                    x: round_to(viewport.x, 2),
                    y: round_to(viewport.y, 2),
                    width: round_to(viewport.width, 2),
                    height: round_to(viewport.height, 2),
                };
                (captured, Some(css))
            } else {
                (capture::viewport(&self.tab)?, None)
            };
            let capture_ms = frame_started.elapsed().as_millis() as u64;
            if index == 0 {
                self.audit_capture(select, &captured, css.as_ref(), timeout)?;
            }

            let path = frame_path(&prefix, index);
            let write_started = Instant::now();
            fs::write(&path, &captured.bytes)
                .with_context(|| format!("write {}", path.display()))?;
            let write_ms = write_started.elapsed().as_millis() as u64;
            let start_ms = frame_started.duration_since(started).as_millis() as u64;
            let interval = last_start
                .map(|last: Instant| frame_started.duration_since(last).as_millis() as u64)
                .unwrap_or(0);
            last_start = Some(frame_started);

            frames.push(BurstFrame {
                path,
                width: captured.width,
                height: captured.height,
                start_ms,
                interval_ms: interval,
                capture_ms,
                write_ms,
            });

            if let Some(interval) = interval_ms {
                let elapsed = frame_started.elapsed();
                let target = Duration::from_millis(interval);
                if elapsed < target {
                    std::thread::sleep(target - elapsed);
                }
            }
        }

        finish_burst(
            "capture",
            prefix,
            summary,
            requested_frames,
            frames,
            started,
        )
    }

    fn burst_screencast(
        &mut self,
        artifacts: &Path,
        request: &BurstRequest<'_>,
    ) -> Result<BurstInfo> {
        let BurstRequest {
            out,
            frames: requested_frames,
            mode: _,
            select,
            timeout,
            ..
        } = *request;
        if select.is_some() {
            bail!("screencast captures the viewport; remove select");
        }
        let prefix = resolve_path(artifacts, out);
        if let Some(parent) = prefix.parent() {
            fs::create_dir_all(parent)?;
        }
        let summary = prefix.with_extension("txt");
        let (frames_tx, frames_rx) = std::sync::mpsc::channel::<ScreencastFrame>();
        let listener = Arc::new(
            move |event: &headless_chrome::protocol::cdp::types::Event| {
                if let headless_chrome::protocol::cdp::types::Event::PageScreencastFrame(event) =
                    event
                {
                    let _ = frames_tx.send(ScreencastFrame {
                        data: event.params.data.clone(),
                        session_id: event.params.session_id,
                    });
                }
            },
        );
        let weak_listener = self.tab.add_event_listener(listener)?;
        let (width, height) = parse_viewport(&self.spec.viewport)?;
        let dpr = self.spec.dpr.unwrap_or(1.0);
        let _ = self.tab.call_method(Page::Enable {
            enable_file_chooser_opened_event: None,
        });
        emulation::set_visible_size(
            &self.tab,
            (f64::from(width) * dpr).round() as u32,
            (f64::from(height) * dpr).round() as u32,
        )?;
        self.tab.call_method(Page::StartScreencast {
            format: Some(Page::StartScreencastFormatOption::Png),
            quality: None,
            max_width: Some((f64::from(width) * dpr).round() as u32),
            max_height: Some((f64::from(height) * dpr).round() as u32),
            every_nth_frame: Some(1),
        })?;

        let started = Instant::now();
        let mut frames = Vec::new();
        let mut last_start = None;
        while frames.len() < requested_frames as usize {
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                self.capture_ok = false;
                self.warns.push(format!(
                    "screencast stopped after {} of {requested_frames} frames at the {} ms timeout",
                    frames.len(),
                    timeout.as_millis()
                ));
                break;
            }
            match frames_rx.recv_timeout(remaining) {
                Ok(frame) => {
                    let received = Instant::now();
                    let start_ms = received.duration_since(started).as_millis() as u64;
                    let interval = last_start
                        .map(|last: Instant| received.duration_since(last).as_millis() as u64)
                        .unwrap_or(0);
                    last_start = Some(received);

                    let decode_started = Instant::now();
                    let bytes = BASE64
                        .decode(&frame.data)
                        .context("decode a screencast frame")?;
                    let (frame_width, frame_height) = capture::png_dimensions(&bytes)
                        .ok_or_else(|| anyhow!("screencast frame was not a PNG"))?;
                    let capture_ms = decode_started.elapsed().as_millis() as u64;
                    let path = frame_path(&prefix, frames.len() as u32);
                    let write_started = Instant::now();
                    fs::write(&path, &bytes)
                        .with_context(|| format!("write {}", path.display()))?;
                    let write_ms = write_started.elapsed().as_millis() as u64;
                    let _ = self.tab.call_method(Page::ScreencastFrameAck {
                        session_id: frame.session_id,
                    });

                    frames.push(BurstFrame {
                        path,
                        width: frame_width,
                        height: frame_height,
                        start_ms,
                        interval_ms: interval,
                        capture_ms,
                        write_ms,
                    });
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    self.capture_ok = false;
                    self.warns.push(format!(
                        "screencast stopped after {} of {requested_frames} frames at the {} ms timeout",
                        frames.len(),
                        timeout.as_millis()
                    ));
                    break;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    self.capture_ok = false;
                    self.warns.push(
                        "screencast listener disconnected before all frames arrived".to_owned(),
                    );
                    break;
                }
            }
        }

        let _ = self.tab.call_method(Page::StopScreencast(None));
        let _ = self.tab.remove_event_listener(&weak_listener);
        finish_burst(
            "screencast",
            prefix,
            summary,
            requested_frames,
            frames,
            started,
        )
    }

    fn report(&self, capture: &CaptureInfo, passed: bool) -> Report {
        let snapshot = self
            .collector
            .lock()
            .map(|collector| collector.snapshot())
            .unwrap_or_default();
        let renderer = probe::renderer(&self.tab);
        let software = probe::is_software_renderer(&renderer);
        let layout = probe::layout(&self.tab);
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
        if let Some(layout) = &layout {
            if layout.width != width || layout.height != height {
                warnings.push(format!(
                    "layout viewport is {}x{}, not requested {}x{}; a page without a viewport meta tag lays out at its own width",
                    layout.width, layout.height, width, height
                ));
            }
        }
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
            args: self.spec.args.clone().unwrap_or_default(),
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
            layout,
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
            bursts: self.bursts.clone(),
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

fn input_log_script(counter: &str) -> String {
    format!(
        r#"(() => {{
  const counter = () => {{ try {{ return Number({counter}); }} catch (error) {{ return null; }} }};
  const log = [];
  window.__inputLog = log;
  const keep = () => {{ if (log.length > 2000) log.shift(); }};
  const record = (type, detail = () => ({{}})) => (event) => {{
    const active = document.activeElement;
    const entry = {{
      type,
      ...detail(event),
      frame: counter(),
      focus: active ? (active.id || active.tagName) : 'none',
      scrollY: window.scrollY,
      afterScrollY: null,
    }};
    log.push(entry);
    keep();
    requestAnimationFrame(() => {{ entry.afterScrollY = window.scrollY; }});
  }};
  const key = (event) => ({{ code: event.code, key: event.key }});
  const pointer = (event) => ({{ x: event.clientX, y: event.clientY, button: event.button }});
  window.addEventListener('keydown', record('keydown', key), true);
  window.addEventListener('keyup', record('keyup', key), true);
  window.addEventListener('mousedown', record('mousedown', pointer), true);
  window.addEventListener('mouseup', record('mouseup', pointer), true);
  window.addEventListener('mousemove', record('mousemove', pointer), true);
  const touch = (event) => {{
    const point = event.changedTouches && event.changedTouches[0];
    return point ? {{ x: point.clientX, y: point.clientY }} : {{}};
  }};
  window.addEventListener('touchstart', record('touchstart', touch), {{ capture: true, passive: true }});
  window.addEventListener('touchmove', record('touchmove', touch), {{ capture: true, passive: true }});
  window.addEventListener('touchend', record('touchend', touch), {{ capture: true, passive: true }});
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

fn frame_path(prefix: &Path, index: u32) -> PathBuf {
    let parent = prefix.parent().unwrap_or_else(|| Path::new(""));
    let stem = prefix
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("frame");
    let extension = prefix
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("png");
    parent.join(format!("{stem}-{index:03}.{extension}"))
}

fn min(values: &[u64]) -> u64 {
    values.iter().copied().min().unwrap_or(0)
}

fn max(values: &[u64]) -> u64 {
    values.iter().copied().max().unwrap_or(0)
}

fn median(values: &[u64]) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted[sorted.len() / 2]
}

fn finish_burst(
    mode: &str,
    prefix: PathBuf,
    summary: PathBuf,
    requested_frames: u32,
    frames: Vec<BurstFrame>,
    started: Instant,
) -> Result<BurstInfo> {
    let count = frames.len();
    let width = frames.first().map_or(0, |frame| frame.width);
    let height = frames.first().map_or(0, |frame| frame.height);
    let intervals = frames
        .iter()
        .skip(1)
        .map(|frame| frame.interval_ms)
        .collect::<Vec<_>>();
    let captures = frames
        .iter()
        .map(|frame| frame.capture_ms)
        .collect::<Vec<_>>();
    let writes = frames
        .iter()
        .map(|frame| frame.write_ms)
        .collect::<Vec<_>>();
    let span_ms = match (frames.first(), frames.last()) {
        (Some(first), Some(last)) if count > 1 && last.start_ms > first.start_ms => {
            last.start_ms - first.start_ms
        }
        _ => started.elapsed().as_millis() as u64,
    };
    let fps = if count > 1 && span_ms > 0 {
        (count - 1) as f64 * 1000.0 / span_ms as f64
    } else {
        0.0
    };

    let info = BurstInfo {
        mode: mode.to_owned(),
        prefix: prefix.display().to_string(),
        summary: summary.display().to_string(),
        frames: count,
        requested_frames: requested_frames as usize,
        width,
        height,
        fps,
        min_interval_ms: min(&intervals),
        median_interval_ms: median(&intervals),
        max_interval_ms: max(&intervals),
        min_capture_ms: min(&captures),
        median_capture_ms: median(&captures),
        max_capture_ms: max(&captures),
        min_write_ms: min(&writes),
        median_write_ms: median(&writes),
        max_write_ms: max(&writes),
    };

    let mut text = String::new();
    use std::fmt::Write as _;
    let _ = writeln!(text, "mode      {mode}");
    let _ = writeln!(text, "prefix    {}", info.prefix);
    let _ = writeln!(
        text,
        "frames    {}/{} in {} ms",
        count,
        requested_frames,
        started.elapsed().as_millis()
    );
    let _ = writeln!(text, "size      {}x{}", width, height);
    let _ = writeln!(text, "rate      {fps:.2} fps (from frame starts)");
    let _ = writeln!(
        text,
        "interval  {} / {} / {} ms (min/median/max)",
        info.min_interval_ms, info.median_interval_ms, info.max_interval_ms
    );
    let _ = writeln!(
        text,
        "capture   {} / {} / {} ms (min/median/max)",
        info.min_capture_ms, info.median_capture_ms, info.max_capture_ms
    );
    let _ = writeln!(
        text,
        "write     {} / {} / {} ms (min/median/max)",
        info.min_write_ms, info.median_write_ms, info.max_write_ms
    );
    let _ = writeln!(
        text,
        "index start_ms interval_ms capture_ms write_ms size file"
    );
    for (index, frame) in frames.iter().enumerate() {
        let _ = writeln!(
            text,
            "{index:>5} {:>8} {:>11} {:>10} {:>8} {}x{} {}",
            frame.start_ms,
            frame.interval_ms,
            frame.capture_ms,
            frame.write_ms,
            frame.width,
            frame.height,
            frame.path.display()
        );
    }
    fs::write(&summary, text).with_context(|| format!("write {}", summary.display()))?;
    Ok(info)
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
        for burst in &peer.bursts {
            let _ = writeln!(out, "burst {}  {}", peer.peer.index, burst.summary_line());
        }
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
