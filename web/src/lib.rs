//! The web page's WebAssembly side: the `mw` library behind the calls the
//! page's worker makes. Files are parsed once, when added; each image is then
//! made from what they hold, like `mw` does from the command line.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, anyhow, bail};
use chrono::{Datelike, NaiveDate};
use chrono_tz::Tz;
use mw::model::{Play, Source};
use mw::render::{self, fmt_count, fmt_int};
use mw::sources::{self, Content, File};
use mw::stats::{self, Filters, RankBy};
use mw::{songdb, text};
use wasm_bindgen::prelude::*;

/// Minutes each YouTube Music play counts when the song database doesn't know
/// the song, as `mw`'s default `--ytm-avg-minutes`.
const AVG_MINUTES: f64 = 3.5;
/// Size of the top lists, as `mw`'s default `--top`.
const TOP: usize = 5;

/// The files the user added and the song database.
#[wasm_bindgen]
#[derive(Default)]
pub struct Session {
    /// What each file holds, by the id the page gave it.
    files: BTreeMap<u32, Vec<Content>>,
    /// The song database, once downloaded.
    db: Option<songdb::Db>,
}

/// An image and what goes with it.
#[wasm_bindgen(getter_with_clone)]
pub struct Recap {
    /// The image, as PNG.
    pub png: Vec<u8>,
    /// The recap as `mw` prints it.
    pub text: String,
    /// Warnings and notes on the sources, as `mw` prints them.
    pub notes: Vec<String>,
}

#[wasm_bindgen]
impl Session {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Session {
        Session::default()
    }

    /// Reads a file the user added (a ZIP, JSON or HTML file) and keeps what
    /// it holds under `id`. Returns a line for each history it holds, or one
    /// saying it holds none.
    pub fn add(&mut self, id: u32, name: String, bytes: Vec<u8>) -> Result<Vec<String>, JsError> {
        let contents = sources::parse(&File { name, bytes }, AVG_MINUTES).map_err(js)?;
        let lines = describe(&contents);
        self.files.insert(id, contents);
        Ok(lines)
    }

    /// Forgets the file added under `id`.
    pub fn remove(&mut self, id: u32) {
        self.files.remove(&id);
    }

    /// Whether any file added holds a YouTube history, which needs the song
    /// database.
    pub fn has_youtube(&self) -> bool {
        self.contents().any(|c| matches!(c, Content::YouTube(_)))
    }

    /// Sets the song database from the text of `songs.json`.
    pub fn set_db(&mut self, text: &str) -> Result<(), JsError> {
        let db = songdb::parse(text).context("the song database is not valid");
        self.db = Some(db.map_err(js)?);
        Ok(())
    }

    /// The years with plays in the time zone `tz`, oldest first. Fails as
    /// [`Session::create`] does when the files can't be used together.
    pub fn years(&self, tz: &str) -> Result<Vec<i32>, JsError> {
        let tz = time_zone(tz).map_err(js)?;
        let found = sources::combine(self.contents()).map_err(js)?;
        let years: BTreeSet<i32> = [found.spotify, found.youtube]
            .into_iter()
            .flatten()
            .flat_map(|l| l.plays)
            .map(|p| p.ts.with_timezone(&tz).year())
            .collect();
        Ok(years.into_iter().collect())
    }

    /// Makes the image from the files added, with `mw`'s options: `source`
    /// (`all`, `spotify` or `ytm`), `from` and `to` (`YYYY-MM-DD`, or empty for
    /// no limit), `rank_by` (`plays` or `minutes`), `color` and `tz`.
    pub fn create(
        &self,
        source: &str,
        from: &str,
        to: &str,
        rank_by: &str,
        color: &str,
        tz: &str,
    ) -> Result<Recap, JsError> {
        self.recap(source, from, to, rank_by, color, tz).map_err(js)
    }
}

impl Session {
    /// What every file added holds.
    fn contents(&self) -> impl Iterator<Item = &Content> {
        self.files.values().flatten()
    }

