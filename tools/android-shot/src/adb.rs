//! The device half: find one, install, launch, and read back the screen and
//! the logs.
//!
//! `adb` is the process this tool owns. It is not bundled — the version that
//! talks to the device is the one the android devshell pins, not a second copy
//! inside a Nix package — so a missing `adb` is an instruction ("run inside
//! the android devshell"), not a mystery.

use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub serial: String,
    /// `device` when authorized; `unauthorized`, `offline` and `no permissions`
    /// are the states worth naming rather than hiding.
    pub state: String,
    pub model: Option<String>,
}

impl Device {
    pub fn label(&self) -> String {
        match &self.model {
            Some(model) => format!("{} ({model})", self.serial),
            None => self.serial.clone(),
        }
    }
}

/// Every device `adb` reports, in its order.
pub fn devices() -> Result<Vec<Device>, String> {
    let out = Command::new("adb")
        .args(["devices", "-l"])
        .output()
        .map_err(|e| format!("run adb: {e} — is adb on PATH? run inside the android devshell"))?;
    if !out.status.success() {
        return Err(format!(
            "adb devices failed:\n{}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(parse(&String::from_utf8_lossy(&out.stdout)))
}

/// Read `adb devices -l`. Lines before the header (the daemon starting) and
/// blank lines are not devices.
pub fn parse(output: &str) -> Vec<Device> {
    output
        .lines()
        .skip_while(|line| !line.starts_with("List of devices"))
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let serial = parts.next()?;
            let state = parts.next()?;
            let model = line
                .split_whitespace()
                .find_map(|part| part.strip_prefix("model:").map(str::to_string));
            Some(Device {
                serial: serial.to_string(),
                state: state.to_string(),
                model,
            })
        })
        .collect()
}

/// Pick the device to use: the named one, or the only authorized one.
pub fn select(devices: &[Device], wanted: Option<&str>) -> Result<Device, String> {
    if let Some(serial) = wanted {
        let device = devices
            .iter()
            .find(|device| device.serial == serial)
            .ok_or_else(|| {
                format!(
                    "no device with serial `{serial}`; attached: {}",
                    listing(devices)
                )
            })?;
        if device.state != "device" {
            return Err(format!(
                "device `{serial}` is {} — accept the USB debugging prompt on the phone",
                device.state
            ));
        }
        return Ok(device.clone());
    }

    let ready: Vec<&Device> = devices
        .iter()
        .filter(|device| device.state == "device")
        .collect();
    match ready.as_slice() {
        [only] => Ok((*only).clone()),
        [] if devices.is_empty() => Err(
            "no Android device attached — connect one and accept the USB debugging prompt"
                .to_string(),
        ),
        [] => Err(format!(
            "no authorized device; attached: {} — accept the USB debugging prompt on the phone",
            listing(devices)
        )),
        many => Err(format!(
            "{} devices are ready ({}); pass --device <serial>",
            many.len(),
            many.iter()
                .map(|device| device.serial.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// `Physical size: 1080x2410` or `Override size: 720x1600`.
fn parse_size(output: &str) -> Option<(u32, u32)> {
    output.lines().find_map(|line| {
        let (_, size) = line.split_once(':')?;
        let (width, height) = size.trim().split_once('x')?;
        Some((width.trim().parse().ok()?, height.trim().parse().ok()?))
    })
}

fn listing(devices: &[Device]) -> String {
    if devices.is_empty() {
        "none".to_string()
    } else {
        devices
            .iter()
            .map(|device| format!("{} ({})", device.serial, device.state))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// One device's adb.
pub struct Adb {
    serial: String,
}

impl Adb {
    pub fn new(serial: &str) -> Self {
        Self {
            serial: serial.to_string(),
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new("adb");
        command.args(["-s", &self.serial]);
        command
    }

    fn run(&self, what: &str, args: &[&str]) -> Result<String, String> {
        let out = self
            .command()
            .args(args)
            .output()
            .map_err(|e| format!("{what}: run adb: {e}"))?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        if !out.status.success() {
            return Err(format!("{what} failed:\n{}", text.trim()));
        }
        Ok(text.trim().to_string())
    }

    /// Clear every buffer, so a crash in the report belongs to this launch.
    pub fn clear_logs(&self) -> Result<(), String> {
        self.run("clear logcat", &["logcat", "-b", "all", "-c"])
            .map(|_| ())
    }

    /// Wake the screen and dismiss a non-secure keyguard. A build can outlast
    /// the screen timeout, and `am start` will happily launch an activity
    /// behind a lock screen: the logs look right and the capture is a clock.
    /// A secure keyguard stays — [`Adb::foreground`] then says so.
    ///
    /// A charging phone with a screensaver set runs a *dream*, which is not a
    /// keyguard: `wm dismiss-keyguard` does not touch it, and miniquad panics
    /// in `eglMakeCurrent` when it is handed a surface behind one. The dream
    /// manager treats a touch as the dismissal, so tap the centre.
    pub fn wake(&self) -> Result<(), String> {
        self.run(
            "wake the screen",
            &["shell", "input", "keyevent", "KEYCODE_WAKEUP"],
        )
        .map(|_| ())?;
        self.run("dismiss the keyguard", &["shell", "wm", "dismiss-keyguard"])
            .map(|_| ())?;
        let power = self.run("read the power state", &["shell", "dumpsys", "power"])?;
        if power.contains("mWakefulness=Dreaming") {
            if let Some((width, height)) = self.screen_size() {
                let x = (width / 2).to_string();
                let y = (height / 2).to_string();
                self.run(
                    "dismiss the screen saver",
                    &["shell", "input", "tap", &x, &y],
                )
                .map(|_| ())?;
            }
        }
        Ok(())
    }

    fn screen_size(&self) -> Option<(u32, u32)> {
        let output = self
            .run("read the screen size", &["shell", "wm", "size"])
            .ok()?;
        parse_size(&output)
    }

    pub fn install(&self, apk: &Path) -> Result<(), String> {
        let apk = apk.display().to_string();
        self.run("install", &["install", "-r", &apk]).map(|_| ())
    }

    /// Stop the package before starting it, so the run is a cold start rather
    /// than `am start` handing the intent to whatever instance was already at
    /// the top of its task.
    pub fn force_stop(&self, package: &str) -> Result<(), String> {
        self.run("stop the app", &["shell", "am", "force-stop", package])
            .map(|_| ())
    }

    /// Start the activity and wait for the launch to settle. The output is
    /// `am start -W`'s status block, which the report keeps.
    pub fn launch(&self, component: &str) -> Result<String, String> {
        self.run("launch", &["shell", "am", "start", "-W", "-n", component])
    }

    /// miniquad logs on Android under `SAPP`; a Java crash is `AndroidRuntime`.
    pub fn logs(&self) -> Result<String, String> {
        self.run(
            "read logcat",
            &["logcat", "-d", "-s", "SAPP", "AndroidRuntime"],
        )
    }

    /// Whether `component` is `topResumedActivity`. The check is the one that
    /// catches a launch that happened behind a lock screen: the process is up
    /// and logging, but what the user (and the capture) sees is not the app.
    pub fn foreground(&self, component: &str) -> Result<bool, String> {
        let dump = self.run(
            "read the resumed activity",
            &["shell", "dumpsys", "activity", "activities"],
        )?;
        Ok(dump
            .lines()
            .any(|line| line.contains("topResumedActivity=") && line.contains(component)))
    }

    pub fn crash(&self) -> Result<String, String> {
        self.run("read the crash buffer", &["logcat", "-d", "-b", "crash"])
    }

    /// The device framebuffer. `exec-out` passes the PNG through untouched;
    /// there is no in-process renderer to ask on the other side of the cable.
    pub fn screenshot(&self, path: &Path) -> Result<(), String> {
        let out = self
            .command()
            .args(["exec-out", "screencap", "-p"])
            .output()
            .map_err(|e| format!("screencap: run adb: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "screencap failed:\n{}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        if !out.stdout.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Err("screencap did not return a PNG".to_string());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        fs::write(path, out.stdout).map_err(|e| format!("write {}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ATTACHED: &str = "\
* daemon not running; starting now at tcp:5037
* daemon started successfully
List of devices attached
56301FDCH006PU         device usb:1-1 product:blazer model:Pixel_10_Pro device:blazer transport_id:1
emulator-5554          unauthorized transport_id:2
192.168.1.20:37737     offline transport_id:3

";

    #[test]
    fn reads_the_serial_state_and_model() {
        let devices = parse(ATTACHED);
        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].serial, "56301FDCH006PU");
        assert_eq!(devices[0].state, "device");
        assert_eq!(devices[0].model.as_deref(), Some("Pixel_10_Pro"));
        assert_eq!(devices[1].state, "unauthorized");
        assert_eq!(devices[1].model, None);
        assert_eq!(devices[2].serial, "192.168.1.20:37737");
        assert_eq!(devices[2].state, "offline");
    }

    #[test]
    fn selects_the_only_authorized_device() {
        let device = select(&parse(ATTACHED), None).expect("one ready");
        assert_eq!(device.serial, "56301FDCH006PU");
    }

    #[test]
    fn refuses_when_nothing_is_authorized_and_names_the_states() {
        let only_unauthorized = "List of devices attached\nabc unauthorized transport_id:1\n";
        let err = select(&parse(only_unauthorized), None).expect_err("unauthorized");
        assert!(err.contains("abc (unauthorized)"), "{err}");
        assert!(err.contains("USB debugging"), "{err}");
    }

    #[test]
    fn refuses_ambiguity_and_names_the_serials() {
        let two = "List of devices attached\none device\ntwo device\n";
        let err = select(&parse(two), None).expect_err("ambiguous");
        assert!(err.contains("one"), "{err}");
        assert!(err.contains("two"), "{err}");
        assert!(err.contains("--device"), "{err}");
    }

    #[test]
    fn a_named_device_is_chosen_even_beside_another() {
        let two = "List of devices attached\none device\ntwo device\n";
        assert_eq!(
            select(&parse(two), Some("two")).expect("named").serial,
            "two"
        );
        assert!(select(&parse(two), Some("three")).is_err());
    }

    #[test]
    fn an_empty_attach_says_to_connect_one() {
        let err = select(&parse("List of devices attached\n"), None).expect_err("none");
        assert!(err.contains("connect one"), "{err}");
    }

    #[test]
    fn reads_the_screen_size() {
        assert_eq!(parse_size("Physical size: 1080x2410\n"), Some((1080, 2410)));
        assert_eq!(parse_size("Override size: 720x1600\n"), Some((720, 1600)));
        assert_eq!(
            parse_size("Physical size: 1080x2410\nOverride size: 720x1600\n"),
            Some((1080, 2410))
        );
        assert_eq!(parse_size("no size here"), None);
    }
}
