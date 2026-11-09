//! The song database: song lengths, shared in this repository
//! (`data/songs.json`) and checked for changes on every run.
//!
//! The latest download is kept in the user's cache folder with its ETag, so it
//! is only downloaded again when it changed. When the download fails the cached
//! copy is used; with no copy either, YouTube Music plays keep
//! the estimate. Nothing here fails the run.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::lengths::{SongLength, most_common};
use crate::model::{Play, Source};
use crate::normalize::{clean_title, track_key};

/// Where the database is downloaded from by default.
pub const DEFAULT_URL: &str = "https://raw.githubusercontent.com/lulivi/mw/main/data/songs.json";
/// Format version this program reads.
const VERSION: u32 = 1;
/// Song lengths an import accepts, in seconds.
pub const IMPORT_RANGE: std::ops::RangeInclusive<u64> = 30..=1200;
/// `check` of a song whose length nobody knows yet.
pub const NO_LENGTH: &str = "no length";
/// Most time a download may take before falling back to the cached copy.
const TIMEOUT: Duration = Duration::from_secs(10);

/// The database file; only the fields used here.
#[derive(Debug, Default, Deserialize)]
pub struct Db {
    version: u32,
    /// Song key ([`track_key`]) -> song.
    songs: BTreeMap<String, Song>,
}

impl Db {
    /// Whether the database has the song with this key ([`track_key`]).
    pub fn contains(&self, key: &str) -> bool {
        self.songs.contains_key(key)
    }
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
    let existing = cache.filter(|c| c.exists());
    let cached = existing.and_then(|c| {
        read(c)
            .map_err(|e| warnings.push(format!("song database: {e:#}")))
            .ok()
    });
    if let Some(url) = url {
        // Only a readable copy may be kept by a 304; otherwise download it in full.
        let etag = existing
            .filter(|_| cached.is_some())
            .and_then(|c| fs::read_to_string(etag_path(c)).ok());
        match download(url, etag.as_deref()) {
            Ok(Download::NotModified) => return (cached, warnings),
            Ok(Download::New { db, text, etag }) => {
                if let Some(c) = cache
                    && let Err(e) = save_with_etag(c, &text, etag.as_deref())
                {
                    warnings.push(format!("song database: could not save the copy ({e:#})"));
                }
                return (Some(db), warnings);
            }
            Err(e) => warnings.push(format!("song database: {e:#}")),
        }
    }
    if existing.is_none() {
        warnings.push("song database: no cached copy; YouTube Music keeps the estimate".into());
    }
    (cached, warnings)
}

/// What a download got.
enum Download {
    /// The server's copy is the cached one (`304 Not Modified`).
    NotModified,
    /// A new copy, with its text for the cache and its ETag.
    New {
        db: Db,
        text: String,
        etag: Option<String>,
    },
}

/// Downloads and parses the database. With `etag` (the cached copy's), the
/// server may answer that the copy is still current.
fn download(url: &str, etag: Option<&str>) -> Result<Download> {
    let mut req = ureq::get(url);
    if let Some(etag) = etag {
        req = req.header("If-None-Match", etag);
    }
    let mut resp = req
        .config()
        .timeout_global(Some(TIMEOUT))
        .build()
        .call()
        .with_context(|| format!("could not download {url}"))?;
    if resp.status() == 304 {
        return Ok(Download::NotModified);
    }
    let etag = resp
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let text = resp
        .body_mut()
        .read_to_string()
        .with_context(|| format!("could not download {url}"))?;
    let db = parse(&text).with_context(|| format!("{url} is not a valid song database"))?;
    Ok(Download::New { db, text, etag })
}

/// Where the cached copy's ETag is kept: next to it, as `songs.json.etag`.
fn etag_path(cache: &Path) -> PathBuf {
    let mut name = cache.as_os_str().to_owned();
    name.push(".etag");
    PathBuf::from(name)
}

/// Saves the cached copy and its ETag (removing a stale one when there is none).
fn save_with_etag(cache: &Path, text: &str, etag: Option<&str>) -> Result<()> {
    save(cache, text)?;
    let path = etag_path(cache);
    match etag {
        Some(etag) => {
            fs::write(&path, etag).with_context(|| format!("cannot write {}", path.display()))?
        }
        None => {
            let _ = fs::remove_file(path);
        }
    }
    Ok(())
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

/// Finds the song of a play: by video ID (`true`: the play takes the database
/// names), else by song key.
fn find<'a>(by_video: &HashMap<&str, &'a Song>, db: &'a Db, p: &Play) -> Option<(&'a Song, bool)> {
    match p.video_id.as_deref().and_then(|id| by_video.get(id)) {
        Some(s) => Some((s, true)),
        None => db
            .songs
            .get(&track_key(&p.artist, &p.track))
            .map(|s| (s, false)),
    }
}

