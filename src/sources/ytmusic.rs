//! Reader for the YouTube history from Google Takeout (`watch-history.json`).
//!
//! Only YouTube Music entries are kept. The history doesn't say how long each
//! song was played, so every play counts as `avg_minutes`.

use std::collections::BTreeSet;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::{File, Loaded};
use crate::model::{Play, Source};
use crate::normalize::{clean_artist, strip_artist_prefix};

/// One item in the history; only the fields used here.
#[derive(Deserialize)]
struct Entry {
    header: Option<String>,
    title: Option<String>,
    #[serde(rename = "titleUrl")]
    title_url: Option<String>,
    subtitles: Option<Vec<Subtitle>>,
    time: Option<String>,
}

/// A subtitle of an [`Entry`]; the first one holds the channel.
#[derive(Deserialize)]
struct Subtitle {
    name: Option<String>,
}

/// Distinct titles needed to trust the wording detected from the file.
const MIN_TITLES: usize = 20;

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

/// Loads the plays from `watch-history.json`, counting each as `avg_minutes`.
///
/// Fails if the file is the HTML export or isn't valid JSON.
pub fn load(file: &File, avg_minutes: f64) -> Result<Loaded> {
    if file.name.ends_with(".html") {
        bail!(
            "{} is HTML. In Google Takeout, choose JSON as the format for the YouTube history.",
            file.name
        );
    }
    let plays =
        parse(file.text()?, avg_minutes).with_context(|| format!("cannot parse {}", file.name))?;
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

/// Parses the history, keeping YouTube Music entries with a known song and artist.
/// An entry without a subtitle is a deleted video or has no channel, so its artist
/// is unknown and it's dropped.
fn parse(text: &str, avg_minutes: f64) -> Result<Vec<Play>> {
    let entries: Vec<Entry> = serde_json::from_str(text).context("invalid JSON")?;
    let ms = (avg_minutes.max(0.0) * 60_000.0).round() as u64;
    let entries: Vec<Entry> = entries.into_iter().filter(is_music).collect();
    let wording = detect_wording(&entries);
    let plays = entries
        .into_iter()
        .filter_map(|e| {
            let track = strip_title(e.title.as_deref()?, &wording);
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

/// Extracts the video ID from `https://music.youtube.com/watch?v=XXXXXXXXXXX&...`.
fn video_id(url: &str) -> Option<String> {
    let (_, query) = url.split_once('?')?;
    let id = query.split('&').find_map(|kv| kv.strip_prefix("v="))?;
    let valid = id.len() == 11
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    valid.then(|| id.to_string())
}

/// Is this entry from YouTube Music, rather than plain YouTube?
fn is_music(e: &Entry) -> bool {
    e.header.as_deref() == Some("YouTube Music")
        || e.title_url
            .as_deref()
            .is_some_and(|u| u.contains("music.youtube.com"))
}

/// The title wording of a file: what every title starts and ends with.
#[derive(Default)]
struct Wording {
    prefix: String,
    suffix: String,
}

/// Detects the title wording from the common prefix and suffix of the titles,
/// cut at a word boundary. Entries without a video (removed ones) have another
/// text, so they're left out. With fewer than [`MIN_TITLES`] distinct titles the
/// titles may share words by chance, so nothing is detected.
fn detect_wording(entries: &[Entry]) -> Wording {
    let titles: BTreeSet<&str> = entries
        .iter()
        .filter(|e| e.title_url.is_some())
        .filter_map(|e| Some(e.title.as_deref()?.trim()))
        .collect();
    if titles.len() < MIN_TITLES {
        return Wording::default();
    }
    let mut iter = titles.iter();
    let first = iter.next().copied().unwrap_or_default();
    let (mut pre, mut suf) = (first, first);
    for t in iter {
        pre = common_prefix(pre, t);
        suf = common_suffix(suf, t);
    }
    let prefix = pre
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace())
        .map_or("", |(i, c)| &pre[..i + c.len_utf8()]);
    let suffix = suf
        .char_indices()
        .find(|(_, c)| c.is_whitespace())
        .map_or("", |(i, _)| &suf[i..]);
    Wording {
        prefix: prefix.to_string(),
        suffix: suffix.to_string(),
    }
}

/// The longest start `a` and `b` share.
fn common_prefix<'a>(a: &'a str, b: &str) -> &'a str {
    let end = a
        .char_indices()
        .zip(b.chars())
        .take_while(|((_, x), y)| x == y)
        .last()
        .map_or(0, |((i, c), _)| i + c.len_utf8());
    &a[..end]
}

/// The longest end `a` and `b` share.
fn common_suffix<'a>(a: &'a str, b: &str) -> &'a str {
    let start = a
        .char_indices()
        .rev()
        .zip(b.chars().rev())
        .take_while(|((_, x), y)| x == y)
        .last()
        .map_or(a.len(), |((i, _), _)| i);
    &a[start..]
}

