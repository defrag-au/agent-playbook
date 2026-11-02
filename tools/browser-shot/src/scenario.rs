//! Scenario data model, the TOML compatibility format, and the line-oriented DSL.
//!
//! `Scenario::load` chooses by extension and validates once, so `run` and `check`
//! cannot disagree about what a valid scenario is.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::input;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

macro_rules! bail_at {
    ($token:expr, $message:expr $(,)?) => {
        return Err(anyhow!(
            "line {}:{}: {}",
            $token.line,
            $token.column,
            $message
        ))
    };
}

pub(crate) const DEFAULT_SEED: u32 = 0x9e37_79b9;
/// The signal handler keeps one slot per process. Keep this in step with the array in `runner.rs`.
///
/// Sixteen rather than eight because a mesh scenario needs one page per member
/// of the room's interest set: at twelve (`INTEREST_MAX` in island-sim) a peer
/// holds eleven links with twelve pages, and twelve with thirteen. Eight pages
/// could only ever hold seven, so the cap was the thing measured rather than
/// the design.
pub(crate) const MAX_PEERS: usize = 16;
/// Spawned process groups share the same fixed-size signal table.
pub(crate) const MAX_SPAWNS: usize = 8;

#[derive(Debug, Deserialize)]
pub(crate) struct Scenario {
    #[serde(default)]
    pub(crate) run: RunConfig,
    pub(crate) peer: Vec<PeerSpec>,
    #[serde(default)]
    pub(crate) step: Vec<Step>,
    #[serde(default)]
    pub(crate) peer_lines: Vec<usize>,
    #[serde(default)]
    pub(crate) step_lines: Vec<usize>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct RunConfig {
    pub(crate) artifacts: Option<PathBuf>,
    pub(crate) timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PeerSpec {
    pub(crate) name: String,
    pub(crate) url: String,
    pub(crate) viewport: String,
    pub(crate) dpr: Option<f64>,
    pub(crate) touch: Option<bool>,
    pub(crate) user_agent: Option<String>,
    pub(crate) headed: Option<bool>,
    pub(crate) browser: Option<PathBuf>,
    pub(crate) preload: Option<PathBuf>,
    pub(crate) frame_counter: Option<String>,
    #[serde(default)]
    pub(crate) args: Option<Vec<String>>,
    pub(crate) network: Option<NetworkSpec>,
    pub(crate) loss_hook: Option<LossHookSpec>,
}

impl PeerSpec {
    pub(crate) fn frame_counter(&self) -> String {
        self.frame_counter
            .clone()
            .unwrap_or_else(|| "window.__frames".to_owned())
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct NetworkSpec {
    pub(crate) latency_ms: Option<u64>,
    pub(crate) download_kbps: Option<u64>,
    pub(crate) upload_kbps: Option<u64>,
    pub(crate) loss_percent: Option<f64>,
    pub(crate) jitter_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct LossHookSpec {
    pub(crate) rate: f64,
    pub(crate) seed: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub(crate) enum Step {
    Wait {
        peer: Option<String>,
        peers: Option<Vec<String>>,
        predicate: Option<String>,
        frames: Option<u32>,
        rate: Option<f64>,
        network_idle: Option<u64>,
        /// CSS selector; with `state` (default `visible`) waits for that element state.
        selector: Option<String>,
        /// `present` | `visible` | `hidden` | `gone` | `enabled`. Requires `selector`.
        state: Option<String>,
        /// Substring of visible text (`innerText`). Combined with `selector` to limit scope.
        text: Option<String>,
        /// Exact match against `location.href`.
        url: Option<String>,
        /// Substring match against `location.href` (`url~=` in the DSL; `url_contains` in TOML).
        url_contains: Option<String>,
        /// No DOM mutations for this many milliseconds (optionally under `selector`).
        stable_ms: Option<u64>,
        timeout_ms: Option<u64>,
    },
    /// An explicit pause. Prefer a condition wait; use this only when nothing else applies,
    /// so the sleep is visible in the scenario and the report.
    Sleep { peer: String, duration_ms: u64 },
    Eval {
        peer: String,
        expression: String,
        #[serde(default)]
        await_promise: bool,
        capture: Option<String>,
        out: Option<String>,
        timeout_ms: Option<u64>,
    },
    Key {
        peer: String,
        code: String,
        state: KeyState,
        hold_frames: Option<u32>,
    },
    Pointer {
        peer: String,
        kind: String,
        x: Option<f64>,
        y: Option<f64>,
        selector: Option<String>,
        steps: Option<u32>,
        button: Option<String>,
        from_x: Option<f64>,
        from_y: Option<f64>,
    },
    Capture {
        peer: String,
        out: String,
        select: Option<String>,
    },
    Burst {
        peer: String,
        out: String,
        frames: u32,
        interval_ms: Option<u64>,
        mode: Option<String>,
        select: Option<String>,
    },
    Assert {
        peer: String,
        expression: String,
        message: Option<String>,
    },
    Network {
        peer: String,
        latency_ms: Option<u64>,
        download_kbps: Option<u64>,
        upload_kbps: Option<u64>,
        loss_percent: Option<f64>,
    },
    /// Start a process the scenario owns. The tool kills its whole process group
    /// when the run ends, however it ends.
    Spawn {
        name: String,
        command: String,
        /// The port this process is expected to listen on. Declared here so the
        /// tool can refuse to start on a port somebody else holds — a check that
        /// cannot move to `wait_for`, because by then a server that failed to
        /// bind has died and the squatter is what answers.
        port: Option<u16>,
        dir: Option<String>,
        /// `KEY=VALUE` entries added to the child's environment.
        env: Option<Vec<String>>,
    },
    /// Wait for something outside the browser. Exactly one variant per step.
    WaitFor {
        /// A spawned process whose declared port must accept a connection.
        ready: Option<String>,
        /// Any TCP accept on `host:port`, for a server the scenario does not own.
        port: Option<u16>,
        host: Option<String>,
        /// A GET that must answer `status` (default 200).
        http: Option<String>,
        status: Option<u16>,
        /// A spawned process whose log must contain `pattern`.
        log: Option<String>,
        pattern: Option<String>,
        /// A spawned process that must end, 0 unless `code` says otherwise.
        exit: Option<String>,
        code: Option<i32>,
        timeout_ms: Option<u64>,
    },
    /// Send a peer to a URL. Peers load their `url=` before any step runs, so a
    /// page served by a process this scenario started needs this: spawn, wait,
    /// then navigate.
    Goto { peer: String, url: String },
}

impl Step {
    pub(crate) fn action_name(&self) -> &'static str {
        match self {
            Self::Wait { .. } => "wait",
            Self::Sleep { .. } => "sleep",
            Self::Eval { .. } => "eval",
            Self::Key { .. } => "key",
            Self::Pointer { .. } => "pointer",
            Self::Capture { .. } => "capture",
            Self::Burst { .. } => "burst",
            Self::Assert { .. } => "assert",
            Self::Network { .. } => "network",
            Self::Spawn { .. } => "spawn",
            Self::WaitFor { .. } => "wait_for",
            Self::Goto { .. } => "goto",
        }
    }

    pub(crate) fn summary(&self) -> String {
        match self {
            Self::Wait {
                peer,
                peers,
                predicate,
                frames,
                rate,
                network_idle,
                selector,
                state,
                text,
                url,
                url_contains,
                stable_ms,
                ..
            } => {
                let who = peer
                    .clone()
                    .or_else(|| peers.as_ref().map(|peers| peers.join(",")))
                    .unwrap_or_default();
                let mut conditions = Vec::new();
                if let Some(predicate) = predicate {
                    conditions.push(format!("predicate {predicate:?}"));
                }
                if let Some(frames) = frames {
                    conditions.push(format!("frames {frames}"));
                }
                if let Some(rate) = rate {
                    conditions.push(format!("rate {rate}"));
                }
                if let Some(network_idle) = network_idle {
                    conditions.push(format!("network_idle {network_idle}"));
                }
                if let Some(selector) = selector {
                    let state = state.as_deref().unwrap_or("visible");
                    conditions.push(format!("selector {selector:?} {state}"));
                }
                if let Some(text) = text {
                    conditions.push(format!("text {text:?}"));
                }
                if let Some(url) = url {
                    conditions.push(format!("url {url:?}"));
                }
                if let Some(url_contains) = url_contains {
                    conditions.push(format!("url~ {url_contains:?}"));
                }
                if let Some(stable_ms) = stable_ms {
                    conditions.push(format!("stable {stable_ms}ms"));
                }
                format!("{who} {}", conditions.join(", "))
            }
            Self::Sleep { peer, duration_ms } => {
                format!("{peer} {duration_ms}ms")
            }
            Self::Eval {
                peer,
                expression,
                capture,
                out,
                ..
            } => {
                let capture = capture
                    .as_ref()
                    .map_or_else(String::new, |capture| format!(" -> {capture}"));
                let out = out
                    .as_ref()
                    .map_or_else(String::new, |out| format!(" => {out}"));
                format!("{peer} {expression}{capture}{out}")
            }
            Self::Key {
                peer,
                code,
                state,
                hold_frames,
            } => {
                let state = match state {
                    KeyState::Down => "down",
                    KeyState::Up => "up",
                };
                let hold = hold_frames.map_or_else(String::new, |hold| format!(" hold={hold}"));
                format!("{peer} {code} {state}{hold}")
            }
            Self::Pointer {
                peer,
                kind,
                x,
                y,
                selector,
                button,
                from_x,
                from_y,
                ..
            } => {
                let target = selector.clone().unwrap_or_else(|| match (x, y) {
                    (Some(x), Some(y)) => format!("{x},{y}"),
                    _ => "missing target".to_owned(),
                });
                let button = button
                    .as_ref()
                    .filter(|button| button.as_str() != "left")
                    .map_or_else(String::new, |button| format!(" button={button}"));
                let from = from_x
                    .zip(*from_y)
                    .map_or_else(String::new, |(x, y)| format!(" from={x},{y}"));
                format!("{peer} {kind} {target}{button}{from}")
            }
            Self::Capture { peer, out, .. } => format!("{peer} -> {out}"),
            Self::Burst {
                peer,
                out,
                frames,
                interval_ms,
                mode,
                select,
            } => {
                let interval = interval_ms.map_or_else(String::new, |ms| format!(" interval={ms}"));
                let mode = mode
                    .as_ref()
                    .map_or_else(String::new, |mode| format!(" mode={mode}"));
                let select = select
                    .as_ref()
                    .map_or_else(String::new, |select| format!(" select={select}"));
                format!("{peer} {out} frames={frames}{interval}{mode}{select}")
            }
            Self::Assert {
                peer, expression, ..
            } => format!("{peer} {expression}"),
            Self::Network { peer, .. } => peer.clone(),
            Self::Spawn {
                name,
                command,
                port,
                ..
            } => {
                let port = port.map_or_else(String::new, |port| format!(" port {port}"));
                format!("{name}{port} {command}")
            }
            Self::WaitFor {
                ready,
                port,
                host,
                http,
                status,
                log,
                pattern,
                exit,
                code,
                ..
            } => {
                if let Some(name) = ready {
                    format!("ready {name}")
                } else if let Some(port) = port {
                    let host = host.as_deref().unwrap_or("127.0.0.1");
                    format!("port {host}:{port}")
                } else if let Some(url) = http {
                    let status =
                        status.map_or_else(String::new, |status| format!(" status {status}"));
                    format!("http {url}{status}")
                } else if let Some(name) = log {
                    let pattern = pattern.as_deref().unwrap_or_default();
                    format!("log {name} for {pattern:?}")
                } else if let Some(name) = exit {
                    let code = code.map_or_else(String::new, |code| format!(" code {code}"));
                    format!("exit {name}{code}")
                } else {
                    "missing a variant".to_owned()
                }
            }
            Self::Goto { peer, url } => format!("{peer} {url}"),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum KeyState {
    Down,
    Up,
}

impl Scenario {
    /// Load TOML or the DSL, chosen by extension. Both `run` and `check` use this.
    pub(crate) fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let scenario = match extension.as_str() {
            "dsl" | "scenario" => {
                parse_dsl(&text).with_context(|| format!("parse {}", path.display()))?
            }
            "toml" => toml::from_str(&text).with_context(|| format!("parse {}", path.display()))?,
            _ => match toml::from_str(&text) {
                Ok(scenario) => scenario,
                Err(toml_error) => parse_dsl(&text).with_context(|| {
                    format!(
                        "parse {}: not TOML ({toml_error}) and not the scenario DSL",
                        path.display()
                    )
                })?,
            },
        };
        scenario
            .validate()
            .with_context(|| format!("validate {}", path.display()))?;
        Ok(scenario)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.peer.is_empty() {
            bail!("the scenario declares no peers");
        }
        if self.peer.len() > MAX_PEERS {
            bail!(
                "the scenario declares {} peers; at most {MAX_PEERS} can be tracked for signal cleanup",
                self.peer.len()
            );
        }
        if self.run.timeout_ms == Some(0) {
            bail!("run timeout must be greater than zero");
        }

        let mut names = HashSet::new();
        for (index, peer) in self.peer.iter().enumerate() {
            let line = self
                .peer_lines
                .get(index)
                .map_or_else(String::new, |line| format!("line {line}: "));
            if peer.name.trim().is_empty() {
                bail!("{line}a peer has an empty name");
            }
            if peer.name.contains(',') || peer.name.chars().any(char::is_whitespace) {
                bail!(
                    "{line}peer name {:?} must not contain spaces or commas",
                    peer.name
                );
            }
            if !names.insert(peer.name.as_str()) {
                bail!("{line}duplicate peer name {:?}", peer.name);
            }
            if peer.url.trim().is_empty() {
                bail!("peer {:?} has an empty url", peer.name);
            }
            parse_viewport(&peer.viewport).with_context(|| format!("peer {:?}", peer.name))?;
            if let Some(dpr) = peer.dpr {
                if !dpr.is_finite() || dpr <= 0.0 {
                    bail!(
                        "peer {:?}: dpr must be a finite number greater than zero",
                        peer.name
                    );
                }
            }
            if let Some(network) = &peer.network {
                validate_network(network, &format!("peer {:?}", peer.name))?;
            }
            if let Some(hook) = &peer.loss_hook {
                if !hook.rate.is_finite() || !(0.0..=1.0).contains(&hook.rate) {
                    bail!(
                        "peer {:?}: loss hook rate must be between 0 and 1",
                        peer.name
                    );
                }
            }
            if peer.frame_counter.as_deref() == Some("") {
                bail!("peer {:?}: frame_counter must not be empty", peer.name);
            }
            if let Some(args) = &peer.args {
                if args.is_empty() {
                    bail!("peer {:?}: args must not be empty", peer.name);
                }
                for arg in args {
                    if arg.trim().is_empty() {
                        bail!("peer {:?}: an arg must not be empty", peer.name);
                    }
                    if arg == "--user-data-dir"
                        || arg.starts_with("--user-data-dir=")
                        || arg == "--remote-debugging-port"
                        || arg.starts_with("--remote-debugging-port=")
                        || arg.starts_with("--headless")
                    {
                        bail!(
                            "peer {:?}: {arg:?} is set by browser-shot; an override would break the run's isolation or transport",
                            peer.name
                        );
                    }
                }
            }
        }

        // Spawn names, with the port each declared. Built in step order, so a
        // `wait_for` can only name a process started before it.
        let mut spawned: HashMap<&str, Option<u16>> = HashMap::new();

        for (index, step) in self.step.iter().enumerate() {
            let at = self.step_lines.get(index).map_or_else(
                || format!("step {} ({})", index + 1, step.action_name()),
                |line| format!("line {line}: step {} ({})", index + 1, step.action_name()),
            );
            match step {
                Step::Spawn {
                    name,
                    command,
                    port,
                    dir,
                    env,
                } => {
                    if name.trim().is_empty() {
                        bail!("{at}: a spawn has an empty name");
                    }
                    if name.contains(',') || name.chars().any(char::is_whitespace) {
                        bail!("{at}: spawn name {name:?} must not contain spaces or commas");
                    }
                    if names.contains(name.as_str()) {
                        bail!("{at}: spawn name {name:?} is also a peer name");
                    }
                    if spawned.len() >= MAX_SPAWNS {
                        bail!("{at}: at most {MAX_SPAWNS} processes can be spawned in one run");
                    }
                    if spawned.insert(name.as_str(), *port).is_some() {
                        bail!("{at}: duplicate spawn name {name:?}");
                    }
                    if command.trim().is_empty() {
                        bail!("{at}: spawn {name:?} needs a command");
                    }
                    if *port == Some(0) {
                        bail!("{at}: port must be greater than zero");
                    }
                    if let Some(dir) = dir {
                        if dir.trim().is_empty() {
                            bail!("{at}: dir must not be empty");
                        }
                    }
                    if let Some(env) = env {
                        if env.is_empty() {
                            bail!("{at}: env must not be empty");
                        }
                        for entry in env {
                            match entry.split_once('=') {
                                Some((key, _)) if !key.trim().is_empty() => {}
                                _ => bail!("{at}: env entry {entry:?} is not KEY=VALUE"),
                            }
                        }
                    }
                }
                Step::WaitFor {
                    ready,
                    port,
                    host,
                    http,
                    status,
                    log,
                    pattern,
                    exit,
                    code,
                    timeout_ms,
                } => {
                    let variants = [
                        ready.is_some(),
                        port.is_some(),
                        http.is_some(),
                        log.is_some(),
                        exit.is_some(),
                    ]
                    .into_iter()
                    .filter(|present| *present)
                    .count();
                    if variants != 1 {
                        bail!("{at}: needs exactly one of ready, port, http, log or exit");
                    }
                    if *timeout_ms == Some(0) {
                        bail!("{at}: timeout must be greater than zero");
                    }
                    if pattern.is_some() && log.is_none() {
                        bail!("{at}: pattern belongs to log");
                    }
                    if status.is_some() && http.is_none() {
                        bail!("{at}: status belongs to http");
                    }
                    if code.is_some() && exit.is_none() {
                        bail!("{at}: code belongs to exit");
                    }
                    if host.is_some() && port.is_none() {
                        bail!("{at}: host belongs to port");
                    }
                    if let Some(name) = ready {
                        match spawned.get(name.as_str()) {
                            None => bail!("{at}: no spawn named {name:?} before this step"),
                            Some(None) => bail!(
                                "{at}: spawn {name:?} declares no port, so use port= rather than ready="
                            ),
                            Some(Some(_)) => {}
                        }
                    }
                    for name in [log.as_deref(), exit.as_deref()].into_iter().flatten() {
                        if !spawned.contains_key(name) {
                            bail!("{at}: no spawn named {name:?} before this step");
                        }
                    }
                    if log.is_some() && pattern.as_deref().unwrap_or("").is_empty() {
                        bail!("{at}: log needs a pattern");
                    }
                    if let Some(url) = http {
                        if !url.starts_with("http://") {
                            bail!(
                                "{at}: http must be an http:// URL; https needs TLS this tool does not carry"
                            );
                        }
                    }
                    if *port == Some(0) {
                        bail!("{at}: port must be greater than zero");
                    }
                }
                Step::Goto { peer, url } => {
                    check_peer(&names, &at, peer)?;
                    if url.trim().is_empty() {
                        bail!("{at}: goto needs a url");
                    }
                }
                Step::Wait {
                    peer,
                    peers,
                    predicate,
                    frames,
                    rate,
                    network_idle,
                    selector,
                    state,
                    text,
                    url,
                    url_contains,
                    stable_ms,
                    timeout_ms,
                } => {
                    let selected = match (peer, peers) {
                        (Some(peer), None) => vec![peer.as_str()],
                        (None, Some(peers)) if !peers.is_empty() => {
                            peers.iter().map(String::as_str).collect()
                        }
                        _ => bail!("{at}: needs exactly one of peer or peers"),
                    };
                    for name in selected {
                        if !names.contains(name) {
                            bail!("{at}: unknown peer {name:?}");
                        }
                    }
                    if state.is_some() && selector.is_none() {
                        bail!("{at}: state= needs selector=");
                    }
                    if predicate.is_none()
                        && frames.is_none()
                        && rate.is_none()
                        && network_idle.is_none()
                        && selector.is_none()
                        && text.is_none()
                        && url.is_none()
                        && url_contains.is_none()
                        && stable_ms.is_none()
                    {
                        bail!(
                            "{at}: needs predicate, frames, rate, network_idle, selector, text, url, url~ or stable"
                        );
                    }
                    if let Some(state) = state {
                        if crate::probe::SelectorState::parse(state).is_none() {
                            bail!(
                                "{at}: state must be present, visible, hidden, gone or enabled, got {state:?}"
                            );
                        }
                    }
                    if let Some(selector) = selector {
                        if selector.trim().is_empty() {
                            bail!("{at}: selector must not be empty");
                        }
                    }
                    if let Some(text) = text {
                        if text.is_empty() {
                            bail!("{at}: text must not be empty");
                        }
                    }
                    if let Some(url) = url {
                        if url.is_empty() {
                            bail!("{at}: url must not be empty");
                        }
                    }
                    if let Some(url_contains) = url_contains {
                        if url_contains.is_empty() {
                            bail!("{at}: url~ must not be empty");
                        }
                    }
                    if url.is_some() && url_contains.is_some() {
                        bail!("{at}: url and url~ cannot be combined; pick one");
                    }
                    if let Some(rate) = rate {
                        if !rate.is_finite() || *rate <= 0.0 {
                            bail!("{at}: rate must be a positive number of frames per second");
                        }
                    }
                    if *stable_ms == Some(0) {
                        bail!("{at}: stable must be greater than zero");
                    }
                    if *timeout_ms == Some(0) {
                        bail!("{at}: timeout must be greater than zero");
                    }
                }
                Step::Sleep { peer, duration_ms } => {
                    check_peer(&names, &at, peer)?;
                    if *duration_ms == 0 {
                        bail!("{at}: sleep duration must be greater than zero");
                    }
                }
                Step::Eval {
                    peer,
                    expression,
                    await_promise,
                    out,
                    timeout_ms,
                    ..
                } => {
                    check_peer(&names, &at, peer)?;
                    if expression.trim().is_empty() {
                        bail!("{at}: expression must not be empty");
                    }
                    if out.as_deref() == Some("") {
                        bail!("{at}: out must not be empty");
                    }
                    if *timeout_ms == Some(0) {
                        bail!("{at}: timeout must be greater than zero");
                    }
                    if timeout_ms.is_some() && !await_promise {
                        bail!("{at}: timeout belongs on an awaited expression; add await");
                    }
                }
                Step::Key {
                    peer,
                    code,
                    state,
                    hold_frames,
                } => {
                    check_peer(&names, &at, peer)?;
                    if code.trim().is_empty() {
                        bail!("{at}: code must not be empty");
                    }
                    if hold_frames.is_some() && matches!(state, KeyState::Up) {
                        bail!("{at}: hold_frames belongs on a key-down step");
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
                    check_peer(&names, &at, peer)?;
                    let Some(kind) = input::Kind::parse(kind) else {
                        bail!("{at}: kind must be click, hover, drag, tap or swipe");
                    };
                    if let Some(button) = button {
                        if input::Button::parse(button).is_none() {
                            bail!("{at}: button must be left, right or middle");
                        }
                        if !matches!(kind, input::Kind::Click | input::Kind::Drag) {
                            bail!("{at}: button belongs on click or drag");
                        }
                    }
                    let has_from = from_x.is_some() || from_y.is_some();
                    if has_from && (!matches!(kind, input::Kind::Drag | input::Kind::Swipe)) {
                        bail!("{at}: from belongs on drag or swipe");
                    }
                    if has_from && (from_x.is_none() || from_y.is_none()) {
                        bail!("{at}: from needs both coordinates");
                    }
                    if let (Some(from_x), Some(from_y)) = (from_x, from_y) {
                        if !from_x.is_finite() || !from_y.is_finite() {
                            bail!("{at}: from coordinates must be finite");
                        }
                    }
                    if kind.is_touch() {
                        let touch = self
                            .peer
                            .iter()
                            .find(|spec| &spec.name == peer)
                            .is_some_and(|spec| spec.touch == Some(true));
                        if !touch {
                            bail!("{at}: {} needs touch on peer {peer:?}", kind.name());
                        }
                    }
                    let has_point = x.is_some() || y.is_some();
                    if has_point && (x.is_none() || y.is_none()) {
                        bail!("{at}: coordinates need both x and y");
                    }
                    if has_point && selector.is_some() {
                        bail!("{at}: use either coordinates or selector, not both");
                    }
                    if !has_point && selector.is_none() {
                        bail!("{at}: needs coordinates or a selector");
                    }
                    if let Some(selector) = selector {
                        if selector.trim().is_empty() {
                            bail!("{at}: selector must not be empty");
                        }
                    }
                    if let (Some(x), Some(y)) = (x, y) {
                        if !x.is_finite() || !y.is_finite() {
                            bail!("{at}: coordinates must be finite");
                        }
                    }
                    if *steps == Some(0) {
                        bail!("{at}: steps must be greater than zero");
                    }
                }
                Step::Capture { peer, out, select } => {
                    check_peer(&names, &at, peer)?;
                    if out.trim().is_empty() {
                        bail!("{at}: out must not be empty");
                    }
                    if select.as_deref() == Some("") {
                        bail!("{at}: select must not be empty");
                    }
                }
                Step::Burst {
                    peer,
                    out,
                    frames,
                    mode,
                    select,
                    ..
                } => {
                    check_peer(&names, &at, peer)?;
                    if out.trim().is_empty() {
                        bail!("{at}: out must not be empty");
                    }
                    if *frames == 0 {
                        bail!("{at}: frames must be greater than zero");
                    }
                    if let Some(mode) = mode {
                        if mode != "capture" && mode != "screencast" {
                            bail!("{at}: mode must be capture or screencast");
                        }
                        if mode == "screencast" && select.is_some() {
                            bail!("{at}: screencast captures the viewport; remove select");
                        }
                    }
                    if select.as_deref() == Some("") {
                        bail!("{at}: select must not be empty");
                    }
                }
                Step::Assert {
                    peer, expression, ..
                } => {
                    check_peer(&names, &at, peer)?;
                    if expression.trim().is_empty() {
                        bail!("{at}: expression must not be empty");
                    }
                }
                Step::Network {
                    peer,
                    latency_ms,
                    download_kbps,
                    upload_kbps,
                    loss_percent,
                } => {
                    check_peer(&names, &at, peer)?;
                    if latency_ms.is_none()
                        && download_kbps.is_none()
                        && upload_kbps.is_none()
                        && loss_percent.is_none()
                    {
                        bail!("{at}: needs a network field");
                    }
                    if let Some(loss) = loss_percent {
                        if !loss.is_finite() || !(0.0..=100.0).contains(loss) {
                            bail!("{at}: loss must be between 0 and 100");
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn check_report(&self, path: &Path) -> CheckReport {
        let spawns = self
            .step
            .iter()
            .filter(|step| matches!(step, Step::Spawn { .. }))
            .count();
        let actions = self
            .step
            .iter()
            .map(|step| step.action_name().to_owned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let steps = self
            .step
            .iter()
            .enumerate()
            .map(|(index, step)| StepSummary {
                line: self.step_lines.get(index).copied(),
                action: step.action_name().to_owned(),
                detail: step.summary(),
            })
            .collect::<Vec<_>>();
        CheckReport {
            scenario: path.display().to_string(),
            valid: true,
            peers: self
                .peer
                .iter()
                .map(|peer| PeerSummary {
                    name: peer.name.clone(),
                    viewport: peer.viewport.clone(),
                    dpr: peer.dpr,
                    touch: peer.touch.unwrap_or(false),
                })
                .collect(),
            step_count: self.step.len(),
            spawns,
            steps,
            actions,
            error: None,
        }
    }
}

fn validate_network(network: &NetworkSpec, at: &str) -> Result<()> {
    if let Some(jitter) = network.jitter_ms.filter(|jitter| *jitter > 0) {
        bail!(
            "{at}: jitter is not available through Network.emulateNetworkConditions; \
             remove jitter_ms rather than believing it applied (got {jitter} ms)"
        );
    }
    if let Some(loss) = network.loss_percent {
        if !loss.is_finite() || !(0.0..=100.0).contains(&loss) {
            bail!("{at}: network loss must be between 0 and 100");
        }
    }
    Ok(())
}

fn check_peer(names: &HashSet<&str>, at: &str, peer: &str) -> Result<()> {
    if !names.contains(peer) {
        bail!("{at}: unknown peer {peer:?}");
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub(crate) struct CheckReport {
    pub(crate) scenario: String,
    pub(crate) valid: bool,
    pub(crate) peers: Vec<PeerSummary>,
    pub(crate) step_count: usize,
    /// How many processes the scenario starts. A valid scenario can execute a
    /// command, so `check` says so rather than leaving it to be discovered.
    pub(crate) spawns: usize,
    pub(crate) steps: Vec<StepSummary>,
    pub(crate) actions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct StepSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) line: Option<usize>,
    pub(crate) action: String,
    pub(crate) detail: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct PeerSummary {
    pub(crate) name: String,
    pub(crate) viewport: String,
    pub(crate) dpr: Option<f64>,
    pub(crate) touch: bool,
}

impl CheckReport {
    fn invalid(path: &Path, error: String) -> Self {
        Self {
            scenario: path.display().to_string(),
            valid: false,
            peers: Vec::new(),
            step_count: 0,
            spawns: 0,
            steps: Vec::new(),
            actions: Vec::new(),
            error: Some(error),
        }
    }

    fn render_text(&self) -> String {
        let mut out = String::new();
        use std::fmt::Write as _;
        let _ = writeln!(out, "scenario  {}", self.scenario);
        let _ = writeln!(out, "peers     {}", self.peers.len());
        for peer in &self.peers {
            let dpr = peer
                .dpr
                .map_or_else(String::new, |dpr| format!(" dpr {dpr}"));
            let touch = if peer.touch { " touch" } else { "" };
            let _ = writeln!(out, "  {:<10} {}{dpr}{touch}", peer.name, peer.viewport);
        }
        let _ = writeln!(out, "steps     {}", self.step_count);
        for step in &self.steps {
            let line = step
                .line
                .map_or_else(|| "  -".to_owned(), |line| format!("{line:>3}"));
            let _ = writeln!(out, "  {line}  {:<8} {}", step.action, step.detail);
        }
        if self.spawns > 0 {
            let _ = writeln!(
                out,
                "spawns    {}  (this scenario starts {} process{})",
                self.spawns,
                self.spawns,
                if self.spawns == 1 { "" } else { "es" }
            );
        }
        if !self.actions.is_empty() {
            let _ = writeln!(out, "actions   {}", self.actions.join(", "));
        }
        if let Some(error) = &self.error {
            let _ = writeln!(out, "error     {error}");
        }
        let _ = writeln!(
            out,
            "result    {}",
            if self.valid { "valid" } else { "INVALID" }
        );
        out
    }
}

pub(crate) fn check(args: Vec<String>) -> Result<bool> {
    let mut scenario_path = None;
    let mut json = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenario" => {
                scenario_path = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| anyhow!("--scenario needs a value"))?,
                ));
            }
            "--json" => json = true,
            "-h" | "--help" => {
                println!("browser-shot check --scenario <file.toml|file.dsl> [--json]");
                return Ok(true);
            }
            other => bail!("unknown option {other}; try --help"),
        }
    }

    let path = scenario_path.ok_or_else(|| anyhow!("--scenario is required"))?;
    match Scenario::load(&path) {
        Ok(scenario) => {
            let report = scenario.check_report(&path);
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", report.render_text());
            }
            Ok(true)
        }
        Err(error) => {
            if json {
                let report = CheckReport::invalid(&path, format!("{error:#}"));
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                eprintln!("browser-shot check: {error:#}");
            }
            Ok(false)
        }
    }
}

const RUN_FIELDS: &[&str] = &["timeout", "artifacts"];
const PEER_FIELDS: &[&str] = &[
    "url",
    "viewport",
    "dpr",
    "user_agent",
    "browser",
    "preload",
    "frame_counter",
    "args",
    "network",
    "loss_hook",
];
const WAIT_FIELDS: &[&str] = &[
    "predicate",
    "frames",
    "rate",
    "network_idle",
    "selector",
    "state",
    "text",
    "url",
    "url~",
    "stable",
    "timeout",
];
const KEY_FIELDS: &[&str] = &["hold"];
const POINTER_FIELDS: &[&str] = &["selector", "at", "steps", "button", "from"];
const CAPTURE_FIELDS: &[&str] = &["out", "select"];
const BURST_FIELDS: &[&str] = &["out", "frames", "interval", "mode", "select"];
const ASSERT_FIELDS: &[&str] = &["message"];
const NETWORK_FIELDS: &[&str] = &["latency", "download", "upload", "loss"];
const SPAWN_FIELDS: &[&str] = &["command", "port", "dir", "env"];
const WAIT_FOR_FIELDS: &[&str] = &[
    "ready", "port", "host", "http", "status", "log", "pattern", "exit", "code", "timeout",
];

pub(crate) fn fmt(args: Vec<String>) -> Result<bool> {
    let mut scenario_path = None;
    let mut write = false;
    let mut check = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenario" => {
                scenario_path = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| anyhow!("--scenario needs a value"))?,
                ));
            }
            "--write" => write = true,
            "--check" => check = true,
            "-h" | "--help" => {
                println!("browser-shot fmt --scenario <file.dsl> [--write|--check]");
                return Ok(true);
            }
            other => bail!("unknown option {other}; try --help"),
        }
    }
    if write && check {
        bail!("--write and --check are mutually exclusive");
    }
    let path = scenario_path.ok_or_else(|| anyhow!("--scenario is required"))?;
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension == "toml" {
        bail!("fmt formats the DSL; TOML is not rewritten");
    }

    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let formatted = match format_dsl(&text).with_context(|| format!("format {}", path.display())) {
        Ok(formatted) => formatted,
        Err(error) => {
            eprintln!("browser-shot fmt: {error:#}");
            return Ok(false);
        }
    };

    if check {
        if formatted == text {
            println!("unchanged {}", path.display());
            return Ok(true);
        }
        println!("would format {}", path.display());
        return Ok(false);
    }
    if write {
        fs::write(&path, &formatted).with_context(|| format!("write {}", path.display()))?;
        println!("formatted {}", path.display());
        return Ok(true);
    }
    print!("{formatted}");
    Ok(true)
}

fn format_dsl(text: &str) -> Result<String> {
    // Formatting is a syntax operation. `check` and `run` own the semantic rules, so a
    // half-written scenario can still be laid out before its peers exist.
    parse_dsl(text)?;
    let mut lines = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let parsed = tokenize_line(raw, index + 1)?;
        if parsed.tokens.is_empty() {
            if let Some(comment) = parsed.comment {
                lines.push(comment);
            } else if lines.last().is_some_and(|line| !line.is_empty()) {
                lines.push(String::new());
            }
        } else {
            lines.push(format_line(&parsed)?);
        }
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    if lines.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!("{}\n", lines.join("\n")))
    }
}

fn format_line(line: &LineTokens) -> Result<String> {
    let first = line
        .tokens
        .first()
        .ok_or_else(|| anyhow!("internal formatter error: empty line"))?;
    if first.is_kv() {
        bail_at!(first, "a line must start with an action");
    }
    let action = first.value.as_str();
    let rest = &line.tokens[1..];
    let positionals = positionals(rest);
    let mut kvs = collect_kvs(rest)?;
    let mut parts = vec![action.to_owned()];

    match action {
        "run" => push_fields(&mut parts, &mut kvs, RUN_FIELDS),
        "peer" => {
            let name = positionals
                .first()
                .ok_or_else(|| anyhow!("line {}: peer needs a name", first.line))?;
            parts.push(format_value(&name.value));
            let mut touch = None;
            let mut headed = None;
            for token in positionals.iter().skip(1) {
                match token.value.as_str() {
                    "touch" => touch = Some(true),
                    "headed" => headed = Some(true),
                    other => bail_at!(
                        token,
                        &format!("peer: unexpected value {other:?}; use key=value fields"),
                    ),
                }
            }
            if let Some(value) = take_bool(&mut kvs, "touch")? {
                touch = Some(value);
            }
            if let Some(value) = take_bool(&mut kvs, "headed")? {
                headed = Some(value);
            }
            push_fields(&mut parts, &mut kvs, PEER_FIELDS);
            push_flag(&mut parts, "touch", touch, "touch=false");
            push_flag(&mut parts, "headed", headed, "headed=false");
        }
        "wait" => {
            let peer_list = positionals
                .first()
                .ok_or_else(|| anyhow!("line {}: wait needs a peer or peer list", first.line))?;
            parts.push(format_value(&peer_list.value));
            push_fields(&mut parts, &mut kvs, WAIT_FIELDS);
        }
        "sleep" => {
            if positionals.len() != 2 {
                bail_at!(first, "sleep needs a peer and a duration such as 1s");
            }
            parts.push(format_value(&positionals[0].value));
            parts.push(format_value(&positionals[1].value));
        }
        "eval" => {
            if positionals.len() < 2 {
                bail_at!(first, "eval needs a peer and an expression");
            }
            parts.push(format_value(&positionals[0].value));
            parts.push(format_value(&positionals[1].value));
            let mut await_promise = None;
            for token in positionals.iter().skip(2) {
                match token.value.as_str() {
                    "await" => await_promise = Some(true),
                    other => bail_at!(token, &format!("eval: unexpected value {other:?}")),
                }
            }
            if let Some(value) = take_bool(&mut kvs, "await")? {
                await_promise = Some(value);
            }
            push_flag(&mut parts, "await", await_promise, "await=false");
            push_fields(&mut parts, &mut kvs, &["capture", "out", "timeout"]);
        }
        "key" => {
            if positionals.len() != 3 {
                bail_at!(first, "key needs a peer, code and state");
            }
            parts.push(format_value(&positionals[0].value));
            parts.push(format_value(&positionals[1].value));
            parts.push(format_value(&positionals[2].value));
            push_fields(&mut parts, &mut kvs, KEY_FIELDS);
        }
        "pointer" => {
            if positionals.len() != 2 {
                bail_at!(first, "pointer needs a peer and a kind");
            }
            parts.push(format_value(&positionals[0].value));
            parts.push(format_value(&positionals[1].value));
            push_fields(&mut parts, &mut kvs, POINTER_FIELDS);
        }
        "capture" => {
            if positionals.len() != 1 {
                bail_at!(first, "capture needs a peer");
            }
            parts.push(format_value(&positionals[0].value));
            push_fields(&mut parts, &mut kvs, CAPTURE_FIELDS);
        }
        "burst" => {
            if positionals.len() != 1 {
                bail_at!(first, "burst needs a peer");
            }
            parts.push(format_value(&positionals[0].value));
            push_fields(&mut parts, &mut kvs, BURST_FIELDS);
        }
        "assert" => {
            if positionals.len() != 2 {
                bail_at!(first, "assert needs a peer and one expression");
            }
            parts.push(format_value(&positionals[0].value));
            parts.push(format_value(&positionals[1].value));
            push_fields(&mut parts, &mut kvs, ASSERT_FIELDS);
        }
        "network" => {
            if positionals.len() != 1 {
                bail_at!(first, "network needs a peer");
            }
            parts.push(format_value(&positionals[0].value));
            push_fields(&mut parts, &mut kvs, NETWORK_FIELDS);
        }
        "spawn" => {
            if positionals.len() != 1 {
                bail_at!(first, "spawn needs a name");
            }
            parts.push(format_value(&positionals[0].value));
            push_fields(&mut parts, &mut kvs, SPAWN_FIELDS);
        }
        "wait_for" => push_fields(&mut parts, &mut kvs, WAIT_FOR_FIELDS),
        "goto" => {
            if positionals.len() != 2 {
                bail_at!(first, "goto needs a peer and a url");
            }
            parts.push(format_value(&positionals[0].value));
            parts.push(format_value(&positionals[1].value));
        }
        other => bail_at!(
            first,
            &format!(
                "unknown action {other:?}; expected run, peer, wait, sleep, eval, key, pointer, capture, \
                 burst, assert, network, spawn, wait_for or goto"
            ),
        ),
    }

    reject_unknown(&kvs, first, action)?;
    let text = parts.join(" ");
    Ok(match &line.comment {
        Some(comment) => format!("{text}  {comment}"),
        None => text,
    })
}

fn push_fields(parts: &mut Vec<String>, kvs: &mut BTreeMap<String, Token>, order: &[&str]) {
    for key in order {
        if let Some(token) = kvs.remove(*key) {
            parts.push(format!("{key}={}", format_value(&token.value)));
        }
    }
}

fn push_flag(parts: &mut Vec<String>, name: &str, value: Option<bool>, false_form: &str) {
    match value {
        Some(true) => parts.push(name.to_owned()),
        Some(false) => parts.push(false_form.to_owned()),
        None => {}
    }
}

fn format_value(value: &str) -> String {
    let quote = value.is_empty()
        || value.chars().any(|character| {
            character.is_whitespace() || matches!(character, '=' | '"' | '\'' | '\\' | '#')
        });
    if !quote {
        return value.to_owned();
    }
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
}

pub(crate) fn parse_viewport(value: &str) -> Result<(u32, u32)> {
    let (width, height) = value
        .split_once('x')
        .ok_or_else(|| anyhow!("viewport must be WxH, got {value:?}"))?;
    let width: u32 = width
        .parse()
        .with_context(|| format!("viewport width {width:?} is not a number"))?;
    let height: u32 = height
        .parse()
        .with_context(|| format!("viewport height {height:?} is not a number"))?;
    if width == 0 || height == 0 {
        bail!("viewport must be greater than zero");
    }
    Ok((width, height))
}

#[derive(Debug, Clone)]
struct Token {
    value: String,
    key: Option<String>,
    line: usize,
    column: usize,
}

impl Token {
    fn is_kv(&self) -> bool {
        self.key.is_some()
    }
}

fn parse_dsl(text: &str) -> Result<Scenario> {
    let mut run = RunConfig::default();
    let mut run_seen = false;
    let mut peers = Vec::new();
    let mut peer_lines = Vec::new();
    let mut steps = Vec::new();
    let mut step_lines = Vec::new();

    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let parsed = tokenize_line(raw, line)?;
        let tokens = parsed.tokens;
        let Some(first) = tokens.first() else {
            continue;
        };
        if first.is_kv() {
            bail_at!(
                first,
                "expected a line to start with run, peer or a step action"
            );
        }
        let action = first.value.as_str();
        let rest = &tokens[1..];
        match action {
            "run" => {
                if run_seen {
                    bail_at!(first, "run may appear only once");
                }
                run = parse_run(rest, first)?;
                run_seen = true;
            }
            "peer" => {
                peers.push(parse_peer(rest, first)?);
                peer_lines.push(first.line);
            }
            "wait" => {
                steps.push(parse_wait(rest, first)?);
                step_lines.push(first.line);
            }
            "sleep" => {
                steps.push(parse_sleep(rest, first)?);
                step_lines.push(first.line);
            }
            "eval" => {
                steps.push(parse_eval(rest, first)?);
                step_lines.push(first.line);
            }
            "key" => {
                steps.push(parse_key(rest, first)?);
                step_lines.push(first.line);
            }
            "pointer" => {
                steps.push(parse_pointer(rest, first)?);
                step_lines.push(first.line);
            }
            "capture" => {
                steps.push(parse_capture(rest, first)?);
                step_lines.push(first.line);
            }
            "burst" => {
                steps.push(parse_burst(rest, first)?);
                step_lines.push(first.line);
            }
            "assert" => {
                steps.push(parse_assert(rest, first)?);
                step_lines.push(first.line);
            }
            "network" => {
                steps.push(parse_network(rest, first)?);
                step_lines.push(first.line);
            }
            "spawn" => {
                steps.push(parse_spawn(rest, first)?);
                step_lines.push(first.line);
            }
            "wait_for" => {
                steps.push(parse_wait_for(rest, first)?);
                step_lines.push(first.line);
            }
            "goto" => {
                steps.push(parse_goto(rest, first)?);
                step_lines.push(first.line);
            }
            other => bail_at!(
                first,
                &format!(
                    "unknown action {other:?}; expected run, peer, wait, sleep, eval, key, pointer, capture, \
                     burst, assert, network, spawn, wait_for or goto"
                ),
            ),
        }
    }

    Ok(Scenario {
        run,
        peer: peers,
        step: steps,
        peer_lines,
        step_lines,
    })
}

struct LineTokens {
    tokens: Vec<Token>,
    comment: Option<String>,
}

fn tokenize_line(line: &str, number: usize) -> Result<LineTokens> {
    let chars: Vec<char> = line.chars().collect();
    let mut tokens = Vec::new();
    let mut comment = None;
    let mut index = 0;

    while index < chars.len() {
        while index < chars.len() && chars[index].is_whitespace() {
            index += 1;
        }
        if index >= chars.len() {
            break;
        }
        if chars[index] == '#' {
            comment = Some(
                chars[index..]
                    .iter()
                    .collect::<String>()
                    .trim_end()
                    .to_owned(),
            );
            break;
        }

        let column = index + 1;
        if chars[index] == '"' || chars[index] == '\'' {
            let (value, next) = read_quoted(&chars, index, number, column)?;
            tokens.push(Token {
                value,
                key: None,
                line: number,
                column,
            });
            index = next;
            if index < chars.len() && !chars[index].is_whitespace() {
                bail!(
                    "line {number}:{}: expected whitespace after a quoted value",
                    index + 1
                );
            }
            continue;
        }

        let start = index;
        let mut key = None;
        while index < chars.len() && !chars[index].is_whitespace() {
            if chars[index] == '=' {
                let candidate: String = chars[start..index].iter().collect();
                if candidate.is_empty() {
                    bail!(
                        "line {number}:{column}: unexpected '='; quote expressions that contain spaces or '='"
                    );
                }
                key = Some(candidate);
                index += 1;
                break;
            }
            index += 1;
        }

        if let Some(key) = key {
            let value = if index < chars.len() && (chars[index] == '"' || chars[index] == '\'') {
                let (value, next) = read_quoted(&chars, index, number, index + 1)?;
                index = next;
                if index < chars.len() && !chars[index].is_whitespace() {
                    bail!(
                        "line {number}:{}: expected whitespace after a quoted value",
                        index + 1
                    );
                }
                value
            } else {
                let start = index;
                while index < chars.len() && !chars[index].is_whitespace() {
                    index += 1;
                }
                chars[start..index].iter().collect()
            };
            tokens.push(Token {
                value,
                key: Some(key),
                line: number,
                column,
            });
        } else {
            let value: String = chars[start..index].iter().collect();
            tokens.push(Token {
                value,
                key: None,
                line: number,
                column,
            });
        }
    }

    Ok(LineTokens { tokens, comment })
}

fn read_quoted(
    chars: &[char],
    start: usize,
    line: usize,
    column: usize,
) -> Result<(String, usize)> {
    let quote = chars[start];
    let mut out = String::new();
    let mut index = start + 1;
    while index < chars.len() {
        let current = chars[index];
        if current == quote {
            return Ok((out, index + 1));
        }
        if current == '\\' {
            index += 1;
            let Some(escaped) = chars.get(index) else {
                bail!("line {line}:{column}: unterminated escape in a quoted value");
            };
            match escaped {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                '\'' => out.push('\''),
                other => {
                    out.push('\\');
                    out.push(*other);
                }
            }
            index += 1;
            continue;
        }
        out.push(current);
        index += 1;
    }
    bail!("line {line}:{column}: unterminated quoted value")
}

fn parse_run(tokens: &[Token], first: &Token) -> Result<RunConfig> {
    if let Some(extra) = positionals(tokens).first() {
        bail_at!(extra, "run takes fields only");
    }
    let mut kvs = collect_kvs(tokens)?;
    let timeout_ms = take_duration(&mut kvs, "timeout")?;
    let artifacts = take_string(&mut kvs, "artifacts").map(PathBuf::from);
    reject_unknown(&kvs, first, "run")?;
    Ok(RunConfig {
        artifacts,
        timeout_ms,
    })
}

fn parse_peer(tokens: &[Token], first: &Token) -> Result<PeerSpec> {
    let positionals = positionals(tokens);
    let name = positionals
        .first()
        .map(|token| token.value.clone())
        .ok_or_else(|| anyhow!("line {}: peer needs a name", first.line))?;
    let mut touch = None;
    let mut headed = None;
    for token in positionals.iter().skip(1) {
        match token.value.as_str() {
            "touch" => touch = Some(true),
            "headed" => headed = Some(true),
            other => bail_at!(
                token,
                &format!("peer {name:?}: unexpected value {other:?}; use key=value fields"),
            ),
        }
    }

    let mut kvs = collect_kvs(tokens)?;
    let url = take_required(&mut kvs, "url", first, "peer")?;
    let viewport = take_required(&mut kvs, "viewport", first, "peer")?;
    let dpr = take_f64(&mut kvs, "dpr")?;
    let user_agent = take_string(&mut kvs, "user_agent");
    let browser = take_string(&mut kvs, "browser").map(PathBuf::from);
    let preload = take_string(&mut kvs, "preload").map(PathBuf::from);
    let frame_counter = take_string(&mut kvs, "frame_counter");
    let args = take_string(&mut kvs, "args").map(|value| {
        value
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    });
    let network = take_string(&mut kvs, "network")
        .map(|value| parse_network_map(&value, first))
        .transpose()?;
    let loss_hook = take_string(&mut kvs, "loss_hook")
        .map(|value| parse_loss_hook_map(&value, first))
        .transpose()?;
    if let Some(value) = take_bool(&mut kvs, "touch")? {
        touch = Some(value);
    }
    if let Some(value) = take_bool(&mut kvs, "headed")? {
        headed = Some(value);
    }
    reject_unknown(&kvs, first, "peer")?;

    Ok(PeerSpec {
        name,
        url,
        viewport,
        dpr,
        touch,
        user_agent,
        headed,
        browser,
        preload,
        frame_counter,
        args,
        network,
        loss_hook,
    })
}

fn parse_wait(tokens: &[Token], first: &Token) -> Result<Step> {
    let positionals = positionals(tokens);
    let peer_list = positionals
        .first()
        .map(|token| token.value.clone())
        .ok_or_else(|| {
            anyhow!(
                "line {}: wait needs a peer or a comma-separated list",
                first.line
            )
        })?;
    let peers = peer_list
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if peers.is_empty() {
        bail_at!(first, "wait needs at least one peer");
    }
    if positionals.len() > 1 {
        bail_at!(
            positionals[1],
            "wait takes one peer list; put peer names in a,b form",
        );
    }

    let mut kvs = collect_kvs(tokens)?;
    let predicate = take_string(&mut kvs, "predicate");
    let frames = take_u32(&mut kvs, "frames")?;
    let rate = take_f64(&mut kvs, "rate")?;
    let network_idle = take_duration(&mut kvs, "network_idle")?;
    let selector = take_string(&mut kvs, "selector");
    let state = take_string(&mut kvs, "state");
    let text = take_string(&mut kvs, "text");
    let url = take_string(&mut kvs, "url");
    let url_contains = take_string(&mut kvs, "url~");
    let stable_ms = take_duration(&mut kvs, "stable")?;
    let timeout_ms = take_duration(&mut kvs, "timeout")?;
    reject_unknown(&kvs, first, "wait")?;

    if peers.len() == 1 {
        Ok(Step::Wait {
            peer: peers.into_iter().next(),
            peers: None,
            predicate,
            frames,
            rate,
            network_idle,
            selector,
            state,
            text,
            url,
            url_contains,
            stable_ms,
            timeout_ms,
        })
    } else {
        Ok(Step::Wait {
            peer: None,
            peers: Some(peers),
            predicate,
            frames,
            rate,
            network_idle,
            selector,
            state,
            text,
            url,
            url_contains,
            stable_ms,
            timeout_ms,
        })
    }
}

fn parse_sleep(tokens: &[Token], first: &Token) -> Result<Step> {
    let positionals = positionals(tokens);
    let peer = positionals
        .first()
        .map(|token| token.value.clone())
        .ok_or_else(|| anyhow!("line {}: sleep needs a peer", first.line))?;
    let duration = positionals.get(1).ok_or_else(|| {
        anyhow!(
            "line {}: sleep needs a duration such as 1s or 500ms",
            first.line
        )
    })?;
    if positionals.len() > 2 {
        bail_at!(positionals[2], "sleep takes a peer and a duration only");
    }
    let kvs = collect_kvs(tokens)?;
    reject_unknown(&kvs, first, "sleep")?;
    let duration_ms = parse_duration_ms(&duration.value)
        .with_context(|| format!("line {}:{}: duration", duration.line, duration.column))?;
    if duration_ms == 0 {
        bail_at!(duration, "sleep duration must be greater than zero");
    }
    Ok(Step::Sleep { peer, duration_ms })
}

fn parse_eval(tokens: &[Token], first: &Token) -> Result<Step> {
    let positionals = positionals(tokens);
    let peer = positionals
        .first()
        .map(|token| token.value.clone())
        .ok_or_else(|| anyhow!("line {}: eval needs a peer", first.line))?;
    let expression = positionals
        .get(1)
        .map(|token| token.value.clone())
        .ok_or_else(|| {
            anyhow!(
                "line {}: eval needs an expression; quote expressions that contain spaces",
                first.line
            )
        })?;
    let mut await_promise = false;
    for token in positionals.iter().skip(2) {
        match token.value.as_str() {
            "await" => await_promise = true,
            other => bail_at!(token, &format!("eval: unexpected value {other:?}")),
        }
    }

    let mut kvs = collect_kvs(tokens)?;
    if let Some(value) = take_bool(&mut kvs, "await")? {
        await_promise = value;
    }
    let capture = take_string(&mut kvs, "capture");
    let out = take_string(&mut kvs, "out");
    let timeout_ms = take_duration(&mut kvs, "timeout")?;
    reject_unknown(&kvs, first, "eval")?;

    Ok(Step::Eval {
        peer,
        expression,
        await_promise,
        capture,
        out,
        timeout_ms,
    })
}

fn parse_key(tokens: &[Token], first: &Token) -> Result<Step> {
    let positionals = positionals(tokens);
    let peer = positionals
        .first()
        .map(|token| token.value.clone())
        .ok_or_else(|| anyhow!("line {}: key needs a peer", first.line))?;
    let code = positionals
        .get(1)
        .map(|token| token.value.clone())
        .ok_or_else(|| anyhow!("line {}: key needs a code such as KeyW", first.line))?;
    let state = match positionals.get(2).map(|token| token.value.as_str()) {
        Some("down") => KeyState::Down,
        Some("up") => KeyState::Up,
        Some(other) => bail_at!(
            positionals[2],
            &format!("key state must be down or up, got {other:?}"),
        ),
        None => bail_at!(first, "key needs a state: down or up"),
    };
    if positionals.len() > 3 {
        bail_at!(positionals[3], "key takes peer, code and state only");
    }

    let mut kvs = collect_kvs(tokens)?;
    let hold_frames = take_u32(&mut kvs, "hold")?;
    reject_unknown(&kvs, first, "key")?;

    Ok(Step::Key {
        peer,
        code,
        state,
        hold_frames,
    })
}

fn parse_pointer(tokens: &[Token], first: &Token) -> Result<Step> {
    let positionals = positionals(tokens);
    let peer = positionals
        .first()
        .map(|token| token.value.clone())
        .ok_or_else(|| anyhow!("line {}: pointer needs a peer", first.line))?;
    let kind = positionals
        .get(1)
        .map(|token| token.value.clone())
        .ok_or_else(|| anyhow!("line {}: pointer needs a kind", first.line))?;
    if positionals.len() > 2 {
        bail_at!(positionals[2], "pointer takes peer and kind only");
    }

    let mut kvs = collect_kvs(tokens)?;
    let selector = take_string(&mut kvs, "selector");
    let at = take_string(&mut kvs, "at");
    let steps = take_u32(&mut kvs, "steps")?;
    let button = take_string(&mut kvs, "button");
    let from = take_string(&mut kvs, "from");
    reject_unknown(&kvs, first, "pointer")?;

    let (x, y) = match at {
        Some(at) => {
            let (x, y) = parse_point(&at, "at", first)?;
            (Some(x), Some(y))
        }
        None => (None, None),
    };
    let (from_x, from_y) = match from {
        Some(from) => {
            let (x, y) = parse_point(&from, "from", first)?;
            (Some(x), Some(y))
        }
        None => (None, None),
    };

    Ok(Step::Pointer {
        peer,
        kind,
        x,
        y,
        selector,
        steps,
        button,
        from_x,
        from_y,
    })
}

fn parse_point(value: &str, field: &str, first: &Token) -> Result<(f64, f64)> {
    let (x, y) = value
        .split_once(',')
        .ok_or_else(|| anyhow!("line {}: {field} must be x,y", first.line))?;
    let x = x
        .trim()
        .parse::<f64>()
        .with_context(|| format!("line {}: {field} x {x:?}", first.line))?;
    let y = y
        .trim()
        .parse::<f64>()
        .with_context(|| format!("line {}: {field} y {y:?}", first.line))?;
    Ok((x, y))
}

fn parse_capture(tokens: &[Token], first: &Token) -> Result<Step> {
    let positionals = positionals(tokens);
    let peer = positionals
        .first()
        .map(|token| token.value.clone())
        .ok_or_else(|| anyhow!("line {}: capture needs a peer", first.line))?;
    if positionals.len() > 1 {
        bail_at!(positionals[1], "capture takes a peer and fields only");
    }

    let mut kvs = collect_kvs(tokens)?;
    let out = take_required(&mut kvs, "out", first, "capture")?;
    let select = take_string(&mut kvs, "select");
    reject_unknown(&kvs, first, "capture")?;

    Ok(Step::Capture { peer, out, select })
}

fn parse_burst(tokens: &[Token], first: &Token) -> Result<Step> {
    let positionals = positionals(tokens);
    let peer = positionals
        .first()
        .map(|token| token.value.clone())
        .ok_or_else(|| anyhow!("line {}: burst needs a peer", first.line))?;
    if positionals.len() > 1 {
        bail_at!(positionals[1], "burst takes a peer and fields only");
    }

    let mut kvs = collect_kvs(tokens)?;
    let out = take_required(&mut kvs, "out", first, "burst")?;
    let frames = take_u32(&mut kvs, "frames")?
        .ok_or_else(|| anyhow!("line {}: burst needs frames=...", first.line))?;
    let interval_ms = take_duration(&mut kvs, "interval")?;
    let mode = take_string(&mut kvs, "mode");
    let select = take_string(&mut kvs, "select");
    reject_unknown(&kvs, first, "burst")?;

    Ok(Step::Burst {
        peer,
        out,
        frames,
        interval_ms,
        mode,
        select,
    })
}

fn parse_assert(tokens: &[Token], first: &Token) -> Result<Step> {
    let positionals = positionals(tokens);
    let peer = positionals
        .first()
        .map(|token| token.value.clone())
        .ok_or_else(|| anyhow!("line {}: assert needs a peer", first.line))?;
    let expression = positionals
        .get(1)
        .map(|token| token.value.clone())
        .ok_or_else(|| {
            anyhow!(
                "line {}: assert needs an expression; quote expressions that contain spaces",
                first.line
            )
        })?;
    if positionals.len() > 2 {
        bail_at!(
            positionals[2],
            "assert takes a peer and one quoted expression; use message=... for a label",
        );
    }

    let mut kvs = collect_kvs(tokens)?;
    let message = take_string(&mut kvs, "message");
    reject_unknown(&kvs, first, "assert")?;

    Ok(Step::Assert {
        peer,
        expression,
        message,
    })
}

fn parse_network(tokens: &[Token], first: &Token) -> Result<Step> {
    let positionals = positionals(tokens);
    let peer = positionals
        .first()
        .map(|token| token.value.clone())
        .ok_or_else(|| anyhow!("line {}: network needs a peer", first.line))?;
    if positionals.len() > 1 {
        bail_at!(positionals[1], "network takes a peer and fields only");
    }

    let mut kvs = collect_kvs(tokens)?;
    let latency_ms = take_duration(&mut kvs, "latency")?;
    let download_kbps = take_u64(&mut kvs, "download")?;
    let upload_kbps = take_u64(&mut kvs, "upload")?;
    let loss_percent = take_percent(&mut kvs, "loss")?;
    reject_unknown(&kvs, first, "network")?;

    Ok(Step::Network {
        peer,
        latency_ms,
        download_kbps,
        upload_kbps,
        loss_percent,
    })
}

fn parse_spawn(tokens: &[Token], first: &Token) -> Result<Step> {
    let positionals = positionals(tokens);
    let name = positionals
        .first()
        .map(|token| token.value.clone())
        .ok_or_else(|| anyhow!("line {}: spawn needs a name", first.line))?;
    if positionals.len() > 1 {
        bail_at!(positionals[1], "spawn takes a name and fields only");
    }

    let mut kvs = collect_kvs(tokens)?;
    let command = take_required(&mut kvs, "command", first, "spawn")?;
    let port = take_u16(&mut kvs, "port", first)?;
    let dir = take_string(&mut kvs, "dir");
    let env = take_string(&mut kvs, "env").map(|env| split_list(&env));
    reject_unknown(&kvs, first, "spawn")?;

    Ok(Step::Spawn {
        name,
        command,
        port,
        dir,
        env,
    })
}

fn parse_wait_for(tokens: &[Token], first: &Token) -> Result<Step> {
    if !positionals(tokens).is_empty() {
        bail_at!(
            first,
            "wait_for takes fields only; name a process with ready=, log= or exit="
        );
    }

    let mut kvs = collect_kvs(tokens)?;
    let ready = take_string(&mut kvs, "ready");
    let port = take_u16(&mut kvs, "port", first)?;
    let host = take_string(&mut kvs, "host");
    let http = take_string(&mut kvs, "http");
    let status = take_u16(&mut kvs, "status", first)?;
    let log = take_string(&mut kvs, "log");
    let pattern = take_string(&mut kvs, "pattern");
    let exit = take_string(&mut kvs, "exit");
    let code = take_u32(&mut kvs, "code")?
        .map(|code| {
            i32::try_from(code).map_err(|_| anyhow!("line {}: code is too large", first.line))
        })
        .transpose()?;
    let timeout_ms = take_duration(&mut kvs, "timeout")?;
    reject_unknown(&kvs, first, "wait_for")?;

    Ok(Step::WaitFor {
        ready,
        port,
        host,
        http,
        status,
        log,
        pattern,
        exit,
        code,
        timeout_ms,
    })
}

fn parse_goto(tokens: &[Token], first: &Token) -> Result<Step> {
    let positionals = positionals(tokens);
    let peer = positionals
        .first()
        .map(|token| token.value.clone())
        .ok_or_else(|| anyhow!("line {}: goto needs a peer", first.line))?;
    let url = positionals
        .get(1)
        .map(|token| token.value.clone())
        .ok_or_else(|| anyhow!("line {}: goto needs a url", first.line))?;
    if positionals.len() > 2 {
        bail_at!(positionals[2], "goto takes a peer and a url only");
    }

    let kvs = collect_kvs(tokens)?;
    reject_unknown(&kvs, first, "goto")?;

    Ok(Step::Goto { peer, url })
}

fn take_u16(kvs: &mut BTreeMap<String, Token>, key: &str, first: &Token) -> Result<Option<u16>> {
    take_u32(kvs, key)?
        .map(|value| {
            u16::try_from(value)
                .map_err(|_| anyhow!("line {}: {key} must be at most 65535", first.line))
        })
        .transpose()
}

/// A comma-separated list, trimmed.
fn split_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

fn parse_network_map(value: &str, first: &Token) -> Result<NetworkSpec> {
    let mut network = NetworkSpec::default();
    for part in value.split(',') {
        let (key, value) = part
            .split_once(':')
            .ok_or_else(|| anyhow!("line {}: network expects key:value pairs", first.line))?;
        match key.trim() {
            "latency" => network.latency_ms = Some(parse_duration_ms(value.trim())?),
            "download" => network.download_kbps = Some(parse_u64_value(value.trim(), "download")?),
            "upload" => network.upload_kbps = Some(parse_u64_value(value.trim(), "upload")?),
            "loss" => network.loss_percent = Some(parse_percent_value(value.trim(), "loss")?),
            "jitter" => network.jitter_ms = Some(parse_duration_ms(value.trim())?),
            other => bail_at!(first, &format!("peer: unknown network field {other:?}")),
        }
    }
    Ok(network)
}

fn parse_loss_hook_map(value: &str, first: &Token) -> Result<LossHookSpec> {
    let mut rate = None;
    let mut seed = None;
    for part in value.split(',') {
        let (key, value) = part
            .split_once(':')
            .ok_or_else(|| anyhow!("line {}: loss_hook expects key:value pairs", first.line))?;
        match key.trim() {
            "rate" => {
                rate = Some(
                    value
                        .trim()
                        .parse::<f64>()
                        .with_context(|| format!("line {}: loss_hook rate", first.line))?,
                );
            }
            "seed" => {
                seed = Some(
                    value
                        .trim()
                        .parse::<u32>()
                        .with_context(|| format!("line {}: loss_hook seed", first.line))?,
                );
            }
            other => bail_at!(first, &format!("peer: unknown loss_hook field {other:?}")),
        }
    }
    let rate = rate.ok_or_else(|| anyhow!("line {}: loss_hook needs rate", first.line))?;
    Ok(LossHookSpec { rate, seed })
}

fn collect_kvs(tokens: &[Token]) -> Result<BTreeMap<String, Token>> {
    let mut kvs = BTreeMap::new();
    for token in tokens.iter().filter(|token| token.is_kv()) {
        let key = token.key.clone().expect("a key-value token has a key");
        if kvs.insert(key.clone(), token.clone()).is_some() {
            bail_at!(token, &format!("duplicate field {key:?}"));
        }
    }
    Ok(kvs)
}

fn positionals(tokens: &[Token]) -> Vec<&Token> {
    tokens.iter().filter(|token| !token.is_kv()).collect()
}

fn take_string(kvs: &mut BTreeMap<String, Token>, key: &str) -> Option<String> {
    kvs.remove(key).map(|token| token.value)
}

fn take_required(
    kvs: &mut BTreeMap<String, Token>,
    key: &str,
    first: &Token,
    context: &str,
) -> Result<String> {
    kvs.remove(key)
        .map(|token| token.value)
        .ok_or_else(|| anyhow!("line {}: {context} needs {key}=...", first.line))
}

fn take_u32(kvs: &mut BTreeMap<String, Token>, key: &str) -> Result<Option<u32>> {
    let Some(token) = kvs.remove(key) else {
        return Ok(None);
    };
    token.value.parse::<u32>().map(Some).with_context(|| {
        format!(
            "line {}:{}: {key} must be an integer",
            token.line, token.column
        )
    })
}

fn take_u64(kvs: &mut BTreeMap<String, Token>, key: &str) -> Result<Option<u64>> {
    let Some(token) = kvs.remove(key) else {
        return Ok(None);
    };
    token.value.parse::<u64>().map(Some).with_context(|| {
        format!(
            "line {}:{}: {key} must be an integer",
            token.line, token.column
        )
    })
}

fn take_f64(kvs: &mut BTreeMap<String, Token>, key: &str) -> Result<Option<f64>> {
    let Some(token) = kvs.remove(key) else {
        return Ok(None);
    };
    token.value.parse::<f64>().map(Some).with_context(|| {
        format!(
            "line {}:{}: {key} must be a number",
            token.line, token.column
        )
    })
}

fn take_duration(kvs: &mut BTreeMap<String, Token>, key: &str) -> Result<Option<u64>> {
    let Some(token) = kvs.remove(key) else {
        return Ok(None);
    };
    parse_duration_ms(&token.value)
        .map(Some)
        .with_context(|| format!("line {}:{}: {key}", token.line, token.column))
}

fn take_percent(kvs: &mut BTreeMap<String, Token>, key: &str) -> Result<Option<f64>> {
    let Some(token) = kvs.remove(key) else {
        return Ok(None);
    };
    parse_percent_value(&token.value, key)
        .map(Some)
        .with_context(|| format!("line {}:{}: {key}", token.line, token.column))
}

fn take_bool(kvs: &mut BTreeMap<String, Token>, key: &str) -> Result<Option<bool>> {
    let Some(token) = kvs.remove(key) else {
        return Ok(None);
    };
    match token.value.as_str() {
        "" | "true" | "1" => Ok(Some(true)),
        "false" | "0" => Ok(Some(false)),
        other => bail_at!(
            &token,
            &format!("{key} must be true or false, got {other:?}"),
        ),
    }
}

fn reject_unknown(kvs: &BTreeMap<String, Token>, _first: &Token, context: &str) -> Result<()> {
    if let Some((key, token)) = kvs.iter().next() {
        bail_at!(token, &format!("{context}: unknown field {key:?}"));
    }
    Ok(())
}

fn parse_duration_ms(value: &str) -> Result<u64> {
    let value = value.trim();
    let milliseconds = if let Some(value) = value.strip_suffix("ms") {
        value.parse::<f64>()?
    } else if let Some(value) = value.strip_suffix('s') {
        value.parse::<f64>()? * 1000.0
    } else {
        value.parse::<f64>()?
    };
    if !milliseconds.is_finite() || milliseconds < 0.0 {
        bail!("duration must not be negative");
    }
    Ok(milliseconds.round() as u64)
}

fn parse_u64_value(value: &str, key: &str) -> Result<u64> {
    value
        .parse::<u64>()
        .with_context(|| format!("{key} must be an integer"))
}

fn parse_percent_value(value: &str, key: &str) -> Result<f64> {
    let value = value.trim().strip_suffix('%').unwrap_or(value);
    let value: f64 = value
        .parse()
        .with_context(|| format!("{key} must be a number"))?;
    if !value.is_finite() {
        bail!("{key} must be finite");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYS: &str = r#"
run timeout=20s

peer focused url="file:///tmp/keys.html" viewport=390x844
peer unfocused url="file:///tmp/keys.html" viewport=390x844

pointer focused click selector=#game
key focused KeyW down hold=12
assert focused "window.__keys[1].frame - window.__keys[0].frame === 12" message="held twelve frames"
capture focused out=focused.png
"#;

    const TWO_PEER: &str = r#"
run timeout=30s

peer a url="file:///tmp/webrtc.html" viewport=390x844 network=latency:80,loss:2 loss_hook=rate:0.0,seed:1
peer b url="file:///tmp/webrtc.html" viewport=390x844

eval a "window.__createOffer()" await capture=offer
eval b "window.__acceptOffer(${offer})" await capture=answer
wait a,b predicate="window.__channelState === 'open'" timeout=15s
wait a,b frames=60
key a KeyW down hold=12
network a latency=120 download=500 loss=1
capture a out=a.png select=canvas
assert b "window.__received === 100" message="all messages arrive"
"#;

    #[test]
    fn parses_the_keys_dsl() {
        let scenario = parse_dsl(KEYS).unwrap();
        scenario.validate().unwrap();
        assert_eq!(scenario.peer.len(), 2);
        assert_eq!(scenario.peer[0].name, "focused");
        assert_eq!(scenario.peer[0].viewport, "390x844");
        assert_eq!(scenario.run.timeout_ms, Some(20_000));
        assert_eq!(scenario.step.len(), 4);
        match &scenario.step[1] {
            Step::Key {
                peer,
                code,
                state,
                hold_frames,
            } => {
                assert_eq!(peer, "focused");
                assert_eq!(code, "KeyW");
                assert!(matches!(state, KeyState::Down));
                assert_eq!(*hold_frames, Some(12));
            }
            other => panic!("expected a key step, got {other:?}"),
        }
    }

    #[test]
    fn parses_the_two_peer_dsl() {
        let scenario = parse_dsl(TWO_PEER).unwrap();
        scenario.validate().unwrap();
        assert_eq!(scenario.peer.len(), 2);
        assert_eq!(
            scenario.peer[0].network.as_ref().unwrap().loss_percent,
            Some(2.0)
        );
        assert_eq!(scenario.peer[0].loss_hook.as_ref().unwrap().seed, Some(1));
        match &scenario.step[0] {
            Step::Eval {
                peer,
                expression,
                await_promise,
                capture,
                ..
            } => {
                assert_eq!(peer, "a");
                assert_eq!(expression, "window.__createOffer()");
                assert!(await_promise);
                assert_eq!(capture.as_deref(), Some("offer"));
            }
            other => panic!("expected an eval step, got {other:?}"),
        }
        match &scenario.step[2] {
            Step::Wait {
                peers, predicate, ..
            } => {
                assert_eq!(
                    peers.as_deref(),
                    Some(&["a".to_owned(), "b".to_owned()][..])
                );
                assert_eq!(
                    predicate.as_deref(),
                    Some("window.__channelState === 'open'")
                );
            }
            other => panic!("expected a wait step, got {other:?}"),
        }
    }

    #[test]
    fn rejects_an_unknown_peer() {
        let text = "peer a url=x viewport=390x844\nkey b KeyW down\n";
        let error = parse_dsl(text).unwrap().validate().unwrap_err();
        assert!(error.to_string().contains("unknown peer \"b\""), "{error}");
        assert!(error.to_string().contains("line 2"), "{error}");
    }

    #[test]
    fn formats_fields_in_a_canonical_order() {
        let input = "peer   a   viewport=390x844   url=x   touch\nkey a KeyW down hold=12\nassert a \"window.x === 1\" message=\"a message\" # note\n";
        let expected = "peer a url=x viewport=390x844 touch\nkey a KeyW down hold=12\nassert a \"window.x === 1\" message=\"a message\"  # note\n";
        let formatted = format_dsl(input).unwrap();
        assert_eq!(formatted, expected);
        assert_eq!(format_dsl(&formatted).unwrap(), formatted);
    }

    #[test]
    fn formats_without_requiring_semantic_validity() {
        assert_eq!(format_dsl("# draft\n").unwrap(), "# draft\n");
    }

    #[test]
    fn formats_hash_values_as_quoted() {
        let input = "peer a url=x viewport=390x844\npointer a click selector=#game\n";
        let formatted = format_dsl(input).unwrap();
        assert!(formatted.contains("selector=\"#game\""), "{formatted}");
    }

    #[test]
    fn formats_comments_and_blank_lines() {
        let input = "# header\n\n\npeer a url=x viewport=390x844\n";
        let formatted = format_dsl(input).unwrap();
        assert_eq!(formatted, "# header\n\npeer a url=x viewport=390x844\n");
    }

    #[test]
    fn parses_a_pointer_at_coordinates() {
        let text = "peer a url=x viewport=390x844\npointer a click at=200,400 steps=8\n";
        let scenario = parse_dsl(text).unwrap();
        scenario.validate().unwrap();
        match &scenario.step[0] {
            Step::Pointer {
                kind,
                x,
                y,
                selector,
                steps,
                ..
            } => {
                assert_eq!(kind, "click");
                assert_eq!((*x, *y), (Some(200.0), Some(400.0)));
                assert!(selector.is_none());
                assert_eq!(*steps, Some(8));
            }
            other => panic!("expected a pointer step, got {other:?}"),
        }
    }

    #[test]
    fn rejects_hold_on_key_up() {
        let text = "peer a url=x viewport=390x844\nkey a KeyW up hold=12\n";
        let error = parse_dsl(text).unwrap().validate().unwrap_err();
        assert!(error.to_string().contains("key-down"), "{error}");
    }

    #[test]
    fn rejects_a_pointer_with_no_target() {
        let text = "peer a url=x viewport=390x844\npointer a click\n";
        let error = parse_dsl(text).unwrap().validate().unwrap_err();
        assert!(
            error.to_string().contains("coordinates or a selector"),
            "{error}"
        );
    }

    #[test]
    fn parses_a_rate_wait() {
        let text = "peer a url=x viewport=390x844\nwait a rate=30 timeout=5s\n";
        let scenario = parse_dsl(text).unwrap();
        scenario.validate().unwrap();
        match &scenario.step[0] {
            Step::Wait { rate, .. } => assert_eq!(*rate, Some(30.0)),
            other => panic!("expected a wait step, got {other:?}"),
        }
        assert_eq!(format_dsl(text).unwrap(), text);
    }

    #[test]
    fn rejects_a_non_positive_rate_wait() {
        let text = "peer a url=x viewport=390x844\nwait a rate=0\n";
        let error = parse_dsl(text).unwrap().validate().unwrap_err();
        assert!(error.to_string().contains("positive number"), "{error}");
    }

    #[test]
    fn rejects_an_empty_wait() {
        let text = "peer a url=x viewport=390x844\nwait a\n";
        let error = parse_dsl(text).unwrap().validate().unwrap_err();
        assert!(
            error
                .to_string()
                .contains("predicate, frames, rate, network_idle, selector, text, url"),
            "{error}"
        );
    }
    #[test]
    fn parses_selector_text_url_and_stable_waits() {
        let text = concat!(
            "peer a url=x viewport=390x844\n",
            "wait a selector=\"#btn\" state=enabled timeout=2s\n",
            "wait a text=\"Hello\" selector=\"#modal\"\n",
            "wait a url~=\"#/wallet\"\n",
            "wait a url=\"http://127.0.0.1/exact\"\n",
            "wait a stable=500ms selector=\"#root\"\n",
            "sleep a 1s\n",
        );
        let scenario = parse_dsl(text).unwrap();
        scenario.validate().unwrap();
        match &scenario.step[0] {
            Step::Wait {
                selector, state, ..
            } => {
                assert_eq!(selector.as_deref(), Some("#btn"));
                assert_eq!(state.as_deref(), Some("enabled"));
            }
            other => panic!("{other:?}"),
        }
        match &scenario.step[1] {
            Step::Wait {
                text,
                selector,
                state,
                ..
            } => {
                assert_eq!(text.as_deref(), Some("Hello"));
                assert_eq!(selector.as_deref(), Some("#modal"));
                assert!(state.is_none());
            }
            other => panic!("{other:?}"),
        }
        match &scenario.step[2] {
            Step::Wait {
                url_contains, url, ..
            } => {
                assert_eq!(url_contains.as_deref(), Some("#/wallet"));
                assert!(url.is_none());
            }
            other => panic!("{other:?}"),
        }
        match &scenario.step[3] {
            Step::Wait { url, .. } => assert_eq!(url.as_deref(), Some("http://127.0.0.1/exact")),
            other => panic!("{other:?}"),
        }
        match &scenario.step[4] {
            Step::Wait {
                stable_ms,
                selector,
                ..
            } => {
                assert_eq!(*stable_ms, Some(500));
                assert_eq!(selector.as_deref(), Some("#root"));
            }
            other => panic!("{other:?}"),
        }
        match &scenario.step[5] {
            Step::Sleep { peer, duration_ms } => {
                assert_eq!(peer, "a");
                assert_eq!(*duration_ms, 1000);
            }
            other => panic!("{other:?}"),
        }
        let formatted = format_dsl(text).unwrap();
        assert!(formatted.contains("selector=\"#btn\""), "{formatted}");
        assert!(formatted.contains("url~=\"#/wallet\""), "{formatted}");
        assert!(formatted.contains("stable=500ms"), "{formatted}");
        assert!(formatted.contains("sleep a 1s"), "{formatted}");
        assert_eq!(format_dsl(&formatted).unwrap(), formatted);
    }

    #[test]
    fn rejects_state_without_selector() {
        let text = "peer a url=x viewport=390x844\nwait a state=visible\n";
        let error = parse_dsl(text).unwrap().validate().unwrap_err();
        assert!(
            error.to_string().contains("state= needs selector="),
            "{error}"
        );
    }

    #[test]
    fn rejects_unknown_selector_state() {
        let text = "peer a url=x viewport=390x844\nwait a selector=\"#x\" state=clickable\n";
        let error = parse_dsl(text).unwrap().validate().unwrap_err();
        assert!(
            error
                .to_string()
                .contains("present, visible, hidden, gone or enabled"),
            "{error}"
        );
    }

    #[test]
    fn rejects_url_and_url_contains_together() {
        let text = "peer a url=x viewport=390x844\nwait a url=\"/a\" url~=\"/b\"\n";
        let error = parse_dsl(text).unwrap().validate().unwrap_err();
        assert!(error.to_string().contains("url and url~"), "{error}");
    }

    #[test]
    fn rejects_empty_new_wait_fields() {
        let cases = [
            ("wait a selector=\"\"\n", "selector must not be empty"),
            ("wait a text=\"\"\n", "text must not be empty"),
            ("wait a url=\"\"\n", "url must not be empty"),
            ("wait a url~=\"\"\n", "url~ must not be empty"),
            ("wait a stable=0\n", "stable must be greater than zero"),
            ("sleep a 0ms\n", "greater than zero"),
        ];
        for (line, needle) in cases {
            let text = format!("peer a url=x viewport=390x844\n{line}");
            let error = match parse_dsl(&text) {
                Ok(scenario) => scenario.validate().unwrap_err().to_string(),
                Err(error) => error.to_string(),
            };
            assert!(
                error.contains(needle),
                "for {line}: {error} (want {needle})"
            );
        }
    }

    #[test]
    fn defaults_selector_state_to_visible_in_summary() {
        let text = "peer a url=x viewport=390x844\nwait a selector=\"#btn\"\n";
        let scenario = parse_dsl(text).unwrap();
        assert!(
            scenario.step[0].summary().contains("visible"),
            "{}",
            scenario.step[0].summary()
        );
    }

    #[test]
    fn rejects_an_empty_network_step() {
        let text = "peer a url=x viewport=390x844\nnetwork a\n";
        let error = parse_dsl(text).unwrap().validate().unwrap_err();
        assert!(
            error.to_string().contains("needs a network field"),
            "{error}"
        );
    }

    #[test]
    fn rejects_jitter_before_a_browser_is_launched() {
        let text = "peer a url=x viewport=390x844 network=jitter:5\n";
        let error = parse_dsl(text).unwrap().validate().unwrap_err();
        assert!(
            error.to_string().contains("jitter is not available"),
            "{error}"
        );
    }

    #[test]
    fn rejects_an_unterminated_quote() {
        let text = "peer a url=\"unterminated viewport=390x844\n";
        let error = parse_dsl(text).unwrap_err().to_string();
        assert!(error.contains("unterminated quoted value"), "{error}");
    }

    #[test]
    fn rejects_an_unknown_action() {
        let text = "tap a canvas\n";
        let error = parse_dsl(text).unwrap_err().to_string();
        assert!(error.contains("unknown action"), "{error}");
    }

    #[test]
    fn parses_a_toml_scenario_through_the_same_model() {
        let text = r#"
[[peer]]
name = "a"
url = "file:///tmp/a.html"
viewport = "390x844"

[[step]]
action = "key"
peer = "a"
code = "KeyW"
state = "down"
hold_frames = 12
"#;
        let scenario: Scenario = toml::from_str(text).unwrap();
        scenario.validate().unwrap();
        assert_eq!(scenario.step.len(), 1);
    }

    #[test]
    fn parses_and_formats_a_burst() {
        let text = "peer a url=x viewport=390x844\nburst a out=frames/pulse frames=120 interval=16ms mode=screencast\n";
        let scenario = parse_dsl(text).unwrap();
        scenario.validate().unwrap();
        match &scenario.step[0] {
            Step::Burst {
                peer,
                out,
                frames,
                interval_ms,
                mode,
                select,
            } => {
                assert_eq!(peer, "a");
                assert_eq!(out, "frames/pulse");
                assert_eq!(*frames, 120);
                assert_eq!(*interval_ms, Some(16));
                assert_eq!(mode.as_deref(), Some("screencast"));
                assert!(select.is_none());
            }
            other => panic!("expected a burst step, got {other:?}"),
        }
        assert_eq!(format_dsl(text).unwrap(), text);
    }

    #[test]
    fn rejects_a_screencast_burst_with_a_selector() {
        let text = "peer a url=x viewport=390x844\nburst a out=frames/pulse frames=20 mode=screencast select=canvas\n";
        let error = parse_dsl(text).unwrap().validate().unwrap_err();
        assert!(error.to_string().contains("remove select"), "{error}");
    }

    #[test]
    fn rejects_a_burst_with_no_frames() {
        let text = "peer a url=x viewport=390x844\nburst a out=frames/pulse frames=0\n";
        let error = parse_dsl(text).unwrap().validate().unwrap_err();
        assert!(
            error
                .to_string()
                .contains("frames must be greater than zero"),
            "{error}"
        );
    }

    #[test]
    fn parses_a_right_click() {
        let text = "peer a url=x viewport=390x844\npointer a click at=200,400 button=right\n";
        let scenario = parse_dsl(text).unwrap();
        scenario.validate().unwrap();
        match &scenario.step[0] {
            Step::Pointer { button, .. } => assert_eq!(button.as_deref(), Some("right")),
            other => panic!("expected a pointer step, got {other:?}"),
        }
    }

    #[test]
    fn parses_and_formats_a_drag() {
        let text = "peer a url=x viewport=390x844\npointer a drag from=100,120 at=240,320 button=middle steps=8\n";
        let scenario = parse_dsl(text).unwrap();
        scenario.validate().unwrap();
        match &scenario.step[0] {
            Step::Pointer {
                kind,
                button,
                from_x,
                from_y,
                ..
            } => {
                assert_eq!(kind, "drag");
                assert_eq!(button.as_deref(), Some("middle"));
                assert_eq!((*from_x, *from_y), (Some(100.0), Some(120.0)));
            }
            other => panic!("expected a pointer step, got {other:?}"),
        }
        let formatted = format_dsl(text).unwrap();
        assert_eq!(
            formatted,
            "peer a url=x viewport=390x844\npointer a drag at=240,320 steps=8 button=middle from=100,120\n"
        );
        assert_eq!(format_dsl(&formatted).unwrap(), formatted);
    }

    #[test]
    fn rejects_a_button_on_hover() {
        let text = "peer a url=x viewport=390x844\npointer a hover at=10,20 button=right\n";
        let error = parse_dsl(text).unwrap().validate().unwrap_err();
        assert!(
            error
                .to_string()
                .contains("button belongs on click or drag"),
            "{error}"
        );
    }

    #[test]
    fn rejects_a_touch_gesture_without_touch() {
        let text = "peer a url=x viewport=390x844\npointer a tap at=10,20\n";
        let error = parse_dsl(text).unwrap().validate().unwrap_err();
        assert!(error.to_string().contains("needs touch"), "{error}");
    }

    #[test]
    fn parses_an_eval_timeout() {
        let text = "peer a url=x viewport=390x844\neval a \"window.__late()\" await capture=late timeout=5s\n";
        let scenario = parse_dsl(text).unwrap();
        scenario.validate().unwrap();
        match &scenario.step[0] {
            Step::Eval { timeout_ms, .. } => assert_eq!(*timeout_ms, Some(5000)),
            other => panic!("expected an eval step, got {other:?}"),
        }
        assert!(format_dsl(text)
            .unwrap()
            .contains("await capture=late timeout=5s"));
    }

    #[test]
    fn rejects_a_timeout_on_a_synchronous_eval() {
        let text = "peer a url=x viewport=390x844\neval a window.__now() timeout=5s\n";
        let error = parse_dsl(text).unwrap().validate().unwrap_err();
        assert!(
            error.to_string().contains("timeout belongs on an awaited"),
            "{error}"
        );
    }

    #[test]
    fn parses_peer_args() {
        let text = "peer a url=x viewport=390x844 args=\"--use-fake-device-for-media-stream --use-fake-ui-for-media-stream\"\n";
        let scenario = parse_dsl(text).unwrap();
        scenario.validate().unwrap();
        assert_eq!(
            scenario.peer[0].args.as_deref(),
            Some(
                &[
                    "--use-fake-device-for-media-stream".to_owned(),
                    "--use-fake-ui-for-media-stream".to_owned()
                ][..]
            )
        );
        assert_eq!(format_dsl(text).unwrap(), text);
    }

    #[test]
    fn rejects_a_reserved_peer_arg() {
        let text = "peer a url=x viewport=390x844 args=\"--user-data-dir=/tmp/x\"\n";
        let error = parse_dsl(text).unwrap().validate().unwrap_err();
        assert!(
            error.to_string().contains("is set by browser-shot"),
            "{error}"
        );
    }

    #[test]
    fn parses_and_formats_an_eval_out() {
        let text = "peer a url=x viewport=390x844\neval a \"window.__samples\" out=samples.json\n";
        let scenario = parse_dsl(text).unwrap();
        scenario.validate().unwrap();
        match &scenario.step[0] {
            Step::Eval { out, .. } => assert_eq!(out.as_deref(), Some("samples.json")),
            other => panic!("expected an eval step, got {other:?}"),
        }
        let formatted = format_dsl(text).unwrap();
        assert_eq!(
            formatted,
            "peer a url=x viewport=390x844\neval a window.__samples out=samples.json\n"
        );
        assert_eq!(format_dsl(&formatted).unwrap(), formatted);
    }

    #[test]
    fn rejects_an_empty_eval_out() {
        let text = "peer a url=x viewport=390x844\neval a window.__samples out=\"\"\n";
        let error = parse_dsl(text).unwrap().validate().unwrap_err();
        assert!(
            error.to_string().contains("out must not be empty"),
            "{error}"
        );
    }

    #[test]
    fn parses_viewports() {
        assert_eq!(parse_viewport("390x844").unwrap(), (390, 844));
        assert!(parse_viewport("390").is_err());
        assert!(parse_viewport("0x844").is_err());
    }

    /// Starting a process and waiting for it are separate steps, so a scenario
    /// can spawn mid-run and can wait on an origin it does not own.
    #[test]
    fn parses_spawn_and_wait_for() {
        let text = concat!(
            "peer a url=x viewport=390x844\n",
            "spawn game command=\"sh -c serve\" port=8787 dir=/tmp env=\"A=1,B=2\"\n",
            "wait_for ready=game timeout=120s\n",
            "wait_for port=9000 host=127.0.0.1\n",
            "wait_for http=\"http://127.0.0.1:8787/health\" status=204\n",
            "wait_for log=game pattern=\"listening\"\n",
            "wait_for exit=game code=3\n",
        );
        let scenario = parse_dsl(text).unwrap();
        scenario.validate().unwrap();
        assert_eq!(scenario.step.len(), 6);
        assert_eq!(scenario.step[0].action_name(), "spawn");
        assert_eq!(scenario.step[1].action_name(), "wait_for");
        match &scenario.step[0] {
            Step::Spawn {
                name,
                command,
                port,
                dir,
                env,
            } => {
                assert_eq!(name, "game");
                assert_eq!(command, "sh -c serve");
                assert_eq!(*port, Some(8787));
                assert_eq!(dir.as_deref(), Some("/tmp"));
                assert_eq!(
                    env.as_deref(),
                    Some(&["A=1".to_owned(), "B=2".to_owned()][..])
                );
            }
            other => panic!("expected a spawn step, got {other:?}"),
        }
        match &scenario.step[1] {
            Step::WaitFor {
                ready, timeout_ms, ..
            } => {
                assert_eq!(ready.as_deref(), Some("game"));
                assert_eq!(*timeout_ms, Some(120_000));
            }
            other => panic!("expected a wait_for step, got {other:?}"),
        }
    }

    #[test]
    fn formats_spawn_and_wait_for_in_field_order() {
        let text =
            "spawn game env=\"A=1\" port=8787 command=serve\nwait_for timeout=5s ready=game\n";
        let formatted = format_dsl(text).unwrap();
        assert_eq!(
            formatted,
            "spawn game command=serve port=8787 env=\"A=1\"\nwait_for ready=game timeout=5s\n"
        );
        assert_eq!(format_dsl(&formatted).unwrap(), formatted);
    }

    #[test]
    fn rejects_wait_for_with_more_than_one_variant() {
        let text =
            "peer a url=x viewport=390x844\nwait_for port=8787 http=\"http://127.0.0.1:8787/\"\n";
        let error = parse_dsl(text).unwrap().validate().unwrap_err();
        assert!(error.to_string().contains("exactly one of"), "{error}");
    }

    /// `ready=` is sugar over the port the spawn declared. Without one there is
    /// nothing to wait on, and the message has to say which field to use instead.
    #[test]
    fn rejects_ready_for_a_spawn_with_no_port() {
        let text = "peer a url=x viewport=390x844\nspawn game command=serve\nwait_for ready=game\n";
        let error = parse_dsl(text).unwrap().validate().unwrap_err();
        assert!(error.to_string().contains("declares no port"), "{error}");
    }

    /// A wait can only name a process started before it; otherwise it could never
    /// succeed, and the typo would surface as a timeout an hour later.
    #[test]
    fn rejects_waiting_on_a_process_that_has_not_been_spawned_yet() {
        let text = "peer a url=x viewport=390x844\nwait_for exit=job\nspawn job command=true\n";
        let error = parse_dsl(text).unwrap().validate().unwrap_err();
        assert!(error.to_string().contains("no spawn named"), "{error}");
    }

    #[test]
    fn rejects_a_spawn_sharing_a_peers_name() {
        let text = "peer game url=x viewport=390x844\nspawn game command=serve\n";
        let error = parse_dsl(text).unwrap().validate().unwrap_err();
        assert!(error.to_string().contains("is also a peer name"), "{error}");
    }

    #[test]
    fn rejects_https_because_the_tool_carries_no_tls() {
        let text = "peer a url=x viewport=390x844\nwait_for http=\"https://example.com/\"\n";
        let error = parse_dsl(text).unwrap().validate().unwrap_err();
        assert!(error.to_string().contains("https needs TLS"), "{error}");
    }

    #[test]
    fn rejects_a_log_wait_with_no_pattern() {
        let text = "peer a url=x viewport=390x844\nspawn game command=serve\nwait_for log=game\n";
        let error = parse_dsl(text).unwrap().validate().unwrap_err();
        assert!(error.to_string().contains("log needs a pattern"), "{error}");
    }

    /// Peers load `url=` before any step, so a page served by a process the
    /// scenario started needs a navigation of its own.
    #[test]
    fn parses_and_formats_a_goto() {
        let text = "peer a url=x viewport=390x844\ngoto a \"http://127.0.0.1:8787/?world=t\"\n";
        let scenario = parse_dsl(text).unwrap();
        scenario.validate().unwrap();
        match &scenario.step[0] {
            Step::Goto { peer, url } => {
                assert_eq!(peer, "a");
                assert_eq!(url, "http://127.0.0.1:8787/?world=t");
            }
            other => panic!("expected a goto step, got {other:?}"),
        }
        assert_eq!(format_dsl(text).unwrap(), text);
    }
}
