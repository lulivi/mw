//! Real length and names of YouTube Music songs via the official YouTube API
//! (YouTube Data API v3, `videos.list`).
//!
//! Optional: only used when an API key is provided. Each request asks for up to 50
//! videos and costs 1 unit of a free 10,000-unit daily quota. Results are stored in
//! a JSON cache: videos with a known length and title are never fetched again.
//! Those missing either are retried on every run with a key, in case it was a
//! transient failure; a deleted video only costs its share of one request.
//!
//! The video title and channel replace the names read from Takeout, so they don't
//! depend on the "Watched"-like wording Takeout adds in each language.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::model::{Play, Source};
use crate::normalize::{clean_artist, strip_artist_prefix};

/// Endpoint of the YouTube Data API `videos.list` method.
const API_URL: &str = "https://www.googleapis.com/youtube/v3/videos";
/// Most videos the API accepts in one request.
const BATCH: usize = 50;

/// What the API told about a video. Every field is `None` when the video no
/// longer exists or the API didn't return it.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Video {
    /// Length in seconds.
    secs: Option<u64>,
    /// Video title, as the uploader wrote it.
    title: Option<String>,
    /// Name of the channel that uploaded it.
    channel: Option<String>,
}

/// Video ID -> what is known about it.
type Cache = BTreeMap<String, Video>;

/// A cache entry as stored on disk: the current [`Video`], or the older format
/// that only held the seconds (`null` when unknown).
#[derive(Deserialize)]
#[serde(untagged)]
enum Stored {
    Secs(Option<u64>),
    Video(Video),
}

/// How to fetch and apply real lengths.
pub struct Options {
    /// YouTube Data API v3 key.
    pub api_key: String,
    /// JSON file where the lengths are cached.
    pub cache_path: PathBuf,
    /// Cap per play, so a livestream or a 2-hour mix doesn't inflate the minutes.
    pub max_minutes: f64,
}

/// Outcome of [`resolve`], for the user.
#[derive(Default)]
pub struct Report {
    /// Distinct YouTube Music videos in the plays.
    pub distinct: usize,
    /// How many of them now have a real length.
    pub resolved: usize,
    /// How many of them now have names from the API.
    pub named: usize,
    /// Lengths obtained from the API in this run.
    pub fetched: usize,
    /// Successful API requests made in this run.
    pub requests: usize,
    /// Problems that didn't stop the program.
    pub warnings: Vec<String>,
}

/// Body of a `videos.list` response; only the fields used here.
#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    items: Vec<Item>,
}

/// One video in a [`Response`].
#[derive(Deserialize)]
struct Item {
    id: String,
    #[serde(rename = "contentDetails")]
    content_details: Option<ContentDetails>,
    snippet: Option<Snippet>,
}

/// The `snippet` part of an [`Item`].
#[derive(Deserialize)]
struct Snippet {
    title: Option<String>,
    #[serde(rename = "channelTitle")]
    channel_title: Option<String>,
}

/// The `contentDetails` part of an [`Item`].
#[derive(Deserialize)]
struct ContentDetails {
    /// Length in ISO 8601, such as "PT3M45S".
    duration: Option<String>,
}

