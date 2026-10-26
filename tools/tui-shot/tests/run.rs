//! Drive the demo TUI through the real binary.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

fn workspace() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::canonicalize(manifest.parent().unwrap().parent().unwrap()).unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("tui-shot-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn write_scenario(directory: &Path, name: &str, text: &str) -> PathBuf {
    let path = directory.join(name);
    fs::write(&path, text).unwrap();
    path
}

fn run(scenario: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_tui-shot"))
        .current_dir(workspace())
        .args(["run", "--scenario"])
        .arg(scenario)
        .output()
        .unwrap()
}

#[derive(Deserialize)]
struct RunJson {
    status: String,
    tuis: Vec<TuiJson>,
    captures: Vec<CaptureRefJson>,
    assertions: Vec<AssertionJson>,
}

#[derive(Deserialize)]
struct TuiJson {
    exit: Option<ExitJson>,
}

#[derive(Deserialize)]
struct ExitJson {
    code: Option<i32>,
}

#[derive(Deserialize)]
struct CaptureRefJson {
    stem: String,
    text: String,
    json: String,
}

#[derive(Deserialize)]
struct AssertionJson {
    ok: bool,
}

#[derive(Deserialize)]
struct CaptureJson {
    size: SizeJson,
    screen: ScreenJson,
    styles: Vec<StyleJson>,
    warnings: Vec<WarningJson>,
}

#[derive(Deserialize)]
struct SizeJson {
    cols: u16,
    rows: u16,
}

#[derive(Deserialize)]
struct ScreenJson {
    alternate: bool,
    rows_used: usize,
}

#[derive(Deserialize)]
struct StyleJson {
    inverse: bool,
}

#[derive(Deserialize)]
struct WarningJson {
    kind: String,
}

fn capture_report(path: &Path) -> CaptureJson {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn drives_the_demo_and_writes_the_screen_and_report() {
    let directory = scratch("drive");
    let artifacts = format!("target/tmp/tui-shot-drive-{}/artifacts", std::process::id());
    let scenario = write_scenario(
        &directory,
        "demo.dsl",
        &format!(
            "\
run artifacts={artifacts} settle=40ms
tui demo bin=tui-shot-demo size=60x16

wait demo contains=\"tui-shot demo\"
capture demo out=01-start

key demo 1
wait demo contains=\"deploy\"
capture demo out=02-permission

resize demo 40x10
wait demo idle=80ms
capture demo out=03-narrow
assert demo contains=\"draft\"

key demo q
wait demo exit=true
"
        ),
    );

    let output = run(&scenario);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.code() == Some(0),
        "status {:?}\nstderr:\n{stderr}\nstdout:\n{stdout}",
        output.status
    );

    let artifacts = workspace().join(artifacts);
    let run_json: RunJson =
        serde_json::from_str(&fs::read_to_string(artifacts.join("run.json")).unwrap()).unwrap();
    assert_eq!(run_json.status, "pass");
    assert_eq!(run_json.tuis.len(), 1);
    assert_eq!(run_json.tuis[0].exit.as_ref().unwrap().code, Some(0));
    assert_eq!(run_json.captures.len(), 3);
    assert_eq!(run_json.assertions.len(), 1);
    assert!(run_json.assertions[0].ok);

    let start = fs::read_to_string(artifacts.join("01-start.txt")).unwrap();
    assert!(start.contains("hello from tui-shot"), "{start}");
    let start_json = capture_report(&artifacts.join("01-start.json"));
    assert_eq!(start_json.size.cols, 60);
    assert_eq!(start_json.size.rows, 16);
    assert!(start_json.screen.alternate);
    assert!(start_json.screen.rows_used > 3);

    let permission = fs::read_to_string(artifacts.join("02-permission.txt")).unwrap();
    assert!(permission.contains("deploy"), "{permission}");
    let permission_json = capture_report(&artifacts.join("02-permission.json"));
    assert!(
        permission_json.styles.iter().any(|style| style.inverse),
        "expected an inverse run for the permission row"
    );

    let narrow = capture_report(&artifacts.join("03-narrow.json"));
    assert_eq!(narrow.size.cols, 40);
    assert_eq!(narrow.size.rows, 10);
    assert!(narrow
        .warnings
        .iter()
        .all(|warning| warning.kind != "blank_screen"));

    let run_text = fs::read_to_string(artifacts.join("run.txt")).unwrap();
    assert!(run_text.contains("result     pass"), "{run_text}");
    for capture in &run_json.captures {
        assert!(artifacts.join(&capture.text).is_file(), "{}", capture.text);
        assert!(artifacts.join(&capture.json).is_file(), "{}", capture.json);
        assert!(!capture.stem.is_empty());
    }
}

