//! Readers for each service's export, which all produce [`Play`]s.
//!
//! They read [`File`]s already in memory, so the same code serves the command
//! line (files read from disk with [`read_path`]) and the web page (files the
//! user added). Files are told apart by their contents, and ZIPs are opened.

pub mod spotify;
pub mod ytmusic;

use std::collections::HashSet;
use std::fs;
use std::io::Read;
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

/// What one file holds.
pub enum Content {
    /// Spotify's extended history.
    Extended(Vec<Play>),
    /// Spotify's basic history.
    Basic(Vec<Play>),
    /// The YouTube history, as JSON.
    YouTube(Vec<Play>),
    /// The YouTube history as HTML, which can't be read; the file's name.
    Html(String),
    /// None of them; the file's name.
    Unused(String),
}

/// Reads every Spotify and YouTube history in `files`, as [`parse`] and
/// [`combine`] do.
pub fn read(files: &[File], avg_minutes: f64) -> Result<Found> {
    let mut contents = vec![];
    for f in files {
        contents.extend(parse(f, avg_minutes)?);
    }
    combine(&contents)
}

/// Tells what a file holds by its contents, not its name; YouTube Music plays
/// count as `avg_minutes` each. A ZIP gives what each `.json` and `.html` file
/// in it holds. Fails only on a ZIP that can't be opened.
pub fn parse(f: &File, avg_minutes: f64) -> Result<Vec<Content>> {
    let name = f.name.to_ascii_lowercase();
    if name.ends_with(".zip") {
        let mut contents = vec![];
        for inner in unzip(f)? {
            contents.extend(parse(&inner, avg_minutes)?);
        }
        return Ok(contents);
    }
    let Ok(text) = f.text() else {
        return Ok(vec![Content::Unused(f.name.clone())]);
    };
    let content = if name.ends_with(".html") {
        if ytmusic::is_html(text) {
            Content::Html(f.name.clone())
        } else {
            Content::Unused(f.name.clone())
        }
    } else if let Some(plays) = spotify::parse_extended(text) {
        Content::Extended(plays)
    } else if let Some(plays) = spotify::parse_basic(text) {
        Content::Basic(plays)
    } else if let Some(plays) = ytmusic::parse(text, avg_minutes) {
        Content::YouTube(plays)
    } else {
        Content::Unused(f.name.clone())
    };
    Ok(vec![content])
}

/// The `.json` and `.html` files in a ZIP, named `<ZIP name>/<path inside>`.
fn unzip(f: &File) -> Result<Vec<File>> {
    let open = || format!("cannot open {} as a ZIP", f.name);
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&f.bytes)).with_context(open)?;
    let mut files = vec![];
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).with_context(open)?;
        let name = format!("{}/{}", f.name, entry.name().with_context(open)?);
        let lower = name.to_ascii_lowercase();
        if !entry.is_file() || !(lower.ends_with(".json") || lower.ends_with(".html")) {
            continue;
        }
        let mut bytes = vec![];
        entry
            .read_to_end(&mut bytes)
            .with_context(|| format!("cannot read {name}"))?;
        files.push(File { name, bytes });
    }
    Ok(files)
}

/// Puts together what the files hold. Spotify plays come from the extended
/// history when there is one, else from the basic history (with a warning).
/// Plays in more than one YouTube history (`watch-history.json` and
/// `My Activity.json` hold the same ones) count once. Fails when the YouTube
/// history is only there as HTML.
pub fn combine<'a>(contents: impl IntoIterator<Item = &'a Content>) -> Result<Found> {
    let (mut extended, mut basic, mut html) = (vec![], vec![], None);
    let mut youtube: Option<Vec<Play>> = None;
    let mut unused = vec![];
    for c in contents {
        match c {
            Content::Extended(plays) => extended.extend_from_slice(plays),
            Content::Basic(plays) => basic.extend_from_slice(plays),
            Content::YouTube(plays) => youtube.get_or_insert_default().extend_from_slice(plays),
            Content::Html(name) => {
                html.get_or_insert(name);
            }
            Content::Unused(name) => unused.push(name.clone()),
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

/// Reads a file, or every `.json`, `.html` and `.zip` file in a folder and its
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
            } else if p.extension().is_some_and(|e| {
                ["json", "html", "zip"]
                    .iter()
                    .any(|x| e.eq_ignore_ascii_case(x))
            }) {
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
    fn opens_zips() {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(vec![]));
        let opts = zip::write::SimpleFileOptions::default();
        for (name, text) in [
            ("Spotify Extended Streaming History/a.json", EXTENDED),
            ("Spotify Extended Streaming History/ReadMeFirst.pdf", "%PDF"),
            ("Takeout/YouTube/historial.json", YOUTUBE),
        ] {
            zip.start_file(name, opts).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        let bytes = zip.finish().unwrap().into_inner();
        let found = read(&[file("my_spotify_data.zip", "")], 3.5);
        assert!(found.err().unwrap().to_string().contains("cannot open"));

        let zip = File {
            name: "export.zip".into(),
            bytes,
        };
        let found = read(&[zip], 3.5).unwrap();
        assert_eq!(found.spotify.unwrap().plays.len(), 1);
        assert_eq!(found.youtube.unwrap().plays.len(), 1);
        assert!(found.unused.is_empty(), "only .json and .html are read");
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
