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
        .arg("--offline")
        // An empty cache folder: no song database, so the estimates are kept.
        .env("XDG_CACHE_HOME", dir.path())
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
    assert!(stdout.contains("Minutes per month:\n  2025-01"), "{stdout}");

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
fn terminal_goes_by_year_past_24_months() {
    let output = bin()
        .arg("--spotify")
        .arg(fixtures().join("spotify"))
        .args(["--from", "2023-01-01", "--to", "2025-12-31", "--no-image"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Minutes per year:\n  2023 "), "{stdout}");
    assert!(stdout.contains("\n  2025 "), "{stdout}");
    assert!(!stdout.contains("2025-01"), "{stdout}");
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

#[test]
fn exports_spotify_song_lengths() {
    let dir = tempfile::tempdir().unwrap();
    let history = dir.path().join("Streaming_History_Audio_2025_0.json");
    let entry = |artist: &str, track: &str, ms: u64, reason: &str| {
        format!(
            r#"{{"ts":"2025-01-02T10:00:00Z","ms_played":{ms},"master_metadata_track_name":"{track}","master_metadata_album_artist_name":"{artist}","master_metadata_album_album_name":"Album","reason_end":"{reason}","ip_addr":"1.2.3.4"}}"#
        )
    };
    let entries = [
        entry("A", "New", 200_000, "trackdone"),
        entry("A", "New", 202_000, "trackdone"),
        entry("A", "New", 90_000, "fwdbtn"),
        entry("A", "Known", 180_000, "trackdone"),
        entry("A", "Known", 180_000, "trackdone"),
        entry("A", "Once", 150_000, "trackdone"),
    ];
    std::fs::write(&history, format!("[{}]", entries.join(","))).unwrap();
    let db = dir.path().join("songs.json");
    std::fs::write(
        &db,
        r#"{"version": 1, "songs": {"a|known": {"artist": "A", "title": "Known", "seconds": 180, "source": "manual"}}}"#,
    )
    .unwrap();

    let o = bin()
        .args(["db", "export-spotify", "--spotify"])
        .arg(&history)
        .arg("--db")
        .arg(&db)
        .current_dir(dir.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(
        stdout.contains("to check, only one full play: 1"),
        "{stdout}"
    );

    let file = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            let name = p.file_name().unwrap().to_string_lossy();
            name.starts_with("spotify-durations-") && name.ends_with(".json")
        })
        .expect("export file");
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    assert_eq!(
        json,
        serde_json::json!([
            {"artist": "A", "title": "New", "album": "Album", "seconds": 201},
            {"artist": "A", "title": "Once", "album": "Album", "check": "one full play", "seconds": "2:30"}
        ])
    );
}