/// Removes the "Watched"-like wording Takeout adds around the title: the one
/// detected from the file, or else a known one. In a language it doesn't know,
/// the title is returned as is.
fn strip_title(title: &str, wording: &Wording) -> String {
    let t = title.trim();
    let rest = t.strip_prefix(wording.prefix.as_str()).unwrap_or(t);
    let rest = rest.strip_suffix(wording.suffix.as_str()).unwrap_or(rest);
    if rest.len() < t.len() && !rest.trim().is_empty() {
        return rest.trim().to_string();
    }
    for p in PREFIXES {
        if let Some(rest) = t.strip_prefix(p) {
            return rest.trim().to_string();
        }
    }
    for s in SUFFIXES {
        if let Some(rest) = t.strip_suffix(s) {
            return rest.trim().to_string();
        }
    }
    t.to_string()
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

    /// A history with one YouTube Music entry per title, plus a removed video.
    fn history(titles: &[String]) -> String {
        let mut entries: Vec<_> = titles
            .iter()
            .enumerate()
            .map(|(i, t)| {
                serde_json::json!({
                    "header": "YouTube Music",
                    "title": t,
                    "titleUrl": format!("https://music.youtube.com/watch?v={i}"),
                    "subtitles": [{"name": "Artist"}],
                    "time": "2025-02-01T20:00:00Z",
                })
            })
            .collect();
        entries.push(serde_json::json!({
            "header": "YouTube Music",
            "title": "Something else entirely",
            "time": "2025-02-01T20:00:00Z",
        }));
        serde_json::Value::Array(entries).to_string()
    }

    const NAMES: [&str; 20] = [
        "Alfa", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India",
        "Juliett", "Kilo", "Lima", "Mike", "November", "Oscar", "Papa", "Quebec", "Romeo",
        "Sierra", "Tango",
    ];

    fn tracks(titles: &[String]) -> Vec<String> {
        parse(&history(titles), 3.0)
            .unwrap()
            .into_iter()
            .map(|p| p.track)
            .collect()
    }

    #[test]
    fn strips_unknown_prefix_wording() {
        let titles: Vec<_> = NAMES.iter().map(|n| format!("Zorp blarg {n}")).collect();
        let want: Vec<_> = NAMES.iter().map(|n| n.to_string()).collect();
        assert_eq!(tracks(&titles), want);
    }

    #[test]
    fn strips_unknown_suffix_wording() {
        let titles: Vec<_> = NAMES.iter().map(|n| format!("{n} glorpen")).collect();
        let want: Vec<_> = NAMES.iter().map(|n| n.to_string()).collect();
        assert_eq!(tracks(&titles), want);
    }

    #[test]
    fn small_history_uses_known_wording() {
        let titles = ["Watched Alfa", "Zorp blarg Bravo"].map(String::from);
        assert_eq!(tracks(&titles), ["Alfa", "Zorp blarg Bravo"]);
    }

    #[test]
    fn few_titles_sharing_a_word_are_not_cut() {
        let titles = ["Love Story", "Love Me", "Love Song"].map(|t| format!("Watched {t}"));
        assert_eq!(tracks(&titles), ["Love Story", "Love Me", "Love Song"]);
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
