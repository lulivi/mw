//! Reader for Spotify's data export.
//!
//! Prefers the "Extended streaming history" (`Streaming_History_Audio_*.json`).
//! If it's not there, falls back to the basic history from "Account data"
//! (`StreamingHistory_music_*.json`), which only covers the last year.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Deserialize;

use super::Loaded;
use crate::model::{Play, Source};
use crate::normalize::clean_artist;

/// One play in the extended history; only the fields used here.
#[derive(Deserialize)]
struct ExtendedEntry {
    ts: String,
    #[serde(default)]
    ms_played: u64,
    master_metadata_track_name: Option<String>,
    master_metadata_album_artist_name: Option<String>,
    master_metadata_album_album_name: Option<String>,
}

/// One play in the basic history.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BasicEntry {
    /// When the play ended, as "2024-03-01 21:15" in UTC.
    end_time: String,
    artist_name: Option<String>,
    track_name: Option<String>,
    #[serde(default)]
    ms_played: u64,
}

/// Loads the plays from a folder or a single file, preferring the extended history
/// over the basic one.
///
/// Fails if the path doesn't exist or holds no Spotify files. A file that can't be
/// read or parsed is skipped with a warning.
pub fn load(path: &Path) -> Result<Loaded> {
    if !path.exists() {
        bail!("Spotify path does not exist: {}", path.display());
    }

    let extended = find(path, &["Streaming_History_Audio_*.json"])?;
    if !extended.is_empty() {
        return read_all(&extended, parse_extended);
    }

    let basic = find(
        path,
        &["StreamingHistory_music_*.json", "StreamingHistory*.json"],
    )?;
    if !basic.is_empty() {
        let mut loaded = read_all(&basic, parse_basic)?;
        loaded.warnings.push(
            "Spotify: using the basic history (last year only). Request the \"Extended streaming history\" to get everything."
                .into(),
        );
        return Ok(loaded);
    }

    bail!(
        "No Spotify files found in {}. Looking for Streaming_History_Audio_*.json (extended history) or StreamingHistory_music_*.json.",
        path.display()
    )
}

/// Finds the files matching the patterns. Accepts a single file or a folder
/// (subfolders are searched too, since Spotify's ZIP contains a folder).
fn find(path: &Path, patterns: &[&str]) -> Result<Vec<PathBuf>> {
    if path.is_file() {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let ok = patterns.iter().any(|p| {
            glob::Pattern::new(p)
                .map(|g| g.matches(name))
                .unwrap_or(false)
        });
        return Ok(if ok { vec![path.to_path_buf()] } else { vec![] });
    }
    for pattern in patterns {
        let full = path.join("**").join(pattern);
        let mut files: Vec<PathBuf> = glob::glob(&full.to_string_lossy())
            .context("invalid search pattern")?
            .filter_map(|r| r.ok())
            .collect();
        files.sort();
        if !files.is_empty() {
            return Ok(files);
        }
    }
    Ok(vec![])
}

/// Parses every file with `parse`, turning failures into warnings.
fn read_all(files: &[PathBuf], parse: fn(&str) -> Result<Vec<Play>>) -> Result<Loaded> {
    let mut loaded = Loaded::default();
    for file in files {
        match fs::read_to_string(file)
            .with_context(|| format!("cannot read {}", file.display()))
            .and_then(|text| parse(&text))
        {
            Ok(mut plays) => loaded.plays.append(&mut plays),
            Err(e) => loaded
                .warnings
                .push(format!("Spotify: skipping {} ({e:#})", file.display())),
        }
    }
    Ok(loaded)
}

/// Parses an extended history file. Entries without a track name are podcasts,
/// audiobooks or empty, and are ignored.
fn parse_extended(text: &str) -> Result<Vec<Play>> {
    let entries: Vec<ExtendedEntry> = serde_json::from_str(text).context("invalid JSON")?;
    let plays = entries
        .into_iter()
        .filter_map(|e| {
            let track = e.master_metadata_track_name?.trim().to_string();
            let artist = clean_artist(&e.master_metadata_album_artist_name?);
            if track.is_empty() || artist.is_empty() {
                return None;
            }
            let ts = DateTime::parse_from_rfc3339(&e.ts)
                .ok()?
                .with_timezone(&Utc);
            Some(Play {
                ts,
                track,
                artist,
                album: e.master_metadata_album_album_name,
                ms_played: e.ms_played,
                estimated: false,
                source: Source::Spotify,
                video_id: None,
            })
        })
        .collect();
    Ok(plays)
}

/// Parses a basic history file, ignoring entries without a known artist.
fn parse_basic(text: &str) -> Result<Vec<Play>> {
    let entries: Vec<BasicEntry> = serde_json::from_str(text).context("invalid JSON")?;
    let plays = entries
        .into_iter()
        .filter_map(|e| {
            let track = e.track_name?.trim().to_string();
            let artist = clean_artist(&e.artist_name?);
            if track.is_empty() || artist.is_empty() || artist == "Unknown Artist" {
                return None;
            }
            let ts = NaiveDateTime::parse_from_str(&e.end_time, "%Y-%m-%d %H:%M")
                .ok()?
                .and_utc();
            Some(Play {
                ts,
                track,
                artist,
                album: None,
                ms_played: e.ms_played,
                estimated: false,
                source: Source::Spotify,
                video_id: None,
            })
        })
        .collect();
    Ok(plays)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_extended_history_and_skips_podcasts() {
        let json = r#"[
          {"ts":"2025-01-02T10:00:00Z","ms_played":200000,"master_metadata_track_name":"Song","master_metadata_album_artist_name":"Artist","master_metadata_album_album_name":"Album"},
          {"ts":"2025-01-02T11:00:00Z","ms_played":900000,"master_metadata_track_name":null,"master_metadata_album_artist_name":null,"episode_name":"Podcast"}
        ]"#;
        let plays = parse_extended(json).unwrap();
        assert_eq!(plays.len(), 1);
        assert_eq!(plays[0].artist, "Artist");
        assert_eq!(plays[0].ms_played, 200000);
    }

    #[test]
    fn reads_basic_history() {
        let json = r#"[{"endTime":"2025-03-01 21:15","artistName":"A","trackName":"T","msPlayed":180000}]"#;
        let plays = parse_basic(json).unwrap();
        assert_eq!(plays.len(), 1);
        assert_eq!(plays[0].ts.to_rfc3339(), "2025-03-01T21:15:00+00:00");
    }
}
