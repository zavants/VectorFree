//! End-to-end checks of the command-line tool.

use std::path::PathBuf;
use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_vectorfree-cli"))
}

/// Fresh empty folder for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vectorfree-cli-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A small two-color picture: a dark square on a light background.
fn write_picture(path: &std::path::Path) {
    image::RgbaImage::from_fn(40, 40, |x, y| {
        if (10..30).contains(&x) && (10..30).contains(&y) {
            image::Rgba([20, 30, 120, 255])
        } else {
            image::Rgba([240, 240, 235, 255])
        }
    })
    .save(path)
    .unwrap();
}

#[test]
fn version_flag_prints_the_version() {
    for flag in ["--version", "-V"] {
        let out = cli().arg(flag).output().unwrap();
        assert!(out.status.success(), "{flag}: {out:?}");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains(env!("CARGO_PKG_VERSION")), "{flag}: {text}");
    }
}

#[test]
fn help_shows_examples() {
    let out = cli().arg("--help").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("EXAMPLES") && text.contains("vectorfree-cli logo.png"), "{text}");
}

/// Exporting a PNG next to a PNG source would replace the original picture.
#[test]
fn never_overwrites_the_source_image() {
    let dir = scratch("overwrite");
    let src = dir.join("logo.png");
    write_picture(&src);
    let before = std::fs::read(&src).unwrap();
    let out = cli().arg(&src).args(["-f", "svg,png"]).output().unwrap();
    assert!(!out.status.success(), "must fail: {out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("overwrite"), "{out:?}");
    assert_eq!(std::fs::read(&src).unwrap(), before, "source image was changed");
    // Into another folder the same command works.
    let other = dir.join("out");
    let out = cli().arg(&src).args(["-f", "svg,png", "-o"]).arg(&other).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(other.join("logo.png").is_file() && other.join("logo.svg").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}