/// Video ID -> song.
fn by_video(db: &Db) -> HashMap<&str, &Song> {
    db.songs
        .values()
        .flat_map(|s| s.video_ids.iter().map(move |id| (id.as_str(), s)))
        .collect()
}

/// Gives YouTube Music plays their song length: by video ID (also taking the
/// database names), else by song key. Others keep the estimate. Returns how many
/// plays got a song length.
pub fn apply(plays: &mut [Play], db: &Db) -> usize {
    let by_video = by_video(db);
    let mut matched = 0;
    for p in plays
        .iter_mut()
        .filter(|p| p.source == Source::YouTubeMusic)
    {
        let Some((song, by_id)) = find(&by_video, db, p) else {
            continue;
        };
        if by_id {
            p.track = song.title.clone();
            p.artist = song.artist.clone();
        }
        p.ms_played = song.seconds * 1000;
        p.estimated = false;
        matched += 1;
    }
    matched
}

/// The YouTube Music songs the database doesn't have (found as [`apply`] does),
/// one per song key, sorted by it: names in their most frequent spelling with
/// the title cleaned, every video ID, and a `check` asking for the length.
pub fn missing(plays: &[Play], db: Option<&Db>) -> Vec<SongLength> {
    let empty = Db::default();
    let db = db.unwrap_or(&empty);
    let by_video = by_video(db);
    let mut by_key: BTreeMap<String, Vec<&Play>> = BTreeMap::new();
    for p in plays.iter().filter(|p| p.source == Source::YouTubeMusic) {
        if find(&by_video, db, p).is_none() {
            by_key
                .entry(track_key(&p.artist, &p.track))
                .or_default()
                .push(p);
        }
    }
    by_key
        .into_values()
        .map(|plays| {
            let (artist, title) =
                most_common(plays.iter().map(|p| (&p.artist, &p.track))).expect("a song has plays");
            let video_ids: BTreeSet<&String> =
                plays.iter().filter_map(|p| p.video_id.as_ref()).collect();
            SongLength {
                artist: artist.clone(),
                title: clean_title(title),
                video_ids: video_ids.into_iter().cloned().collect(),
                check: Some(NO_LENGTH.into()),
                seconds: None,
                ..Default::default()
            }
        })
        .collect()
}

/// What [`import`] did.
#[derive(Debug, Default)]
pub struct Imported {
    /// Songs added (or that would be, on a dry run).
    pub added: Vec<SongLength>,
    /// Songs whose key the database already had.
    pub present: usize,
    /// Songs skipped for a length outside [`IMPORT_RANGE`].
    pub out_of_range: Vec<SongLength>,
    /// Songs skipped for an empty artist or title.
    pub unnamed: usize,
    /// Songs skipped because they still have a `check`.
    pub to_check: usize,
}