    /// [`Session::create`], with Rust errors.
    fn recap(
        &self,
        source: &str,
        from: &str,
        to: &str,
        rank_by: &str,
        color: &str,
        tz: &str,
    ) -> Result<Recap> {
        let found = sources::combine(self.contents())?;
        let source = match source {
            "all" => None,
            "spotify" => Some(Source::Spotify),
            "ytm" => Some(Source::YouTubeMusic),
            _ => bail!("unknown source {source}"),
        };
        if found.spotify.is_none() && found.youtube.is_none() {
            bail!("Add your Spotify or YouTube history first.");
        }
        if source == Some(Source::Spotify) && found.spotify.is_none() {
            bail!("Only Spotify needs your Spotify history: add it, or choose all sources.");
        }
        if source == Some(Source::YouTubeMusic) && found.youtube.is_none() {
            bail!("Only YouTube Music needs your YouTube history: add it, or choose all sources.");
        }
        let from = date(from)?;
        let to = date(to)?;
        if let (Some(f), Some(t)) = (from, to)
            && f > t
        {
            bail!("The start date ({f}) is after the end date ({t}).");
        }
        let rank_by = match rank_by {
            "plays" => RankBy::Plays,
            "minutes" => RankBy::Minutes,
            _ => bail!("unknown ranking {rank_by}"),
        };
        let accent = render::accent(color).ok_or_else(|| anyhow!("unknown color {color}"))?;
        let tz = time_zone(tz)?;

        let mut plays: Vec<Play> = vec![];
        let mut notes = vec![];
        if let Some(loaded) = found
            .spotify
            .filter(|_| source != Some(Source::YouTubeMusic))
        {
            notes.extend(loaded.warnings);
            plays.extend(loaded.plays);
        }
        if let Some(mut loaded) = found.youtube.filter(|_| source != Some(Source::Spotify)) {
            notes.extend(loaded.warnings);
            match &self.db {
                Some(db) => {
                    let matched = songdb::apply(&mut loaded.plays, db);
                    notes.push(format!(
                        "YouTube Music: song length for {} of {} from the song database",
                        fmt_int(matched as u64),
                        fmt_count(loaded.plays.len() as u64, "play")
                    ));
                }
                None => notes.push(
                    "song database: could not be downloaded; YouTube Music keeps the estimate"
                        .into(),
                ),
            }
            plays.extend(loaded.plays);
        }

        let filters = Filters {
            from,
            to,
            source,
            tz,
        };
        let Some(stats) = stats::compute(&plays, &filters, TOP, rank_by) else {
            bail!("No plays in that date range.");
        };
        let svg = render::build_svg(&stats, rank_by, accent)?;
        Ok(Recap {
            png: render::png(&svg)?,
            text: text::stats(&stats),
            notes,
        })
    }
}

/// One line per history in a file's contents, or one saying it has none.
fn describe(contents: &[Content]) -> Vec<String> {
    let mut extended = None;
    let mut basic = None;
    let mut youtube = None;
    let mut html = false;
    for c in contents {
        match c {
            Content::Extended(p) => *extended.get_or_insert(0) += p.len(),
            Content::Basic(p) => *basic.get_or_insert(0) += p.len(),
            Content::YouTube(p) => *youtube.get_or_insert(0) += p.len(),
            Content::Html(_) => html = true,
            Content::Unused(_) => {}
        }
    }
    let count = |n: usize, noun: &str| fmt_count(n as u64, noun);
    let mut lines = vec![];
    if let Some(n) = extended {
        lines.push(format!("Spotify extended history: {}", count(n, "play")));
    }
    if let Some(n) = basic {
        lines.push(format!("Spotify basic history: {}", count(n, "play")));
    }
    if let Some(n) = youtube {
        lines.push(format!(
            "YouTube history: {}",
            count(n, "YouTube Music play")
        ));
    }
    if html {
        lines.push("YouTube history as HTML: can't be read, export it as JSON".into());
    }
    if lines.is_empty() {
        lines.push("Not used: no Spotify or YouTube history in it".into());
    }
    lines
}

/// A `YYYY-MM-DD` date; empty means none.
fn date(s: &str) -> Result<Option<NaiveDate>> {
    if s.is_empty() {
        return Ok(None);
    }
    let d =
        NaiveDate::parse_from_str(s, "%Y-%m-%d").with_context(|| format!("invalid date {s}"))?;
    Ok(Some(d))
}

/// A time zone by its IANA name, such as `Europe/Madrid`.
fn time_zone(s: &str) -> Result<Tz> {
    s.parse().map_err(|_| anyhow!("unknown time zone {s}"))
}

/// A Rust error for JavaScript, with its causes.
fn js(e: anyhow::Error) -> JsError {
    JsError::new(&format!("{e:#}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const YOUTUBE: &str = r#"[{"header":"YouTube Music","title":"Watched T","titleUrl":"https://music.youtube.com/watch?v=dQw4w9WgXcQ","subtitles":[{"name":"A - Topic"}],"time":"2025-02-01T20:00:00Z"}]"#;

    #[test]
    fn says_what_files_hold_and_makes_the_image() {
        let mut s = Session::new();
        let lines = s.add(0, "h.json".into(), YOUTUBE.into()).ok().unwrap();
        assert_eq!(lines, ["YouTube history: 1 YouTube Music play"]);
        let lines = s
            .add(1, "Userdata.json".into(), b"{}".to_vec())
            .ok()
            .unwrap();
        assert_eq!(lines, ["Not used: no Spotify or YouTube history in it"]);
        assert!(s.has_youtube());

        let err = s
            .recap("spotify", "", "", "plays", "orange", "UTC")
            .err()
            .unwrap();
        assert!(err.to_string().starts_with("Only Spotify needs"), "{err}");
        let err = s.recap("all", "2025-03-01", "2025-02-01", "plays", "orange", "UTC");
        assert!(err.err().unwrap().to_string().contains("is after"));

        let r = s.recap("all", "", "", "plays", "orange", "UTC").unwrap();
        assert_eq!(&r.png[1..4], b"PNG");
        assert!(r.text.contains("Minutes listened"));
        assert!(r.notes[0].contains("could not be downloaded"));

        s.remove(0);
        let err = s
            .recap("all", "", "", "plays", "orange", "UTC")
            .err()
            .unwrap();
        assert!(err.to_string().contains("first"), "{err}");
    }
}
