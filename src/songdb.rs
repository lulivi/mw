//! The song database: real lengths of songs, shared in this repository
//! (`data/songs.json`) and downloaded on every run.
//!
//! The latest download is kept in the user's cache folder. When the download
//! fails the cached copy is used; with no copy either, YouTube Music plays keep
//! the estimate. Nothing here fails the run.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::model::{Play, Source};
use crate::normalize::track_key;

/// Where the database is downloaded from by default.
pub const DEFAULT_URL: &str = "https://raw.githubusercontent.com/lulivi/mw/main/data/songs.json";
/// Format version this program reads.
const VERSION: u32 = 1;
/// Most time a download may take before falling back to the cached copy.
const TIMEOUT: Duration = Duration::from_secs(10);

/// The database file; only the fields used here.
#[derive(Debug, Default, Deserialize)]
pub struct Db {
    version: u32,
    /// Song key ([`track_key`]) -> song.
    songs: BTreeMap<String, Song>,
}

/// One song in the database.
#[derive(Debug, Deserialize)]
struct Song {
    artist: String,
    title: String,
    seconds: u64,
    #[serde(default)]
    video_ids: Vec<String>,
}

/// Where the database is read from.
pub enum Origin<'a> {
    /// Downloaded, falling back to the cached copy.
    Url(&'a str),
    /// A local file; the cache is not touched.
    Path(&'a Path),
    /// Only the cached copy.
    Offline,
}

impl<'a> Origin<'a> {
    /// From `--db` and `--offline`: a value starting with `http://` or `https://`
    /// is a URL, anything else a path.
    pub fn new(db: Option<&'a str>, offline: bool) -> Self {
        match db {
            _ if offline => Origin::Offline,
            None => Origin::Url(DEFAULT_URL),
            Some(s) if s.starts_with("http://") || s.starts_with("https://") => Origin::Url(s),
            Some(s) => Origin::Path(Path::new(s)),
        }
    }
}

/// The cached copy: `<cache folder>/mw/songs.json`, if the system has a cache folder.
pub fn cache_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("mw").join("songs.json"))
}

/// Loads the database. `None` when it couldn't be read; the warnings say why.
pub fn load(origin: Origin, cache: Option<&Path>) -> (Option<Db>, Vec<String>) {
    let mut warnings = vec![];
    let url = match origin {
        Origin::Path(p) => {
            let db = read(p).map_err(|e| warnings.push(format!("song database: {e:#}")));
            return (db.ok(), warnings);
        }
        Origin::Url(url) => Some(url),
        Origin::Offline => None,
    };
    if let Some(url) = url {
        match download(url) {
            Ok((db, text)) => {
                if let Some(c) = cache
                    && let Err(e) = save(c, &text)
                {
                    warnings.push(format!("song database: could not save the copy ({e:#})"));
                }
                return (Some(db), warnings);
            }
            Err(e) => warnings.push(format!("song database: {e:#}")),
        }
    }
    match cache.filter(|c| c.exists()) {
        Some(c) => {
            let db = read(c).map_err(|e| warnings.push(format!("song database: {e:#}")));
            (db.ok(), warnings)
        }
        None => {
            warnings.push("song database: no cached copy; YouTube Music keeps the estimate".into());
            (None, warnings)
        }
    }
}

/// Downloads and parses the database, returning it with its text for the cache.
fn download(url: &str) -> Result<(Db, String)> {
    let text = ureq::get(url)
        .config()
        .timeout_global(Some(TIMEOUT))
        .build()
        .call()
        .and_then(|mut r| r.body_mut().read_to_string())
        .with_context(|| format!("could not download {url}"))?;
    let db = parse(&text).with_context(|| format!("{url} is not a valid song database"))?;
    Ok((db, text))
}

/// Reads and parses a database file.
fn read(path: &Path) -> Result<Db> {
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    parse(&text).with_context(|| format!("{} is not a valid song database", path.display()))
}

/// Parses the database, rejecting other format versions.
fn parse(text: &str) -> Result<Db> {
    let db: Db = serde_json::from_str(text)?;
    if db.version != VERSION {
        bail!(
            "version {} (this mw reads {VERSION}; update mw)",
            db.version
        );
    }
    Ok(db)
}

/// Writes the cached copy, creating its folder.
fn save(path: &Path, text: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, text).with_context(|| format!("cannot write {}", path.display()))
}

