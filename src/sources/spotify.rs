//! Reader for Spotify's data export.
//!
//! Prefers the "Extended streaming history" (`Streaming_History_Audio_*.json`).
//! If it's not there, falls back to the basic history from "Account data"
//! (`StreamingHistory_music_*.json`), which only covers the last year.

use anyhow::{Context, Result, bail};
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Deserialize;

use super::{File, Loaded};
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
    /// Why the play ended; `"trackdone"` means it reached the end of the song.
    reason_end: Option<String>,
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

/// Loads the plays from the files of an export, preferring the extended history
/// over the basic one. `place` says where the files come from, for messages.
///
/// Fails if none of the files is a Spotify history. A file that can't be read
/// or parsed is skipped with a warning.
pub fn load(files: &[File], place: &str) -> Result<Loaded> {
    let extended = find(files, &["Streaming_History_Audio_*.json"]);
    if !extended.is_empty() {
        return Ok(read_all(&extended, parse_extended));
    }

    let basic = find(
        files,
        &["StreamingHistory_music_*.json", "StreamingHistory*.json"],
    );
    if !basic.is_empty() {
        let mut loaded = read_all(&basic, parse_basic);
        loaded.warnings.push(
            "Spotify: using the basic history (last year only). Request the \"Extended streaming history\" to get everything."
                .into(),
        );
        return Ok(loaded);
    }

    bail!(
        "No Spotify files found in {place}. Looking for Streaming_History_Audio_*.json (extended history) or StreamingHistory_music_*.json."
    )
}

/// Loads only the full plays (those that reached the end of the song) from the
/// extended history. Fails if there is none: the basic history doesn't say how a
/// play ended.
pub fn full_plays(files: &[File], place: &str) -> Result<Vec<Play>> {
    let files = find(files, &["Streaming_History_Audio_*.json"]);
    if files.is_empty() {
        bail!(
            "Spotify song lengths need the Extended streaming history (Streaming_History_Audio_*.json) in {place}; the basic history doesn't say whether a play reached the end"
        );
    }
    let mut plays = vec![];
    for file in files {
        let entries = parse_entries(file.text()?).with_context(|| format!("in {}", file.name))?;
        plays.extend(
            entries
                .into_iter()
                .filter(|(_, reason)| reason.as_deref() == Some("trackdone"))
                .map(|(p, _)| p),
        );
    }
    Ok(plays)
}

/// The files whose name matches the first of the patterns that matches any.
fn find<'a>(files: &'a [File], patterns: &[&str]) -> Vec<&'a File> {
    patterns
        .iter()
        .filter_map(|p| glob::Pattern::new(p).ok())
        .map(|g| {
            files
                .iter()
                .filter(|f| g.matches(f.file_name()))
                .collect::<Vec<_>>()
        })
        .find(|found| !found.is_empty())
        .unwrap_or_default()
}

/// Parses every file with `parse`, turning failures into warnings.
fn read_all(files: &[&File], parse: fn(&str) -> Result<Vec<Play>>) -> Loaded {
    let mut loaded = Loaded::default();
    for file in files {
        match file.text().and_then(parse) {
            Ok(mut plays) => loaded.plays.append(&mut plays),
            Err(e) => loaded
                .warnings
                .push(format!("Spotify: skipping {} ({e:#})", file.name)),
        }
    }
    loaded
}

/// Parses an extended history file. Entries without a track name are podcasts,
/// audiobooks or empty, and are ignored.
fn parse_extended(text: &str) -> Result<Vec<Play>> {
    Ok(parse_entries(text)?.into_iter().map(|(p, _)| p).collect())
}

/// Parses an extended history file into plays with their `reason_end`.
fn parse_entries(text: &str) -> Result<Vec<(Play, Option<String>)>> {
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
            let play = Play {
                ts,
                track,
                artist,
                album: e.master_metadata_album_album_name,
                ms_played: e.ms_played,
                estimated: false,
                source: Source::Spotify,
                video_id: None,
            };
            Some((play, e.reason_end))
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

    fn file(name: &str, text: &str) -> File {
        File {
            name: name.into(),
            bytes: text.as_bytes().to_vec(),
        }
    }

    #[test]
    fn full_plays_are_only_trackdone() {
        let entry = |ms: u64, reason: &str| {
            format!(
                r#"{{"ts":"2025-01-02T10:00:00Z","ms_played":{ms},"master_metadata_track_name":"T","master_metadata_album_artist_name":"A","reason_end":"{reason}"}}"#
            )
        };
        let json = format!(
            "[{},{},{}]",
            entry(200_000, "trackdone"),
            entry(50_000, "fwdbtn"),
            entry(10_000, "endplay")
        );
        let files = [file("Streaming_History_Audio_2025.json", &json)];
        let plays = full_plays(&files, "here").unwrap();
        assert_eq!(plays.len(), 1);
        assert_eq!(plays[0].ms_played, 200_000);
    }

    #[test]
    fn full_plays_need_the_extended_history() {
        let files = [file("StreamingHistory_music_0.json", "[]")];
        let err = full_plays(&files, "here").unwrap_err().to_string();
        assert!(err.contains("Extended streaming history"), "{err}");
    }
}
