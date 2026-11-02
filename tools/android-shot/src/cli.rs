//! The command line: what to build, where to send it, and what to read back.

use std::path::{Path, PathBuf};

pub const USAGE: &str = "\
usage: android-shot --package <android.package>
                    (--apk <file.apk> | --library <lib> --crate <cargo-package>)
                    [--manifest <path>] [--icon <png> --banner <png> | --no-art]
                    [--abi <abi>]... [--out <file.apk>]
                    [--device <serial>] [--activity <.MainActivity>]
                    [--at <seconds>] [--expect <substring>]
                    [--shot <file.png>] [--mq-apk <path>] [--dry-run]

  --package   the app's Android package, as the APK names it
  --apk       install this APK instead of building one
  --library   passed to mq-apk: lib<library>.so, which MainActivity loads
  --crate     passed to mq-apk: the cargo package with the app's library target
  --manifest  passed to mq-apk (defaults to <crate>/android/AndroidManifest.xml)
  --icon      passed to mq-apk: the launcher icon
  --banner    passed to mq-apk: the TV banner
  --no-art    passed to mq-apk: package no resources
  --abi       passed to mq-apk: arm64-v8a or armeabi-v7a (repeatable)
  --out       where mq-apk writes the APK (defaults to .tmp/android/<package>.apk)
  --device    the adb serial; required when more than one device is attached
  --activity  the activity to start (defaults to .MainActivity)
  --at        seconds to let the app run before the logs are read (default 3)
  --expect    exit 2 unless this text appears in the app's logs
  --shot      capture the device screen to this PNG; the report is <shot>.txt
  --mq-apk    the packager binary; defaults to mq-apk on PATH, else
              `cargo run -q -p mq-apk --` in this workspace
  --dry-run   print the packager and adb commands; run nothing";

#[derive(Debug)]
pub struct Options {
    pub package: String,
    pub apk: Option<PathBuf>,
    pub library: Option<String>,
    pub krate: Option<String>,
    pub manifest: Option<PathBuf>,
    pub icon: Option<PathBuf>,
    pub banner: Option<PathBuf>,
    pub no_art: bool,
    pub abis: Vec<String>,
    pub out: Option<PathBuf>,
    pub device: Option<String>,
    pub activity: String,
    pub at: f64,
    pub expect: Option<String>,
    pub shot: Option<PathBuf>,
    pub mq_apk: Option<PathBuf>,
    pub dry_run: bool,
    pub cwd: PathBuf,
}

impl Options {
    /// The APK to install: the one given, the one `mq-apk` will write, or the
    /// tool's own scratch path. Everything lands under `.tmp/` unless told
    /// otherwise, so an experiment leaves the repository clean.
    pub fn apk_path(&self) -> PathBuf {
        if let Some(apk) = &self.apk {
            return absolute(&self.cwd, apk);
        }
        if let Some(out) = &self.out {
            return absolute(&self.cwd, out);
        }
        self.cwd
            .join(".tmp/android")
            .join(format!("{}.apk", self.package))
    }

    pub fn shot_path(&self) -> Option<PathBuf> {
        self.shot.as_ref().map(|path| absolute(&self.cwd, path))
    }

    /// What `am start -n` takes. A full `package/class` is used as given; a
    /// class name (`.MainActivity` by default) is resolved against the package.
    pub fn activity_component(&self) -> String {
        if self.activity.contains('/') {
            self.activity.clone()
        } else {
            format!("{}/{}", self.package, self.activity)
        }
    }
}

pub fn parse(args: &[String], cwd: PathBuf) -> Result<Options, String> {
    let mut package = None;
    let mut apk = None;
    let (mut library, mut krate) = (None, None);
    let (mut manifest, mut icon, mut banner, mut out) = (None, None, None, None);
    let mut no_art = false;
    let mut abis = Vec::new();
    let mut device = None;
    let mut activity = ".MainActivity".to_string();
    let mut at: f64 = 3.0;
    let mut expect = None;
    let mut shot = None;
    let mut mq_apk = None;
    let mut dry_run = false;

    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let mut value = || {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--package" => package = Some(value()?),
            "--apk" => apk = Some(PathBuf::from(value()?)),
            "--library" => library = Some(value()?),
            "--crate" => krate = Some(value()?),
            "--manifest" => manifest = Some(PathBuf::from(value()?)),
            "--icon" => icon = Some(PathBuf::from(value()?)),
            "--banner" => banner = Some(PathBuf::from(value()?)),
            "--no-art" => no_art = true,
            "--abi" => abis.push(value()?),
            "--out" => out = Some(PathBuf::from(value()?)),
            "--device" => device = Some(value()?),
            "--activity" => activity = value()?,
            "--at" => {
                let seconds = value()?;
                at = seconds
                    .parse()
                    .map_err(|_| format!("--at needs seconds, not `{seconds}`"))?;
            }
            "--expect" => expect = Some(value()?),
            "--shot" => shot = Some(PathBuf::from(value()?)),
            "--mq-apk" => mq_apk = Some(PathBuf::from(value()?)),
            "--dry-run" => dry_run = true,
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown argument {other}")),
        }
    }

    let package = package.ok_or("--package is required")?;
    validate_package(&package)?;
    if !at.is_finite() || at < 0.0 {
        return Err(format!(
            "--at must be a non-negative number of seconds, not `{at}`"
        ));
    }
    if activity.is_empty() {
        return Err("--activity cannot be empty".to_string());
    }

    if apk.is_some() {
        let build_flag = if library.is_some() {
            Some("--library")
        } else if krate.is_some() {
            Some("--crate")
        } else if manifest.is_some() {
            Some("--manifest")
        } else if icon.is_some() {
            Some("--icon")
        } else if banner.is_some() {
            Some("--banner")
        } else if no_art {
            Some("--no-art")
        } else if !abis.is_empty() {
            Some("--abi")
        } else if out.is_some() {
            Some("--out")
        } else {
            None
        };
        if let Some(flag) = build_flag {
            return Err(format!(
                "{flag} builds an APK, but --apk installs one as-is — drop one of them"
            ));
        }
    } else {
        if library.is_none() {
            return Err("--library is required unless --apk is given".to_string());
        }
        if krate.is_none() {
            return Err("--crate is required unless --apk is given".to_string());
        }
    }

    Ok(Options {
        package,
        apk,
        library,
        krate,
        manifest,
        icon,
        banner,
        no_art,
        abis,
        out,
        device,
        activity,
        at,
        expect,
        shot,
        mq_apk,
        dry_run,
        cwd,
    })
}

