//! Reader for the YouTube history from Google Takeout (`watch-history.json`).
//!
//! Only YouTube Music entries are kept. The history doesn't say how long each
//! song was played, so every play counts as `avg_minutes`.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::Loaded;
use crate::model::{Play, Source};
use crate::normalize::{clean_artist, strip_artist_prefix};

#[derive(Deserialize)]
struct Entry {
    header: Option<String>,
    title: Option<String>,
    #[serde(rename = "titleUrl")]
    title_url: Option<String>,
    subtitles: Option<Vec<Subtitle>>,
    time: Option<String>,
}

#[derive(Deserialize)]
struct Subtitle {
    name: Option<String>,
}

/// Prefixes Takeout puts before the title, depending on the account language.
const PREFIXES: &[&str] = &[
    "Has visto ",
    "Watched ",
    "Vous avez regardé ",
    "Hai guardato ",
    "Você assistiu a ",
    "Assistiu a ",
    "Has escoltat ",
    "Heu vist ",
];

/// Equivalent suffixes for languages that put the verb last.
const SUFFIXES: &[&str] = &[" angesehen", " bekeken"];

pub fn load(path: &Path, avg_minutes: f64) -> Result<Loaded> {
    if !path.is_file() {
        bail!("YouTube Music file does not exist: {}", path.display());
    }
    if path.extension().and_then(|e| e.to_str()) == Some("html") {
        bail!(
            "{} is HTML. In Google Takeout, choose JSON as the format for the YouTube history.",
            path.display()
        );
    }
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let plays =
        parse(&text, avg_minutes).with_context(|| format!("cannot parse {}", path.display()))?;
    let mut loaded = Loaded {
        plays,
        warnings: vec![],
    };
    if loaded.plays.is_empty() {
        loaded
            .warnings
            .push("YouTube Music: the file has no YouTube Music entries.".into());
    }
    Ok(loaded)
}

fn parse(text: &str, avg_minutes: f64) -> Result<Vec<Play>> {
    let entries: Vec<Entry> = serde_json::from_str(text).context("invalid JSON")?;
    let ms = (avg_minutes.max(0.0) * 60_000.0).round() as u64;
    let plays = entries
        .into_iter()
        .filter(is_music)
        .filter_map(|e| {
            let track = strip_title(e.title.as_deref()?)?;
            // No subtitle = deleted video or no channel: the artist is unknown.
            let artist = clean_artist(e.subtitles?.into_iter().next()?.name.as_deref()?);
            if track.is_empty() || artist.is_empty() {
                return None;
            }
            let track = strip_artist_prefix(&track, &artist);
            let ts = DateTime::parse_from_rfc3339(e.time.as_deref()?)
                .ok()?
                .with_timezone(&Utc);
            let video_id = e.title_url.as_deref().and_then(video_id);
            Some(Play {
                ts,
                track,
                artist,
                album: None,
                ms_played: ms,
                estimated: true,
                source: Source::YouTubeMusic,
                video_id,
            })
        })
        .collect();
    Ok(plays)
}

/// Extracts the video ID from "https://music.youtube.com/watch?v=XXXXXXXXXXX&...".
fn video_id(url: &str) -> Option<String> {
    let (_, query) = url.split_once('?')?;
    let id = query.split('&').find_map(|kv| kv.strip_prefix("v="))?;
    let valid = id.len() == 11
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    valid.then(|| id.to_string())
}

fn is_music(e: &Entry) -> bool {
    e.header.as_deref() == Some("YouTube Music")
        || e.title_url
            .as_deref()
            .is_some_and(|u| u.contains("music.youtube.com"))
}

fn strip_title(title: &str) -> Option<String> {
    let t = title.trim();
    for p in PREFIXES {
        if let Some(rest) = t.strip_prefix(p) {
            return Some(rest.trim().to_string());
        }
    }
    for s in SUFFIXES {
        if let Some(rest) = t.strip_suffix(s) {
            return Some(rest.trim().to_string());
        }
    }
    // Unknown language: use the title as is.
    Some(t.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_youtube_music_and_cleans() {
        let json = r#"[
          {"header":"YouTube Music","title":"Has visto DESPECHÁ","titleUrl":"https://music.youtube.com/watch?v=1","subtitles":[{"name":"ROSALÍA - Topic"}],"time":"2025-02-01T20:00:00.123Z"},
          {"header":"YouTube","title":"Watched Cat video","titleUrl":"https://www.youtube.com/watch?v=2","subtitles":[{"name":"Cats"}],"time":"2025-02-01T20:05:00Z"},
          {"header":"YouTube Music","title":"Watched a video that has been removed","time":"2025-02-01T20:10:00Z"}
        ]"#;
        let plays = parse(json, 3.5).unwrap();
        assert_eq!(plays.len(), 1);
        assert_eq!(plays[0].track, "DESPECHÁ");
        assert_eq!(plays[0].artist, "ROSALÍA");
        assert_eq!(plays[0].ms_played, 210_000);
        assert!(plays[0].estimated);
    }

    #[test]
    fn extracts_video_id() {
        assert_eq!(
            video_id("https://music.youtube.com/watch?v=dQw4w9WgXcQ").as_deref(),
            Some("dQw4w9WgXcQ")
        );
        assert_eq!(
            video_id("https://music.youtube.com/watch?list=x&v=dQw4w9WgXcQ&t=3").as_deref(),
            Some("dQw4w9WgXcQ")
        );
        assert_eq!(video_id("https://music.youtube.com/watch?v=short"), None);
        assert_eq!(video_id("https://music.youtube.com/"), None);
    }
}