/// Fetches the lengths and names missing from the cache and applies them to the
/// YouTube Music plays. Network or quota failures don't stop the program: a warning
/// is reported, songs without a length keep the estimate and songs without names
/// keep the ones from Takeout.
pub fn resolve(plays: &mut [Play], opt: &Options) -> Report {
    let mut report = Report::default();
    let mut cache = match load_cache(&opt.cache_path) {
        Ok(c) => c,
        Err(e) => {
            report.warnings.push(format!("cache ignored ({e:#})"));
            Cache::new()
        }
    };

    let ids: BTreeSet<&str> = plays
        .iter()
        .filter(|p| p.source == Source::YouTubeMusic)
        .filter_map(|p| p.video_id.as_deref())
        .collect();
    report.distinct = ids.len();

    let missing = pending(&ids, &cache);

    for chunk in missing.chunks(BATCH) {
        match fetch(&opt.api_key, chunk) {
            Ok(mut found) => {
                report.requests += 1;
                for id in chunk {
                    let new = found.remove(id).unwrap_or_default();
                    if new.secs.is_some() {
                        report.fetched += 1;
                    }
                    let entry = cache.entry(id.clone()).or_default();
                    // Keep what was known if the API no longer returns it.
                    *entry = Video {
                        secs: new.secs.or(entry.secs),
                        title: new.title.or(entry.title.take()),
                        channel: new.channel.or(entry.channel.take()),
                    };
                }
            }
            Err(e) => {
                let msg = format!("{e:#}").replace(&opt.api_key, "***");
                report.warnings.push(format!(
                    "YouTube API: {msg}. Songs without a length keep the estimate."
                ));
                break;
            }
        }
    }

    if report.requests > 0
        && let Err(e) = save_cache(&opt.cache_path, &cache)
    {
        report
            .warnings
            .push(format!("could not save the cache ({e:#})"));
    }

    let max_ms = (opt.max_minutes.max(0.0) * 60_000.0) as u64;
    (report.resolved, report.named) = apply(plays, &cache, max_ms);
    report
}

/// IDs to fetch: those not in the cache or cached without a length or title.
fn pending(ids: &BTreeSet<&str>, cache: &Cache) -> Vec<String> {
    ids.iter()
        .filter(|id| {
            cache
                .get(**id)
                .is_none_or(|v| v.secs.is_none() || v.title.is_none())
        })
        .map(|id| id.to_string())
        .collect()
}

/// Replaces the estimate with the real length (capped) and the Takeout names with
/// the API ones. Returns how many distinct videos have a length and how many have
/// names.
fn apply(plays: &mut [Play], cache: &Cache, max_ms: u64) -> (usize, usize) {
    let mut resolved = BTreeSet::new();
    let mut named = BTreeSet::new();
    for p in plays
        .iter_mut()
        .filter(|p| p.source == Source::YouTubeMusic)
    {
        let Some(id) = p.video_id.as_deref() else {
            continue;
        };
        let Some(v) = cache.get(id) else {
            continue;
        };
        if let Some(secs) = v.secs
            && secs > 0
        {
            p.ms_played = (secs * 1000).min(max_ms);
            p.estimated = false;
            resolved.insert(id.to_string());
        }
        if let Some((track, artist)) = names(v) {
            p.track = track;
            p.artist = artist;
            named.insert(id.to_string());
        }
    }
    (resolved.len(), named.len())
}

/// Song and artist from the video title and channel, cleaned like the Takeout
/// ones. `None` if either ends up empty.
fn names(v: &Video) -> Option<(String, String)> {
    let artist = clean_artist(v.channel.as_deref()?);
    let title = v.title.as_deref()?.trim();
    if artist.is_empty() || title.is_empty() {
        return None;
    }
    Some((strip_artist_prefix(title, &artist), artist))
}

/// Asks the API for the lengths and names of up to [`BATCH`] videos. Videos it
/// doesn't know about are simply missing from the result.
///
/// Fails on connection problems and non-success responses, such as an invalid key
/// or an exhausted quota.
fn fetch(key: &str, ids: &[String]) -> Result<HashMap<String, Video>> {
    let mut resp = ureq::get(API_URL)
        .query("part", "contentDetails,snippet")
        .query("id", ids.join(","))
        .query("maxResults", BATCH.to_string())
        .query("key", key)
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .map_err(|e| anyhow!("could not connect ({e})"))?;

    let status = resp.status();
    let text = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| anyhow!("unreadable response ({e})"))?;

    if !status.is_success() {
        let detail = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
            .unwrap_or_else(|| text.chars().take(200).collect());
        return Err(anyhow!("error {} ({detail})", status.as_u16()));
    }

    parse_response(&text)
}