/// A Java package name: dot-separated identifiers. The same rule the packager
/// applies, because this value also forms the activity component.
fn validate_package(package: &str) -> Result<(), String> {
    let identifier = |segment: &str| {
        let mut chars = segment.chars();
        chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    if !package.is_empty() && package.split('.').all(identifier) {
        Ok(())
    } else {
        Err(format!(
            "--package `{package}` is not a Java package name (dot-separated letters, digits and `_`, not starting with a digit)"
        ))
    }
}

fn absolute(cwd: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|a| a.to_string()).collect()
    }

    fn build_args() -> Vec<String> {
        args(&[
            "--package",
            "au.defrag.example",
            "--library",
            "example_mq",
            "--crate",
            "example-mq",
            "--no-art",
        ])
    }

    #[test]
    fn a_build_run_defaults_to_a_scratch_apk_and_the_miniquad_activity() {
        let opts = parse(&build_args(), PathBuf::from("/w")).expect("parses");
        assert_eq!(
            opts.apk_path(),
            PathBuf::from("/w/.tmp/android/au.defrag.example.apk")
        );
        assert_eq!(opts.activity_component(), "au.defrag.example/.MainActivity");
        assert_eq!(opts.at, 3.0);
        assert!(opts.shot_path().is_none());
    }

    #[test]
    fn an_apk_run_takes_no_build_flags() {
        let list = args(&[
            "--package",
            "au.defrag.example",
            "--apk",
            "x.apk",
            "--library",
            "example_mq",
        ]);
        let err = parse(&list, PathBuf::new()).expect_err("conflict");
        assert!(err.contains("--library"), "{err}");
        assert!(err.contains("--apk"), "{err}");
    }

    #[test]
    fn building_needs_the_library_and_the_crate() {
        let err = parse(&args(&["--package", "a.b"]), PathBuf::new()).expect_err("no library");
        assert_eq!(err, "--library is required unless --apk is given");
        let err = parse(
            &args(&["--package", "a.b", "--library", "l"]),
            PathBuf::new(),
        )
        .expect_err("no crate");
        assert_eq!(err, "--crate is required unless --apk is given");
    }

    #[test]
    fn a_full_activity_component_is_left_alone() {
        let mut list = build_args();
        list.extend(args(&["--activity", "au.defrag.other/.MainActivity"]));
        let opts = parse(&list, PathBuf::new()).expect("parses");
        assert_eq!(opts.activity_component(), "au.defrag.other/.MainActivity");
    }

    #[test]
    fn at_must_be_a_number() {
        let mut list = build_args();
        list.extend(args(&["--at", "soon"]));
        assert!(parse(&list, PathBuf::new()).is_err());
        let mut list = build_args();
        list.extend(args(&["--at", "-1"]));
        assert!(parse(&list, PathBuf::new()).is_err());
    }

    #[test]
    fn a_package_name_cannot_escape_the_scratch_path() {
        for bad in ["", "a..b", ".a", "a.", "a/b", "../a", "a.b-c", "1a.b"] {
            let mut list = build_args();
            list[1] = bad.to_string();
            assert!(parse(&list, PathBuf::new()).is_err(), "{bad} parsed");
        }
    }

    #[test]
    fn a_shot_path_is_resolved_against_the_working_directory() {
        let mut list = build_args();
        list.extend(args(&["--shot", ".tmp/phone.png"]));
        let opts = parse(&list, PathBuf::from("/w")).expect("parses");
        assert_eq!(opts.shot_path(), Some(PathBuf::from("/w/.tmp/phone.png")));
    }

    #[test]
    fn help_is_an_empty_error_so_main_prints_usage() {
        assert_eq!(parse(&args(&["--help"]), PathBuf::new()).unwrap_err(), "");
    }
}
