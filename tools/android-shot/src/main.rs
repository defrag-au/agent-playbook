//! `android-shot` — put a macroquad app on an Android device and bring back
//! what it showed and said.
//!
//! The device half of the loop whose build half is shared-crates' `mq-apk`.
//! The split is deliberate: `mq-apk` produces an APK and never needs a phone
//! (CI, a release build); this owns the process that talks to one, the way
//! `browser-shot` owns a browser and `tui-shot` owns a terminal.
//!
//! Build, install, launch, wait, read `SAPP`/`AndroidRuntime` and the crash
//! buffer, optionally capture the device screen, and write a report. Exit 0
//! when the app ran cleanly, 2 when it ran but the report has problems (a
//! crash, no `SAPP`, a missing `--expect` line), 1 when nothing could run.

mod adb;
mod cli;
mod packager;
mod report;

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread::sleep;
use std::time::Duration;

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("✗ {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let opts = match cli::parse(&args, cwd) {
        Ok(opts) => opts,
        Err(e) if e.is_empty() => {
            println!("{}", cli::USAGE);
            return Ok(ExitCode::SUCCESS);
        }
        Err(e) => return Err(e),
    };

    if opts.dry_run {
        print_plan(&opts)?;
        return Ok(ExitCode::SUCCESS);
    }

    let apk = opts.apk_path();
    if opts.apk.is_some() {
        if !apk.is_file() {
            return Err(format!("no APK at {}", apk.display()));
        }
    } else {
        packager::build(&opts, &apk)?;
    }

    let devices = adb::devices()?;
    let device = adb::select(&devices, opts.device.as_deref())?;
    println!("→ device {}", device.label());

    let adb = adb::Adb::new(&device.serial);
    adb.clear_logs()?;
    println!("→ install {}", apk.display());
    adb.install(&apk)?;
    adb.force_stop(&opts.package)?;

    // The build outlasts the screen timeout on a phone left on a desk.
    adb.wake()?;
    let component = opts.activity_component();
    println!("→ start {component}");
    let launch = adb.launch(&component)?;
    if let Some(status) = launch.lines().find(|line| line.starts_with("Status:")) {
        println!("  {status}");
    }

    sleep(Duration::from_secs_f64(opts.at));
    let foreground = adb.foreground(&component)?;
    let logs = adb.logs()?;
    let crash = adb.crash()?;
    let shot = match opts.shot_path() {
        Some(path) => {
            adb.screenshot(&path)?;
            Some(path)
        }
        None => None,
    };

    let report = report::Report {
        device: &device,
        package: &opts.package,
        apk: &apk,
        component: &component,
        launch: &launch,
        logs: &logs,
        crash: &crash,
        shot: shot.as_deref(),
        expected: opts.expect.as_deref(),
        foreground,
    };
    let problems = report.problems();
    let text = report.render();

    if let Some(shot) = &shot {
        let sidecar = shot.with_extension("txt");
        fs::write(&sidecar, &text).map_err(|e| format!("write {}: {e}", sidecar.display()))?;
        println!("✓ {} and {}", shot.display(), sidecar.display());
    } else {
        println!();
        print!("{text}");
    }

    if problems.is_empty() {
        Ok(ExitCode::SUCCESS)
    } else {
        for problem in &problems {
            eprintln!("✗ {problem}");
        }
        Ok(ExitCode::from(2))
    }
}

/// What the run will do, without doing any of it. No adb query either: with no
/// `--device` the commands show the plain `adb` that selects the only one.
fn print_plan(opts: &cli::Options) -> Result<(), String> {
    let apk = opts.apk_path();
    if let Some(apk) = &opts.apk {
        println!("apk      {}", apk.display());
    } else {
        let packager = packager::Packager::choose(opts.mq_apk.as_deref());
        let (program, mut args) = packager.command();
        args.extend(packager::build_args(opts, &apk)?);
        println!("build    {}", packager::command_line(&program, &args));
    }

    match &opts.device {
        Some(serial) => println!("device   {serial}"),
        None => println!("device   (the only authorized device)"),
    }
    let adb = match &opts.device {
        Some(serial) => format!("adb -s {serial}"),
        None => "adb".to_string(),
    };
    println!(
        "install  {adb} install -r {}",
        packager::quote(&apk.display().to_string())
    );
    println!("stop     {adb} shell am force-stop {}", opts.package);
    println!("wake     {adb} shell input keyevent KEYCODE_WAKEUP");
    println!("keyguard {adb} shell wm dismiss-keyguard");
    println!(
        "launch   {adb} shell am start -W -n {}",
        opts.activity_component()
    );
    println!("logs     {adb} logcat -d -s SAPP AndroidRuntime");
    println!("crash    {adb} logcat -d -b crash");
    if let Some(shot) = opts.shot_path() {
        println!(
            "shot     {adb} exec-out screencap -p > {}",
            packager::quote(&shot.display().to_string())
        );
    }
    if let Some(expected) = &opts.expect {
        println!("expect   `{expected}` in the logs");
    }
    println!();
    println!("dry run — nothing was built, installed or launched");
    Ok(())
}