/// Extracts video ID -> [`Video`] from a response body. An unreadable length is
/// left as `None`.
fn parse_response(text: &str) -> Result<HashMap<String, Video>> {
    let resp: Response = serde_json::from_str(text).context("unexpected response")?;
    Ok(resp
        .items
        .into_iter()
        .map(|it| {
            let secs = it
                .content_details
                .and_then(|c| c.duration)
                .and_then(|d| parse_iso8601(&d));
            let (title, channel) = it
                .snippet
                .map_or((None, None), |s| (s.title, s.channel_title));
            let video = Video {
                secs,
                title,
                channel,
            };
            (it.id, video)
        })
        .collect())
}

/// "PT3M45S" -> 225. Supports weeks, days, hours, minutes and seconds; years and
/// months are rejected because they don't apply to videos.
fn parse_iso8601(s: &str) -> Option<u64> {
    let rest = s.strip_prefix('P')?;
    let mut total = 0u64;
    let mut num = String::new();
    let mut in_time = false;
    for c in rest.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' => num.push(c),
            'D' | 'H' | 'M' | 'S' | 'W' => {
                let n: u64 = num.parse().ok()?;
                num.clear();
                total += n * match (c, in_time) {
                    ('W', false) => 604_800,
                    ('D', false) => 86_400,
                    ('H', true) => 3_600,
                    ('M', true) => 60,
                    ('S', true) => 1,
                    _ => return None,
                };
            }
            _ => return None,
        }
    }
    num.is_empty().then_some(total)
}

/// Reads the cache, in either format. A missing file is an empty cache, not an
/// error.
fn load_cache(path: &Path) -> Result<Cache> {
    if !path.exists() {
        return Ok(Cache::new());
    }
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let stored: BTreeMap<String, Stored> = serde_json::from_str(&text)
        .with_context(|| format!("{} is not a valid cache", path.display()))?;
    Ok(stored
        .into_iter()
        .map(|(id, s)| {
            let v = match s {
                Stored::Secs(secs) => Video {
                    secs,
                    ..Video::default()
                },
                Stored::Video(v) => v,
            };
            (id, v)
        })
        .collect())
}

