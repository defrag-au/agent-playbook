//! What the device showed and said, and the problems worth naming.
//!
//! The image is the first check; this is the second, for the things a glance
//! at a phone misses: a launch that never reached `Status: ok`, no `SAPP` line
//! at all, a crash buffer that is not empty, an expected log line that never
//! arrived. The same shape as `browser-shot`'s report: a sidecar beside the
//! capture, not a claim in place of one.

use crate::adb::Device;
use std::path::Path;

pub struct Report<'a> {
    pub device: &'a Device,
    pub package: &'a str,
    pub apk: &'a Path,
    pub component: &'a str,
    pub launch: &'a str,
    pub logs: &'a str,
    pub crash: &'a str,
    pub shot: Option<&'a Path>,
    pub expected: Option<&'a str>,
    pub foreground: bool,
}

impl Report<'_> {
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if !self.launch.contains("Status: ok") {
            problems.push("the launcher did not report `Status: ok`".to_string());
        }
        if !self.foreground {
            problems.push(
                "the activity is not the top resumed activity — the device may be locked"
                    .to_string(),
            );
        }
        if !self.logs.lines().any(|line| line.contains("SAPP")) {
            problems.push("no SAPP output — the activity may not have drawn".to_string());
        }
        if !self.crash.trim().is_empty() {
            problems.push("the crash buffer is not empty".to_string());
        }
        if let Some(expected) = self.expected {
            if !self.logs.contains(expected) {
                problems.push(format!(
                    "expected `{expected}` in the logs, and it is not there"
                ));
            }
        }
        problems
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("device  {}\n", self.device.label()));
        out.push_str(&format!("package {}\n", self.package));
        out.push_str(&format!("apk     {}\n", self.apk.display()));
        out.push_str(&format!("launch  {}\n", self.component));
        for line in self.launch.lines() {
            out.push_str(&format!("  {line}\n"));
        }
        out.push_str(&format!(
            "foreground {}\n",
            if self.foreground { "yes" } else { "no" }
        ));
        section(&mut out, "logs", self.logs);
        section(&mut out, "crash", self.crash);
        out.push_str("\nproblems\n");
        let problems = self.problems();
        if problems.is_empty() {
            out.push_str("  none\n");
        } else {
            for problem in problems {
                out.push_str(&format!("  - {problem}\n"));
            }
        }
        if let Some(shot) = self.shot {
            out.push_str(&format!("\nshot    {}\n", shot.display()));
        }
        out
    }
}

fn section(out: &mut String, name: &str, body: &str) {
    out.push_str(&format!("\n{name}\n"));
    if body.trim().is_empty() {
        out.push_str("  (empty)\n");
    } else {
        for line in body.lines() {
            out.push_str(&format!("  {line}\n"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn device() -> Device {
        Device {
            serial: "56301FDCH006PU".into(),
            state: "device".into(),
            model: Some("Pixel_10_Pro".into()),
        }
    }

    fn report<'a>(device: &'a Device, apk: &'a Path, logs: &'a str, crash: &'a str) -> Report<'a> {
        Report {
            device,
            package: "au.defrag.example",
            apk,
            component: "au.defrag.example/.MainActivity",
            launch: "Status: ok\nLaunchState: COLD\nTotalTime: 122",
            logs,
            crash,
            shot: None,
            expected: None,
            foreground: true,
        }
    }

    #[test]
    fn a_clean_run_has_no_problems_and_names_the_device() {
        let device = device();
        let apk = PathBuf::from("/w/x.apk");
        let text = report(&device, &apk, "I SAPP: surfaceCreated", "").render();
        assert!(text.contains("56301FDCH006PU (Pixel_10_Pro)"), "{text}");
        assert!(text.contains("problems\n  none"), "{text}");
        assert!(text.contains("Status: ok"), "{text}");
    }

    #[test]
    fn a_crash_and_missing_sapp_are_problems() {
        let device = device();
        let apk = PathBuf::from("/w/x.apk");
        let text = report(&device, &apk, "", "FATAL EXCEPTION").render();
        assert!(text.contains("no SAPP output"), "{text}");
        assert!(text.contains("crash buffer is not empty"), "{text}");
    }

    #[test]
    fn an_expected_line_is_checked() {
        let device = device();
        let apk = PathBuf::from("/w/x.apk");
        let mut report = report(&device, &apk, "I SAPP: cycle 0: 217 scenes", "");
        report.expected = Some("cycle 0");
        assert!(report.problems().is_empty());
        report.expected = Some("cycle 1");
        assert!(report.problems()[0].contains("cycle 1"));
    }

    #[test]
    fn a_missing_launch_status_is_a_problem() {
        let device = device();
        let apk = PathBuf::from("/w/x.apk");
        let mut report = report(&device, &apk, "I SAPP: surfaceCreated", "");
        report.launch = "Error: Activity not started";
        assert!(report.problems()[0].contains("Status: ok"));
    }

    #[test]
    fn an_app_that_is_not_resumed_is_a_problem() {
        let device = device();
        let apk = PathBuf::from("/w/x.apk");
        let mut report = report(&device, &apk, "I SAPP: surfaceCreated", "");
        report.foreground = false;
        assert!(report.problems()[0].contains("resumed"));
    }
}
