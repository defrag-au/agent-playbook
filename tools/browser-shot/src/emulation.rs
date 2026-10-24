//! Device and network emulation shared by the single-shot and scenario paths.

use anyhow::{bail, Context, Result};
use headless_chrome::browser::tab::Tab;
use headless_chrome::protocol::cdp::types::Method;
use headless_chrome::protocol::cdp::{Emulation, Network};

#[derive(Debug, serde::Serialize)]
struct SetTouchEmulation {
    enabled: bool,
    #[serde(rename = "maxTouchPoints", skip_serializing_if = "Option::is_none")]
    max_touch_points: Option<u32>,
}

#[derive(Debug, serde::Deserialize)]
struct Empty {}

// `auto_generate_cdp` 0.4.6 swaps the parameters of `Emulation.setTouchEmulationEnabled` and
// the deprecated `Page.setTouchEmulationEnabled`, so the generated `Emulation` struct carries
// `configuration` instead of `maxTouchPoints`. Send the correct shape ourselves.
impl Method for SetTouchEmulation {
    const NAME: &'static str = "Emulation.setTouchEmulationEnabled";
    type ReturnObject = Empty;
}

#[derive(Debug, Clone, Default)]
pub struct NetworkSpec {
    pub latency_ms: Option<u64>,
    pub download_kbps: Option<u64>,
    pub upload_kbps: Option<u64>,
    pub loss_percent: Option<f64>,
    pub jitter_ms: Option<u64>,
}

impl NetworkSpec {
    pub fn is_set(&self) -> bool {
        self.latency_ms.is_some()
            || self.download_kbps.is_some()
            || self.upload_kbps.is_some()
            || self.loss_percent.is_some()
    }

    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if let Some(latency) = self.latency_ms {
            parts.push(format!("latency {latency} ms"));
        }
        if let Some(download) = self.download_kbps {
            parts.push(format!("down {download} kbps"));
        }
        if let Some(upload) = self.upload_kbps {
            parts.push(format!("up {upload} kbps"));
        }
        if let Some(loss) = self.loss_percent {
            parts.push(format!("loss {loss}%"));
        }
        if parts.is_empty() {
            return "not emulated".to_owned();
        }
        format!("{} — HTTP/WebSocket only, NOT WebRTC", parts.join(", "))
    }
}

pub fn set_metrics(tab: &Tab, width: u32, height: u32, dpr: f64, mobile: bool) -> Result<()> {
    tab.call_method(Emulation::SetDeviceMetricsOverride {
        width,
        height,
        device_scale_factor: dpr,
        mobile,
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
    Ok(())
}

pub fn set_visible_size(tab: &Tab, width: u32, height: u32) -> Result<()> {
    tab.call_method(Emulation::SetVisibleSize { width, height })
        .context("set the visible size")?;
    Ok(())
}

pub fn enable_touch(tab: &Tab) -> Result<()> {
    tab.call_method(SetTouchEmulation {
        enabled: true,
        max_touch_points: Some(5),
    })
    .context("enable touch emulation")?;
    Ok(())
}

pub fn set_user_agent(tab: &Tab, user_agent: &str) -> Result<()> {
    tab.call_method(Emulation::SetUserAgentOverride {
        user_agent: user_agent.to_owned(),
        accept_language: None,
        platform: None,
        user_agent_metadata: None,
    })
    .context("set the user agent")?;
    Ok(())
}

pub fn emulate_network(tab: &Tab, spec: &NetworkSpec) -> Result<()> {
    if spec.jitter_ms.is_some_and(|jitter| jitter > 0) {
        bail!(
            "jitter is not available through Network.emulateNetworkConditions; \
             remove jitter_ms rather than believing it applied"
        );
    }
    if !spec.is_set() {
        return Ok(());
    }

    // CDP measures throughput in bytes/second; -1 disables throttling.
    let throughput = |kbps: Option<u64>| kbps.map_or(-1.0, |kbps| (kbps as f64 * 1000.0) / 8.0);

    tab.call_method(Network::EmulateNetworkConditions {
        offline: false,
        latency: spec.latency_ms.unwrap_or(0) as f64,
        download_throughput: throughput(spec.download_kbps),
        upload_throughput: throughput(spec.upload_kbps),
        connection_Type: None,
        packet_loss: spec.loss_percent,
        packet_queue_length: None,
        packet_reordering: None,
    })
    .context("emulate network conditions")?;
    Ok(())
}