/// Writes the cache as readable JSON.
fn save_cache(path: &Path, cache: &Cache) -> Result<()> {
    let text = serde_json::to_string_pretty(cache)?;
    fs::write(path, text).with_context(|| format!("cannot write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn iso8601_durations() {
        assert_eq!(parse_iso8601("PT3M45S"), Some(225));
        assert_eq!(parse_iso8601("PT45S"), Some(45));
        assert_eq!(parse_iso8601("PT1H2M3S"), Some(3723));
        assert_eq!(parse_iso8601("PT4M"), Some(240));
        assert_eq!(parse_iso8601("P0D"), Some(0));
        assert_eq!(parse_iso8601("P1DT1S"), Some(86_401));
        assert_eq!(parse_iso8601("basura"), None);
        assert_eq!(parse_iso8601("PT3"), None);
    }

    fn video(secs: Option<u64>, title: Option<&str>, channel: Option<&str>) -> Video {
        Video {
            secs,
            title: title.map(Into::into),
            channel: channel.map(Into::into),
        }
    }

    #[test]
    fn parses_api_response() {
        let json = r#"{"kind":"youtube#videoListResponse","items":[
            {"kind":"youtube#video","id":"aaaaaaaaaaa","contentDetails":{"duration":"PT3M30S"},
             "snippet":{"title":"DESPECHÁ","channelTitle":"ROSALÍA - Topic"}},
            {"kind":"youtube#video","id":"bbbbbbbbbbb","contentDetails":{"duration":"PT1H"}},
            {"kind":"youtube#video","id":"ccccccccccc","contentDetails":{"duration":"P1M"},
             "snippet":{"title":"Live","channelTitle":"X"}}
        ],"pageInfo":{"totalResults":3,"resultsPerPage":3}}"#;
        let found = parse_response(json).unwrap();
        assert_eq!(
            found["aaaaaaaaaaa"],
            video(Some(210), Some("DESPECHÁ"), Some("ROSALÍA - Topic"))
        );
        assert_eq!(found["bbbbbbbbbbb"], video(Some(3600), None, None));
        assert_eq!(found["ccccccccccc"], video(None, Some("Live"), Some("X")));
    }
    fn yt(id: &str) -> Play {
        Play {
            ts: Utc::now(),
            track: "T".into(),
            artist: "A".into(),
            album: None,
            ms_played: 210_000,
            estimated: true,
            source: Source::YouTubeMusic,
            video_id: Some(id.into()),
        }
    }

    #[test]
    fn applies_capped_length() {
        let mut plays = vec![
            yt("aaaaaaaaaaa"),
            yt("bbbbbbbbbbb"),
            yt("ccccccccccc"),
            yt("ddddddddddd"),
        ];
        let mut cache = Cache::new();
        cache.insert("aaaaaaaaaaa".into(), video(Some(180), None, None));
        cache.insert("bbbbbbbbbbb".into(), video(Some(7200), None, None)); // 2-hour mix
        cache.insert("ccccccccccc".into(), Video::default()); // deleted video
        let (resolved, _) = apply(&mut plays, &cache, 15 * 60_000);
        assert_eq!(resolved, 2);
        assert_eq!((plays[0].ms_played, plays[0].estimated), (180_000, false));
        assert_eq!((plays[1].ms_played, plays[1].estimated), (900_000, false));
        assert_eq!((plays[2].ms_played, plays[2].estimated), (210_000, true));
        assert_eq!((plays[3].ms_played, plays[3].estimated), (210_000, true));
    }

    #[test]
    fn api_names_replace_takeout_ones() {
        // Takeout in a language without a known prefix: the wording stays in the title.
        let mut p = yt("aaaaaaaaaaa");
        p.track = "Ha guardato Brillante".into();
        p.artist = "Repion".into();
        let mut plays = vec![p, yt("bbbbbbbbbbb"), yt("ccccccccccc"), yt("ddddddddddd")];
        let mut cache = Cache::new();
        cache.insert(
            "aaaaaaaaaaa".into(),
            video(Some(180), Some("Repion - Brillante"), Some("RepionVEVO")),
        );
        cache.insert(
            "bbbbbbbbbbb".into(),
            video(Some(180), Some("Song"), Some("  ")),
        );
        cache.insert("ccccccccccc".into(), video(None, Some(" Song "), None));
        let (resolved, named) = apply(&mut plays, &cache, 15 * 60_000);
        assert_eq!((resolved, named), (2, 1));
        assert_eq!(
            (plays[0].track.as_str(), plays[0].artist.as_str()),
            ("Brillante", "Repion")
        );
        // Missing or empty names: Takeout ones are kept.
        for p in &plays[1..] {
            assert_eq!((p.track.as_str(), p.artist.as_str()), ("T", "A"));
        }
    }

    #[test]
    fn retries_missing_lengths_and_titles() {
        let mut cache = Cache::new();
        cache.insert("aaaaaaaaaaa".into(), video(Some(200), Some("T"), Some("A")));
        cache.insert("bbbbbbbbbbb".into(), Video::default());
        cache.insert("ccccccccccc".into(), video(Some(200), None, None)); // old cache
        let ids: BTreeSet<&str> =
            ["aaaaaaaaaaa", "bbbbbbbbbbb", "ccccccccccc", "ddddddddddd"].into();
        assert_eq!(
            pending(&ids, &cache),
            vec!["bbbbbbbbbbb", "ccccccccccc", "ddddddddddd"]
        );
    }

    #[test]
    fn cache_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.json");
        let mut cache = Cache::new();
        cache.insert("aaaaaaaaaaa".into(), video(Some(200), Some("T"), Some("A")));
        cache.insert("ccccccccccc".into(), Video::default());
        save_cache(&path, &cache).unwrap();
        assert_eq!(load_cache(&path).unwrap(), cache);
    }

    #[test]
    fn reads_old_cache_format() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.json");
        fs::write(&path, r#"{"aaaaaaaaaaa": 200, "ccccccccccc": null}"#).unwrap();
        let cache = load_cache(&path).unwrap();
        assert_eq!(cache["aaaaaaaaaaa"], video(Some(200), None, None));
        assert_eq!(cache["ccccccccccc"], Video::default());
    }
}
