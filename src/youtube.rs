//! Real length of YouTube Music songs via the official YouTube API
//! (YouTube Data API v3, `videos.list`).
//!
//! Optional: only used when an API key is provided. Each request asks for up to 50
//! videos and costs 1 unit of a free 10,000-unit daily quota. Results are stored in
//! a JSON cache: videos with a known length are never fetched again. Those left
//! without one (`null`) are retried on every run with a key, in case it was a
//! transient failure; a deleted video only costs its share of one request.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

use crate::model::{Play, Source};

/// Endpoint of the YouTube Data API `videos.list` method.
const API_URL: &str = "https://www.googleapis.com/youtube/v3/videos";
/// Most videos the API accepts in one request.
const BATCH: usize = 50;

/// Video ID -> seconds. `None` = the video no longer exists or has no length.
type Cache = BTreeMap<String, Option<u64>>;

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
}

/// The `contentDetails` part of an [`Item`].
#[derive(Deserialize)]
struct ContentDetails {
    /// Length in ISO 8601, such as "PT3M45S".
    duration: Option<String>,
}

/// Fetches the lengths missing from the cache and applies them to the YouTube Music
/// plays. Network or quota failures don't stop the program: a warning is reported
/// and songs without a length keep the estimate.
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
            Ok(found) => {
                report.requests += 1;
                for id in chunk {
                    let secs = found.get(id).copied();
                    if secs.is_some() {
                        report.fetched += 1;
                    }
                    cache.insert(id.clone(), secs);
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
    report.resolved = apply(plays, &cache, max_ms);
    report
}

/// IDs to fetch: those not in the cache or cached without a length.
fn pending(ids: &BTreeSet<&str>, cache: &Cache) -> Vec<String> {
    ids.iter()
        .filter(|id| !matches!(cache.get(**id), Some(Some(_))))
        .map(|id| id.to_string())
        .collect()
}

/// Replaces the estimate with the real length (capped). Returns how many distinct
/// videos have a length.
fn apply(plays: &mut [Play], cache: &Cache, max_ms: u64) -> usize {
    let mut resolved = BTreeSet::new();
    for p in plays
        .iter_mut()
        .filter(|p| p.source == Source::YouTubeMusic)
    {
        let Some(id) = p.video_id.as_deref() else {
            continue;
        };
        if let Some(Some(secs)) = cache.get(id)
            && *secs > 0
        {
            p.ms_played = (secs * 1000).min(max_ms);
            p.estimated = false;
            resolved.insert(id.to_string());
        }
    }
    resolved.len()
}

/// Asks the API for the lengths of up to [`BATCH`] videos. Returns video ID ->
/// seconds; videos it doesn't know about are simply missing from the result.
///
/// Fails on connection problems and non-success responses, such as an invalid key
/// or an exhausted quota.
fn fetch(key: &str, ids: &[String]) -> Result<HashMap<String, u64>> {
    let mut resp = ureq::get(API_URL)
        .query("part", "contentDetails")
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

/// Extracts video ID -> seconds from a response body, skipping videos without a
/// readable length.
fn parse_response(text: &str) -> Result<HashMap<String, u64>> {
    let resp: Response = serde_json::from_str(text).context("unexpected response")?;
    Ok(resp
        .items
        .into_iter()
        .filter_map(|it| {
            let secs = parse_iso8601(&it.content_details?.duration?)?;
            Some((it.id, secs))
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

/// Reads the cache. A missing file is an empty cache, not an error.
fn load_cache(path: &Path) -> Result<Cache> {
    if !path.exists() {
        return Ok(Cache::new());
    }
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("{} is not a valid cache", path.display()))
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

    #[test]
    fn parses_api_response() {
        let json = r#"{"kind":"youtube#videoListResponse","items":[
            {"kind":"youtube#video","id":"aaaaaaaaaaa","contentDetails":{"duration":"PT3M30S"}},
            {"kind":"youtube#video","id":"bbbbbbbbbbb","contentDetails":{"duration":"PT1H"}}
        ],"pageInfo":{"totalResults":2,"resultsPerPage":2}}"#;
        let found = parse_response(json).unwrap();
        assert_eq!(found["aaaaaaaaaaa"], 210);
        assert_eq!(found["bbbbbbbbbbb"], 3600);
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
        cache.insert("aaaaaaaaaaa".into(), Some(180));
        cache.insert("bbbbbbbbbbb".into(), Some(7200)); // 2-hour mix
        cache.insert("ccccccccccc".into(), None); // deleted video
        let resolved = apply(&mut plays, &cache, 15 * 60_000);
        assert_eq!(resolved, 2);
        assert_eq!((plays[0].ms_played, plays[0].estimated), (180_000, false));
        assert_eq!((plays[1].ms_played, plays[1].estimated), (900_000, false));
        assert_eq!((plays[2].ms_played, plays[2].estimated), (210_000, true));
        assert_eq!((plays[3].ms_played, plays[3].estimated), (210_000, true));
    }

    #[test]
    fn retries_nulls() {
        let mut cache = Cache::new();
        cache.insert("aaaaaaaaaaa".into(), Some(200));
        cache.insert("bbbbbbbbbbb".into(), None);
        let ids: BTreeSet<&str> = ["aaaaaaaaaaa", "bbbbbbbbbbb", "ccccccccccc"].into();
        assert_eq!(pending(&ids, &cache), vec!["bbbbbbbbbbb", "ccccccccccc"]);
    }

    #[test]
    fn cache_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.json");
        let mut cache = Cache::new();
        cache.insert("aaaaaaaaaaa".into(), Some(200));
        cache.insert("ccccccccccc".into(), None);
        save_cache(&path, &cache).unwrap();
        assert_eq!(load_cache(&path).unwrap(), cache);
    }
}
