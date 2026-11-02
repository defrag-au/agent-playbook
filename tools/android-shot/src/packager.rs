//! How the packager is found and invoked.
//!
//! `mq-apk` is a library plus a one-line runner in the app's workspace (the
//! same shape as `mq-bundle`), so it is usually not on PATH. This uses
//! `mq-apk` when it is, and `cargo run -q -p mq-apk --` otherwise; `--mq-apk`
//! names a binary explicitly — a built checkout, or a CI artefact.
//!
//! Nothing about packaging lives here. The build flags below are passed
//! through unchanged, including their validation: `mq-apk` owns the rules for
//! what a manifest, an ABI or an art pair must be.

use crate::cli::Options;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packager {
    Binary(PathBuf),
    /// `cargo run -q -p mq-apk --` in the app's workspace.
    Cargo,
}

impl Packager {
    pub fn choose(explicit: Option<&Path>) -> Self {
        if let Some(path) = explicit {
            return Packager::Binary(path.to_path_buf());
        }
        if on_path("mq-apk") {
            Packager::Binary(PathBuf::from("mq-apk"))
        } else {
            Packager::Cargo
        }
    }

    /// The program and its leading arguments, before the build flags.
    pub fn command(&self) -> (String, Vec<String>) {
        match self {
            Packager::Binary(path) => (path.display().to_string(), Vec::new()),
            Packager::Cargo => (
                "cargo".to_string(),
                vec![
                    "run".to_string(),
                    "-q".to_string(),
                    "-p".to_string(),
                    "mq-apk".to_string(),
                    "--".to_string(),
                ],
            ),
        }
    }
}

pub fn build(opts: &Options, apk: &Path) -> Result<(), String> {
    let packager = Packager::choose(opts.mq_apk.as_deref());
    let (program, mut args) = packager.command();
    args.extend(build_args(opts, apk)?);
    println!("→ {}", command_line(&program, &args));
    let status = Command::new(&program)
        .args(&args)
        .current_dir(&opts.cwd)
        .status()
        .map_err(|e| format!("run {program}: {e}"))?;
    if !status.success() {
        return Err(format!("{program} exited {status}"));
    }
    if !apk.is_file() {
        return Err(format!(
            "{program} reported success but {} is missing",
            apk.display()
        ));
    }
    Ok(())
}

/// The `mq-apk` arguments for this run, without the program.
pub fn build_args(opts: &Options, apk: &Path) -> Result<Vec<String>, String> {
    let library = opts
        .library
        .as_deref()
        .ok_or("--library is required to build an APK")?;
    let krate = opts
        .krate
        .as_deref()
        .ok_or("--crate is required to build an APK")?;

    let mut args = vec![
        "--package".to_string(),
        opts.package.clone(),
        "--library".to_string(),
        library.to_string(),
        "--crate".to_string(),
        krate.to_string(),
        "--out".to_string(),
        apk.display().to_string(),
    ];
    if let Some(manifest) = &opts.manifest {
        args.push("--manifest".to_string());
        args.push(manifest.display().to_string());
    }
    if let Some(icon) = &opts.icon {
        args.push("--icon".to_string());
        args.push(icon.display().to_string());
    }
    if let Some(banner) = &opts.banner {
        args.push("--banner".to_string());
        args.push(banner.display().to_string());
    }
    if opts.no_art {
        args.push("--no-art".to_string());
    }
    for abi in &opts.abis {
        args.push("--abi".to_string());
        args.push(abi.clone());
    }
    Ok(args)
}

pub fn command_line(program: &str, args: &[String]) -> String {
    let mut line = program.to_string();
    for arg in args {
        line.push(' ');
        line.push_str(&quote(arg));
    }
    line
}

/// Enough quoting for a readable dry run: a path with spaces stays one word.
pub fn quote(argument: &str) -> String {
    if argument.is_empty() || argument.contains(' ') {
        format!("'{argument}'")
    } else {
        argument.to_string()
    }
}

fn on_path(program: &str) -> bool {
    if program.contains('/') {
        return Path::new(program).is_file();
    }
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli;

    fn options(list: &[&str]) -> Options {
        let args: Vec<String> = list.iter().map(|arg| arg.to_string()).collect();
        cli::parse(&args, PathBuf::from("/w")).expect("parses")
    }

    #[test]
    fn the_build_flags_are_passed_through_with_an_explicit_output() {
        let opts = options(&[
            "--package",
            "au.defrag.example",
            "--library",
            "example_mq",
            "--crate",
            "example-mq",
            "--icon",
            "icon.png",
            "--banner",
            "banner.png",
            "--abi",
            "arm64-v8a",
        ]);
        let apk = opts.apk_path();
        let args = build_args(&opts, &apk).expect("args");
        assert_eq!(
            args,
            vec![
                "--package",
                "au.defrag.example",
                "--library",
                "example_mq",
                "--crate",
                "example-mq",
                "--out",
                "/w/.tmp/android/au.defrag.example.apk",
                "--icon",
                "icon.png",
                "--banner",
                "banner.png",
                "--abi",
                "arm64-v8a",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_art_is_forwarded_and_art_is_not_invented() {
        let opts = options(&[
            "--package",
            "a.b",
            "--library",
            "l",
            "--crate",
            "c",
            "--no-art",
        ]);
        let args = build_args(&opts, Path::new("/w/x.apk")).expect("args");
        assert!(args.contains(&"--no-art".to_string()));
        assert!(!args.contains(&"--icon".to_string()));
    }

    #[test]
    fn an_explicit_packager_is_one_command() {
        let (program, args) = Packager::Binary(PathBuf::from("/tools/mq-apk")).command();
        assert_eq!(program, "/tools/mq-apk");
        assert!(args.is_empty());
        let (program, args) = Packager::Cargo.command();
        assert_eq!(program, "cargo");
        assert_eq!(args.join(" "), "run -q -p mq-apk --");
    }

    #[test]
    fn the_command_line_quotes_only_what_needs_it() {
        assert_eq!(
            command_line("mq-apk", &["--out".into(), "/a b/x.apk".into()]),
            "mq-apk --out '/a b/x.apk'"
        );
    }
}
