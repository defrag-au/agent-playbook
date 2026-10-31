use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use image::{Rgba, RgbaImage};

fn workspace(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("image-shot-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fixture(dir: &Path, name: &str, width: u32, height: u32, seed: u8) -> PathBuf {
    let mut image = RgbaImage::new(width, height);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        *pixel = Rgba([
            seed.wrapping_add((x * 37) as u8),
            seed.wrapping_add((y * 53) as u8),
            64,
            255,
        ]);
    }
    let path = dir.join(name);
    image.save(&path).unwrap();
    path
}

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_image-shot"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn info_reports_the_header_without_decoding_pixels() {
    let dir = workspace("info");
    let source = fixture(&dir, "source.png", 7, 3, 10);
    let output = run(&dir, &["info", source.to_str().unwrap()]);
    assert!(output.status.success(), "{}", stderr(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("7x3"), "{stdout}");
    assert!(stdout.contains("png"), "{stdout}");
}

#[test]
fn crop_writes_the_requested_box_at_the_zoom_size() {
    let dir = workspace("crop");
    let source = fixture(&dir, "source.png", 10, 8, 10);
    let artifacts = dir.join("artifacts");
    let output = run(
        &dir,
        &[
            "--artifacts",
            artifacts.to_str().unwrap(),
            "crop",
            source.to_str().unwrap(),
            "--box",
            "1,1,4,2",
            "--zoom",
            "2",
            "--out",
            "crop.png",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));

    let cropped = image::open(artifacts.join("crop.png")).unwrap();
    assert_eq!((cropped.width(), cropped.height()), (8, 4));
}

#[test]
fn diff_exits_one_and_writes_a_view_for_different_images() {
    let dir = workspace("diff");
    let a = fixture(&dir, "a.png", 4, 4, 10);
    let b = fixture(&dir, "b.png", 4, 4, 20);
    let artifacts = dir.join("artifacts");
    let output = run(
        &dir,
        &[
            "--artifacts",
            artifacts.to_str().unwrap(),
            "diff",
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            "--out",
            "diffs/change.png",
        ],
    );
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(artifacts.join("diffs/change.png").exists());

    let identical = run(&dir, &["diff", a.to_str().unwrap(), a.to_str().unwrap()]);
    assert_eq!(identical.status.code(), Some(0), "{}", stderr(&identical));
}

#[test]
fn out_may_not_escape_the_artifacts_directory() {
    let dir = workspace("escape");
    let source = fixture(&dir, "source.png", 10, 8, 10);
    let artifacts = dir.join("artifacts");
    let output = run(
        &dir,
        &[
            "--artifacts",
            artifacts.to_str().unwrap(),
            "crop",
            source.to_str().unwrap(),
            "--box",
            "0,0,4,2",
            "--out",
            "../escape.png",
        ],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("must not contain"),
        "{}",
        stderr(&output)
    );
    assert!(!dir.join("escape.png").exists());
}

#[test]
fn out_may_not_name_the_source() {
    let dir = workspace("overwrite");
    let source = fixture(&dir, "source.png", 10, 8, 10);
    let output = run(
        &dir,
        &[
            "--artifacts",
            dir.to_str().unwrap(),
            "crop",
            source.to_str().unwrap(),
            "--box",
            "0,0,4,2",
            "--out",
            "source.png",
        ],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("names the source"),
        "{}",
        stderr(&output)
    );
}