/// Gives YouTube Music plays their real length: by video ID (also taking the
/// database names), else by song key. Others keep the estimate. Returns how many
/// plays got a real length.
pub fn apply(plays: &mut [Play], db: &Db) -> usize {
    let by_video: HashMap<&str, &Song> = db
        .songs
        .values()
        .flat_map(|s| s.video_ids.iter().map(move |id| (id.as_str(), s)))
        .collect();
    let mut matched = 0;
    for p in plays
        .iter_mut()
        .filter(|p| p.source == Source::YouTubeMusic)
    {
        let song = match p.video_id.as_deref().and_then(|id| by_video.get(id)) {
            Some(s) => {
                p.track = s.title.clone();
                p.artist = s.artist.clone();
                s
            }
            None => match db.songs.get(&track_key(&p.artist, &p.track)) {
                Some(s) => s,
                None => continue,
            },
        };
        p.ms_played = song.seconds * 1000;
        p.estimated = false;
        matched += 1;
    }
    matched
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::collections::HashSet;

    fn yt(id: &str, artist: &str, track: &str) -> Play {
        Play {
            ts: Utc::now(),
            track: track.into(),
            artist: artist.into(),
            album: None,
            ms_played: 210_000,
            estimated: true,
            source: Source::YouTubeMusic,
            video_id: Some(id.into()),
        }
    }

    const DB: &str = r#"{"version": 1, "songs": {
        "rosalia\u001fdespecha": {"artist": "ROSALÍA", "title": "DESPECHÁ", "seconds": 157,
            "source": "musicbrainz", "mbid": "x", "video_ids": ["aaaaaaaaaaa"]},
        "repion\u001fbrillante": {"artist": "Repion", "title": "Brillante", "seconds": 200,
            "source": "manual"}
    }}"#;

    #[test]
    fn matches_by_video_id_then_key() {
        let db = parse(DB).unwrap();
        let mut plays = vec![
            yt("aaaaaaaaaaa", "Some Label", "Hai guardato DESPECHÁ"),
            yt("bbbbbbbbbbb", "RepionVEVO", "Brillante (Official Video)"),
            yt("ccccccccccc", "Nobody", "Unknown"),
        ];
        assert_eq!(apply(&mut plays, &db), 2);
        let got = |p: &Play| (p.artist.clone(), p.track.clone(), p.ms_played, p.estimated);
        // By video ID: length and names from the database.
        assert_eq!(
            got(&plays[0]),
            ("ROSALÍA".into(), "DESPECHÁ".into(), 157_000, false)
        );
        // By key: length only, Takeout names kept.
        assert_eq!(
            got(&plays[1]),
            (
                "RepionVEVO".into(),
                "Brillante (Official Video)".into(),
                200_000,
                false
            )
        );
        // Not found: the estimate.
        assert_eq!(
            got(&plays[2]),
            ("Nobody".into(), "Unknown".into(), 210_000, true)
        );
    }

    #[test]
    fn rejects_other_versions() {
        assert!(parse(r#"{"version": 2, "songs": {}}"#).is_err());
    }

    #[test]
    fn offline_uses_the_cached_copy() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("mw/songs.json");
        save(&cache, DB).unwrap();
        let (db, warnings) = load(Origin::Offline, Some(&cache));
        assert_eq!(db.unwrap().songs.len(), 2);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn offline_without_a_copy_keeps_the_estimate() {
        let dir = tempfile::tempdir().unwrap();
        let (db, warnings) = load(Origin::Offline, Some(&dir.path().join("songs.json")));
        assert!(db.is_none());
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn origin_from_options() {
        assert!(matches!(Origin::new(None, false), Origin::Url(DEFAULT_URL)));
        assert!(matches!(Origin::new(None, true), Origin::Offline));
        assert!(matches!(
            Origin::new(Some("https://x/s.json"), false),
            Origin::Url(_)
        ));
        assert!(matches!(
            Origin::new(Some("s.json"), false),
            Origin::Path(_)
        ));
    }

    /// The database in this repository: keys match their song, a video ID
    /// belongs to one song only, and every length is positive.
    #[test]
    fn repo_database_is_valid() {
        let db = parse(include_str!("../data/songs.json")).unwrap();
        let mut ids = HashSet::new();
        for (key, s) in &db.songs {
            assert_eq!(key, &track_key(&s.artist, &s.title), "{key:?}");
            assert!(s.seconds > 0, "{key:?}: seconds must be > 0");
            for id in &s.video_ids {
                assert!(ids.insert(id), "{key:?}: video ID {id} is in two songs");
            }
        }
    }
}
