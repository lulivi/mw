//! Readers for each service's export, which all produce [`Play`]s.
//!
//! They read [`File`]s already in memory, so the same code serves the command
//! line (files read from disk with [`read_path`]) and the web page (files the
//! user added).

pub mod spotify;
pub mod ytmusic;

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::model::Play;

/// Result of loading a source: the plays and any non-fatal warnings.
#[derive(Default)]
pub struct Loaded {
    /// Plays read from the source.
    pub plays: Vec<Play>,
    /// Problems that didn't stop the load, such as a skipped file.
    pub warnings: Vec<String>,
}

/// What the files of one or more exports hold.
#[derive(Default)]
pub struct Found {
    /// Spotify plays: from the extended history when a file has it, else from
    /// the basic history (with a warning). `None` when no file has either.
    pub spotify: Option<Loaded>,
    /// YouTube Music plays from the YouTube history. `None` when no file has it.
    pub youtube: Option<Loaded>,
    /// Names of the files that hold none of them.
    pub unused: Vec<String>,
}

/// Reads every Spotify and YouTube history in `files`, telling them apart by
/// their contents; YouTube Music plays count as `avg_minutes` each.
///
/// Plays in more than one YouTube history (`watch-history.json` and
/// `My Activity.json` hold the same ones) count once. Fails when the YouTube
/// history is only there as HTML.
pub fn read(files: &[File], avg_minutes: f64) -> Result<Found> {
    let (mut extended, mut basic, mut html) = (vec![], vec![], None);
    let mut youtube: Option<Vec<Play>> = None;
    let mut unused = vec![];
    for f in files {
        let Ok(text) = f.text() else {
            unused.push(f.name.clone());
            continue;
        };
        if f.name.to_ascii_lowercase().ends_with(".html") {
            if ytmusic::is_html(text) {
                html.get_or_insert(&f.name);
            } else {
                unused.push(f.name.clone());
            }
        } else if let Some(plays) = spotify::parse_extended(text) {
            extended.extend(plays);
        } else if let Some(plays) = spotify::parse_basic(text) {
            basic.extend(plays);
        } else if let Some(plays) = ytmusic::parse(text, avg_minutes) {
            youtube.get_or_insert_default().extend(plays);
        } else {
            unused.push(f.name.clone());
        }
    }

    let spotify = if !extended.is_empty() {
        Some(Loaded {
            plays: extended,
            warnings: vec![],
        })
    } else if !basic.is_empty() {
        Some(Loaded {
            plays: basic,
            warnings: vec![
                "Spotify: using the basic history (last year only). Request the \"Extended streaming history\" to get everything."
                    .into(),
            ],
        })
    } else {
        None
    };

    let youtube = match youtube {
        Some(mut plays) => {
            // watch-history.json and My Activity.json hold the same plays.
            let mut seen = HashSet::new();
            plays.retain(|p| seen.insert((p.ts, p.video_id.clone(), p.track.clone())));
            let mut warnings = vec![];
            if plays.is_empty() {
                warnings.push("YouTube Music: the file has no YouTube Music entries.".into());
            }
            Some(Loaded { plays, warnings })
        }
        None => match html {
            Some(name) => return Err(ytmusic::html_error(name)),
            None => None,
        },
    };

    Ok(Found {
        spotify,
        youtube,
        unused,
    })
}

/// A file of an export, in memory.
pub struct File {
    /// Its path, as shown in messages; the last component is the file name.
    pub name: String,
    /// Its contents.
    pub bytes: Vec<u8>,
}

impl File {
    /// The file name, without the folders.
    pub fn file_name(&self) -> &str {
        self.name.rsplit(['/', '\\']).next().unwrap_or(&self.name)
    }

    /// The contents as text; a file that isn't UTF-8 fails with its name.
    pub fn text(&self) -> Result<&str> {
        std::str::from_utf8(&self.bytes).with_context(|| format!("cannot read {}", self.name))
    }
}

/// Reads a file, or every `.json` and `.html` file in a folder and its
/// subfolders, sorted by path.
pub fn read_path(path: &Path) -> Result<Vec<File>> {
    let read = |p: &Path| -> Result<File> {
        Ok(File {
            name: p.display().to_string(),
            bytes: fs::read(p).with_context(|| format!("cannot read {}", p.display()))?,
        })
    };
    if !path.is_dir() {
        return Ok(vec![read(path)?]);
    }
    let mut paths = vec![];
    let mut dirs = vec![path.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir).with_context(|| format!("cannot read {}", dir.display()))? {
            let p = entry?.path();
            if p.is_dir() {
                dirs.push(p);
            } else if p
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("json") || e.eq_ignore_ascii_case("html"))
            {
                paths.push(p);
            }
        }
    }
    paths.sort();
    paths.iter().map(|p| read(p)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, text: &str) -> File {
        File {
            name: name.into(),
            bytes: text.as_bytes().to_vec(),
        }
    }

    const EXTENDED: &str = r#"[{"ts":"2025-01-02T10:00:00Z","ms_played":200000,"master_metadata_track_name":"S","master_metadata_album_artist_name":"A"}]"#;
    const BASIC: &str =
        r#"[{"endTime":"2025-03-01 21:15","artistName":"A","trackName":"T","msPlayed":180000}]"#;
    const YOUTUBE: &str = r#"[{"header":"YouTube Music","title":"Watched T","titleUrl":"https://music.youtube.com/watch?v=dQw4w9WgXcQ","subtitles":[{"name":"A - Topic"}],"time":"2025-02-01T20:00:00Z"}]"#;

    #[test]
    fn tells_files_apart_by_contents() {
        let files = [
            file("x/mi historial.json", YOUTUBE),
            file("a.json", EXTENDED),
            file("b.json", BASIC),
            file("Userdata.json", r#"{"username":"u"}"#),
            file("empty.json", "[]"),
            file("other.json", r#"[{"header":"Search","title":"Searched"}]"#),
        ];
        let found = read(&files, 3.5).unwrap();
        let spotify = found.spotify.unwrap();
        assert_eq!(spotify.plays.len(), 1);
        assert_eq!(spotify.plays[0].track, "S", "the extended history wins");
        assert!(spotify.warnings.is_empty());
        assert_eq!(found.youtube.unwrap().plays[0].artist, "A");
        assert_eq!(found.unused, ["Userdata.json", "empty.json", "other.json"]);
    }

    #[test]
    fn basic_history_comes_with_a_warning() {
        let found = read(&[file("b.json", BASIC)], 3.5).unwrap();
        assert!(found.spotify.unwrap().warnings[0].contains("basic history"));
        assert!(found.youtube.is_none());
    }

    #[test]
    fn plays_in_both_youtube_histories_count_once() {
        let files = [
            file("watch-history.json", YOUTUBE),
            file("My Activity.json", YOUTUBE),
        ];
        assert_eq!(read(&files, 3.5).unwrap().youtube.unwrap().plays.len(), 1);
    }

    #[test]
    fn html_export_has_its_own_message() {
        let html =
            r#"<div class="outer-cell"><a href="https://www.youtube.com/watch?v=x">T</a></div>"#;
        let err = read(&[file("historial.html", html)], 3.5)
            .err()
            .unwrap()
            .to_string();
        assert!(err.starts_with("historial.html is HTML."), "{err}");

        // With the JSON too, the HTML is not needed.
        let found = read(&[file("h.html", html), file("w.json", YOUTUBE)], 3.5).unwrap();
        assert!(found.youtube.is_some());
    }
}