#[test]
fn a_failed_assertion_writes_the_run_and_exits_two() {
    let directory = scratch("assert");
    let artifacts = format!(
        "target/tmp/tui-shot-assert-{}/artifacts",
        std::process::id()
    );
    let scenario = write_scenario(
        &directory,
        "assert.dsl",
        &format!(
            "\
run artifacts={artifacts} settle=40ms
tui demo bin=tui-shot-demo size=60x16
wait demo contains=\"tui-shot demo\"
assert demo contains=\"a string that is not there\" message=\"the demo is alive\"
key demo q
wait demo exit=true
"
        ),
    );

    let output = run(&scenario);
    assert_eq!(output.status.code(), Some(2));
    let artifacts = workspace().join(artifacts);
    let run_json: RunJson =
        serde_json::from_str(&fs::read_to_string(artifacts.join("run.json")).unwrap()).unwrap();
    assert_eq!(run_json.status, "fail");
    assert_eq!(run_json.assertions.len(), 1);
    assert!(!run_json.assertions[0].ok);
    let run_text = fs::read_to_string(artifacts.join("run.txt")).unwrap();
    assert!(run_text.contains("FAILED"), "{run_text}");
    assert!(run_text.contains("the demo is alive"), "{run_text}");
}

#[test]
fn a_wait_that_times_out_writes_the_screen_it_saw() {
    let directory = scratch("timeout");
    let artifacts = format!(
        "target/tmp/tui-shot-timeout-{}/artifacts",
        std::process::id()
    );
    let scenario = write_scenario(
        &directory,
        "timeout.dsl",
        &format!(
            "\
run artifacts={artifacts} settle=20ms
tui demo bin=tui-shot-demo size=60x16
wait demo contains=\"tui-shot demo\" timeout=2s
wait demo contains=\"this never appears\" timeout=250ms
"
        ),
    );

    let output = run(&scenario);
    assert_eq!(output.status.code(), Some(2));
    let artifacts = workspace().join(artifacts);
    let run_text = fs::read_to_string(artifacts.join("run.txt")).unwrap();
    assert!(run_text.contains("this never appears"), "{run_text}");
    let failure = artifacts.join("demo-failure-4.txt");
    assert!(failure.is_file(), "expected {}", failure.display());
    let screen = fs::read_to_string(&failure).unwrap();
    assert!(screen.contains("hello from tui-shot"), "{screen}");
}

#[test]
fn check_reports_the_built_target_without_running_it() {
    let directory = scratch("check");
    let scenario = write_scenario(
        &directory,
        "check.dsl",
        "\
tui demo bin=tui-shot-demo size=60x16
wait demo contains=\"tui-shot demo\"
capture demo out=01
",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_tui-shot"))
        .current_dir(workspace())
        .args(["check", "--scenario"])
        .arg(&scenario)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.code() == Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: CheckJson = serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).unwrap();
    assert!(json.valid);
    assert_eq!(json.tuis.len(), 1);
    assert!(json.tuis[0]
        .path
        .as_deref()
        .unwrap()
        .contains("tui-shot-demo"));
    assert_eq!(json.steps.len(), 2);
    assert_eq!(json.steps[0].action, "wait");
    assert_eq!(json.captures, ["01"]);
}

#[test]
fn single_shot_preserves_the_order_of_waits_and_keys() {
    let out = format!("target/tmp/tui-shot-single-{}/shot", std::process::id());
    let output = Command::new(env!("CARGO_BIN_EXE_tui-shot"))
        .current_dir(workspace())
        .args([
            "tui-shot-demo",
            "60x16",
            &out,
            "--wait-for",
            "tui-shot demo",
            "--key",
            "1",
            "--wait-for",
            "deploy",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.code() == Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let screen = workspace().join(format!("{out}.txt"));
    let report = workspace().join(format!("{out}.json"));
    assert!(screen.is_file(), "expected {}", screen.display());
    assert!(report.is_file(), "expected {}", report.display());
    let text = fs::read_to_string(&screen).unwrap();
    assert!(text.contains("deploy"), "{text}");
    assert!(text.contains("hello from tui-shot"), "{text}");
}

#[derive(Deserialize)]
struct CheckJson {
    valid: bool,
    tuis: Vec<CheckTuiJson>,
    steps: Vec<CheckStepJson>,
    captures: Vec<String>,
}

#[derive(Deserialize)]
struct CheckTuiJson {
    path: Option<String>,
}

#[derive(Deserialize)]
struct CheckStepJson {
    action: String,
}