/// Adds the songs of a contribution file to the database file at `path`, with
/// their `source` (by default `manual` for songs with video IDs, else
/// `spotify`), `mbid` and `video_ids`. Never changes a song already there, and
/// skips the songs that still have a `check`; the file's other songs and fields
/// are kept. Fails, writing nothing, on a song without `check` or length, or
/// when a video ID would be in two songs. With `dry_run` nothing is written.
pub fn import(path: &Path, songs: Vec<SongLength>, dry_run: bool) -> Result<Imported> {
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let db =
        parse(&text).with_context(|| format!("{} is not a valid song database", path.display()))?;
    let mut doc: Value = serde_json::from_str(&text)?;
    let entries = doc["songs"]
        .as_object_mut()
        .context("\"songs\" is not an object")?;
    // Video ID -> "artist · title" of the song that has it.
    let mut owners: HashMap<String, String> = db
        .songs
        .values()
        .flat_map(|s| {
            let name = format!("{} · {}", s.artist, s.title);
            s.video_ids.iter().map(move |id| (id.clone(), name.clone()))
        })
        .collect();
    let mut conflicts = vec![];

    let mut out = Imported::default();
    for s in songs {
        if s.check.is_some() {
            out.to_check += 1;
            continue;
        }
        let Some(seconds) = s.seconds else {
            bail!(
                "{} · {} has no length: set \"seconds\" or put back a \"check\"",
                s.artist,
                s.title
            );
        };
        let (artist, title) = (s.artist.trim(), s.title.trim());
        if artist.is_empty() || title.is_empty() {
            out.unnamed += 1;
            continue;
        }
        let key = track_key(artist, title);
        if entries.contains_key(&key) {
            out.present += 1;
        } else if !IMPORT_RANGE.contains(&seconds) {
            out.out_of_range.push(s);
        } else {
            let name = format!("{artist} · {title}");
            for id in &s.video_ids {
                match owners.get(id) {
                    Some(owner) if *owner != name => {
                        conflicts.push(format!("video ID {id}: {owner} and {name}"))
                    }
                    _ => {
                        owners.insert(id.clone(), name.clone());
                    }
                }
            }
            let mut entry = json!({"artist": artist, "title": title});
            if let Some(album) = s.album.as_deref().map(str::trim).filter(|a| !a.is_empty()) {
                entry["album"] = album.into();
            }
            entry["seconds"] = seconds.into();
            let source = match (&s.source, s.video_ids.is_empty()) {
                (Some(source), _) => source.as_str(),
                (None, true) => "spotify",
                (None, false) => "manual",
            };
            entry["source"] = source.into();
            if let Some(mbid) = &s.mbid {
                entry["mbid"] = mbid.as_str().into();
            }
            if !s.video_ids.is_empty() {
                entry["video_ids"] = s.video_ids.clone().into();
            }
            entries.insert(key, entry);
            out.added.push(s);
        }
    }
    if !conflicts.is_empty() {
        bail!(
            "a video ID belongs to one song only; fix these and import again:\n  {}",
            conflicts.join("\n  ")
        );
    }
    entries.sort_keys();
    if !dry_run && !out.added.is_empty() {
        save(path, &(serde_json::to_string_pretty(&doc)? + "\n"))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::collections::HashSet;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

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
        "rosalia|despecha": {"artist": "ROSALÍA", "title": "DESPECHÁ", "seconds": 157,
            "source": "musicbrainz", "mbid": "x", "video_ids": ["aaaaaaaaaaa"]},
        "repion|brillante": {"artist": "Repion", "title": "Brillante", "seconds": 200,
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
    fn missing_leaves_out_known_songs() {
        let db = parse(DB).unwrap();
        let plays = vec![
            yt("aaaaaaaaaaa", "Some Label", "Anything"), // known by video ID
            yt("bbbbbbbbbbb", "Repion", "Brillante"),    // known by key
            yt("ccccccccccc", "Nobody", "Unknown (Official Video)"),
            yt("ddddddddddd", "Nobody", "Unknown"),
            yt("ccccccccccc", "Nobody", "Unknown (Official Video)"),
        ];
        let json = serde_json::to_value(missing(&plays, Some(&db))).unwrap();
        assert_eq!(
            json,
            json!([{"artist": "Nobody", "title": "Unknown",
                "video_ids": ["ccccccccccc", "ddddddddddd"], "check": "no length", "seconds": null}])
        );
        assert_eq!(missing(&plays, None).len(), 3);
    }

    fn length(artist: &str, title: &str, seconds: u64) -> SongLength {
        SongLength {
            artist: artist.into(),
            title: title.into(),
            album: Some("Album".into()),
            seconds: Some(seconds),
            ..Default::default()
        }
    }

    #[test]
    fn import_adds_new_songs_and_keeps_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("songs.json");
        save(&file, DB).unwrap();
        let got = import(
            &file,
            vec![
                length("Repion", "Brillante", 999), // already there: not overwritten
                length("A", "New", 200),
                length("A", "Short", 29),
                length("A", "Long", 1201),
                length(" ", "No artist", 200),
                SongLength {
                    check: Some("one full play".into()),
                    ..length("A", "Unchecked", 200)
                },
            ],
            false,
        )
        .unwrap();
        assert_eq!(got.added.len(), 1);
        assert_eq!(got.present, 1);
        let skipped: Vec<_> = got.out_of_range.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(skipped, ["Short", "Long"]);
        assert_eq!(got.unnamed, 1);
        assert_eq!(got.to_check, 1);

        let doc: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(
            doc["songs"]["a|new"],
            json!({"artist": "A", "title": "New", "album": "Album", "seconds": 200, "source": "spotify"})
        );
        assert_eq!(doc["songs"]["repion|brillante"]["seconds"], 200);
        assert_eq!(doc["songs"]["rosalia|despecha"]["mbid"], "x");
        assert_eq!(doc["songs"].as_object().unwrap().len(), 3);
        parse(&fs::read_to_string(&file).unwrap()).unwrap();
    }

    #[test]
    fn import_rejects_no_length_without_check() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("songs.json");
        save(&file, DB).unwrap();
        let unknown = SongLength {
            seconds: None,
            ..length("A", "Unknown", 0)
        };
        let err = import(&file, vec![unknown], false).unwrap_err();
        assert!(err.to_string().contains("A · Unknown"), "{err}");
        assert_eq!(fs::read_to_string(&file).unwrap(), DB);
    }

    #[test]
    fn import_adds_source_mbid_and_video_ids() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("songs.json");
        save(&file, DB).unwrap();
        import(
            &file,
            vec![
                SongLength {
                    video_ids: vec!["ddddddddddd".into()],
                    mbid: Some("m".into()),
                    source: Some("musicbrainz".into()),
                    ..length("A", "Resolved", 200)
                },
                SongLength {
                    video_ids: vec!["eeeeeeeeeee".into()],
                    ..length("A", "Typed", 210)
                },
            ],
            false,
        )
        .unwrap();
        let doc: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(
            doc["songs"]["a|resolved"],
            json!({"artist": "A", "title": "Resolved", "album": "Album", "seconds": 200,
                "source": "musicbrainz", "mbid": "m", "video_ids": ["ddddddddddd"]})
        );
        assert_eq!(doc["songs"]["a|typed"]["source"], "manual");
        parse(&fs::read_to_string(&file).unwrap()).unwrap();
    }

    #[test]
    fn import_rejects_a_video_id_in_two_songs() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("songs.json");
        save(&file, DB).unwrap();
        let with_id = |title, id: &str| SongLength {
            video_ids: vec![id.into()],
            ..length("A", title, 200)
        };
        for dry_run in [false, true] {
            let err = import(
                &file,
                vec![
                    with_id("Taken", "aaaaaaaaaaa"), // ROSALÍA · DESPECHÁ has it
                    with_id("First", "fffffffffff"),
                    with_id("Second", "fffffffffff"),
                ],
                dry_run,
            )
            .unwrap_err()
            .to_string();
            assert!(
                err.contains("video ID aaaaaaaaaaa: ROSALÍA · DESPECHÁ and A · Taken"),
                "{err}"
            );
            assert!(
                err.contains("video ID fffffffffff: A · First and A · Second"),
                "{err}"
            );
            assert_eq!(fs::read_to_string(&file).unwrap(), DB);
        }
    }

    #[test]
    fn import_dry_run_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("songs.json");
        save(&file, DB).unwrap();
        let got = import(&file, vec![length("A", "New", 200)], true).unwrap();
        assert_eq!(got.added.len(), 1);
        assert_eq!(fs::read_to_string(&file).unwrap(), DB);
    }

    #[test]
    fn rejects_other_versions() {
        assert!(parse(r#"{"version": 2, "songs": {}}"#).is_err());
    }

    /// Serves `body` with ETag `"v1"` on a local port, answering `304` to a
    /// matching `If-None-Match`. Returns the URL and how many full answers it sent.
    fn serve(body: &'static str) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/songs.json", listener.local_addr().unwrap());
        let full = Arc::new(AtomicUsize::new(0));
        let counter = full.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut buf = [0; 4096];
                let n = stream.read(&mut buf).unwrap();
                let request = String::from_utf8_lossy(&buf[..n]).to_lowercase();
                let response = if request.contains("if-none-match: \"v1\"") {
                    "HTTP/1.1 304 Not Modified\r\nETag: \"v1\"\r\nConnection: close\r\n\r\n"
                        .to_string()
                } else {
                    counter.fetch_add(1, Ordering::SeqCst);
                    format!(
                        "HTTP/1.1 200 OK\r\nETag: \"v1\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                };
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        (url, full)
    }

    #[test]
    fn downloads_only_when_changed() {
        let (url, full) = serve(DB);
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("mw/songs.json");

        // No copy: downloaded in full and saved with its ETag.
        let (db, warnings) = load(Origin::Url(&url), Some(&cache));
        assert_eq!(db.unwrap().songs.len(), 2, "{warnings:?}");
        assert_eq!(fs::read_to_string(&cache).unwrap(), DB);
        assert_eq!(fs::read_to_string(etag_path(&cache)).unwrap(), "\"v1\"");

        // Same ETag: 304, the cached copy is used (changed here to tell them apart).
        save(&cache, r#"{"version": 1, "songs": {}}"#).unwrap();
        let (db, warnings) = load(Origin::Url(&url), Some(&cache));
        assert_eq!(db.unwrap().songs.len(), 0, "{warnings:?}");
        assert_eq!(full.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn unreadable_copy_is_downloaded_again() {
        let (url, full) = serve(DB);
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("songs.json");
        save(&cache, "not json").unwrap();
        fs::write(etag_path(&cache), "\"v1\"").unwrap();

        let (db, _) = load(Origin::Url(&url), Some(&cache));
        assert_eq!(db.unwrap().songs.len(), 2);
        assert_eq!(full.load(Ordering::SeqCst), 1);
        assert_eq!(fs::read_to_string(&cache).unwrap(), DB);
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
    /// belongs to one song only, every length is positive, and the file is
    /// laid out as the import commands write it (sorted, pretty-printed).
    #[test]
    fn repo_database_is_valid() {
        let text = include_str!("../data/songs.json");
        let mut doc: serde_json::Value = serde_json::from_str(text).unwrap();
        doc["songs"].as_object_mut().unwrap().sort_keys();
        let canonical = serde_json::to_string_pretty(&doc).unwrap() + "\n";
        assert!(
            text == canonical,
            "data/songs.json: run an import to reformat it"
        );
        let db = parse(text).unwrap();
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
