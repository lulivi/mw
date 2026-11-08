//! End-to-end tests: run the binary on the sample data.

use std::path::PathBuf;
use std::process::Command;

/// The compiled `mw` binary, ready to receive arguments.
fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mw"))
}

/// Folder with the sample data.
fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Reads width and height from a PNG's IHDR header.
fn png_size(bytes: &[u8]) -> (u32, u32) {
    assert_eq!(&bytes[1..4], b"PNG");
    let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    (w, h)
}

#[test]
fn builds_mobile_png_with_both_sources() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("w.png");
    let output = bin()
        .arg("--spotify")
        .arg(fixtures().join("spotify"))
        .arg("--ytm")
        .arg(fixtures().join("watch-history.json"))
        .args(["--year", "2025"])
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Rosalía: 200,000 + 230,000 ms from Spotify (the Dec 31 UTC play falls in 2025 in Madrid)
    // + 210,000 estimated from YouTube Music = a single entry with ~11 min.
    assert!(stdout.contains("1. Rosalía  (11 min, 3 plays)"), "{stdout}");
    assert!(stdout.contains("Spotify:"));
    assert!(stdout.contains("YouTube Music:"));

    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(png_size(&bytes), (1080, 1920));
}

#[test]
fn terminal_only_and_spotify_only() {
    let output = bin()
        .arg("--spotify")
        .arg(fixtures().join("spotify"))
        .args([
            "--from",
            "2024-06-01",
            "--to",
            "2024-06-30",
            "--source",
            "spotify",
            "--no-image",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Malamente"));
    assert!(!stdout.contains("≈"));
}

#[test]
fn color_changes_the_image() {
    let dir = tempfile::tempdir().unwrap();
    let render = |name: &str, args: &[&str]| {
        let out = dir.path().join(name);
        let o = bin()
            .arg("--spotify")
            .arg(fixtures().join("spotify"))
            .args(args)
            .arg("--out")
            .arg(&out)
            .output()
            .unwrap();
        assert!(o.status.success());
        std::fs::read(&out).unwrap()
    };
    let default = render("default.png", &[]);
    assert_eq!(default, render("orange.png", &["--color", "orange"]));
    assert_ne!(default, render("purple.png", &["--color", "purple"]));
}

#[test]
fn keeps_an_existing_image_unless_overwriting() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("w.png");
    std::fs::write(&out, "keep me").unwrap();
    let run = |args: &[&str]| {
        bin()
            .arg("--spotify")
            .arg(fixtures().join("spotify"))
            .arg("--out")
            .arg(&out)
            .args(args)
            .output()
            .unwrap()
    };

    let refused = run(&[]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--overwrite"));
    assert_eq!(std::fs::read(&out).unwrap(), b"keep me");

    assert!(run(&["--no-image"]).status.success());
    assert_eq!(std::fs::read(&out).unwrap(), b"keep me");

    assert!(run(&["--overwrite"]).status.success());
    assert_eq!(&std::fs::read(&out).unwrap()[1..4], b"PNG");
}

#[test]
fn clear_errors() {
    // No sources
    let o = bin().arg("--no-image").output().unwrap();
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("Provide at least one source"));

    // --year with --from
    let o = bin()
        .arg("--spotify")
        .arg(fixtures().join("spotify"))
        .args(["--year", "2025", "--from", "2025-01-01"])
        .output()
        .unwrap();
    assert!(!o.status.success());

    // Range without data
    let o = bin()
        .arg("--spotify")
        .arg(fixtures().join("spotify"))
        .args(["--year", "2019", "--no-image"])
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("No plays in that date range"));
}
