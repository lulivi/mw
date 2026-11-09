//! Song lengths from MusicBrainz for the entries of a contribution file that
//! have a `check` (`mw db resolve`).
//!
//! MusicBrainz asks for at most one request per second and a User-Agent naming
//! the program: <https://musicbrainz.org/doc/MusicBrainz_API/Rate_Limiting>.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::lengths::{FULL_PLAYS, ONE_PLAY, SongLength, mmss, parse_length, twice_median};
use crate::normalize::{artist_key, title_key};

/// The recording search.
pub const URL: &str = "https://musicbrainz.org/ws/2/recording";
/// Time between two requests.
pub const DELAY: Duration = Duration::from_millis(1100);
/// Wait before the first retry after a 503; doubled on each one.
pub const BACKOFF: Duration = Duration::from_secs(2);
/// Retries after a 503.
const RETRIES: u32 = 3;
const USER_AGENT: &str = concat!(
    "mw/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/lulivi/mw)"
);
/// A MusicBrainz length further than this from every length played keeps a `check`.
const FAR_S: u64 = 2;
/// Added to the `check` of a song MusicBrainz doesn't have.
const NOT_FOUND: &str = "; not in musicbrainz";
/// Entries whose `check` has this were already looked up.
const LOOKED_UP: &str = "musicbrainz";

/// A search result; only the fields used here.
#[derive(Debug, Deserialize)]
struct Search {
    recordings: Vec<Recording>,
}

/// A MusicBrainz recording.
#[derive(Debug, Deserialize)]
pub struct Recording {
    id: String,
    title: String,
    /// Milliseconds.
    length: Option<u64>,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Vec<Credit>,
}

/// One artist of a recording's credit ("A" + " feat. " + "B").
#[derive(Debug, Deserialize)]
struct Credit {
    name: String,
    #[serde(default)]
    joinphrase: String,
}

/// Searches MusicBrainz, waiting between requests.
pub struct Client {
    agent: ureq::Agent,
    url: String,
    delay: Duration,
    backoff: Duration,
    last: Option<Instant>,
}

impl Client {
    /// A client for the search at `url`, with `delay` between requests and
    /// `backoff` before the first retry of a 503.
    pub fn new(url: &str, delay: Duration, backoff: Duration) -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .user_agent(USER_AGENT)
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .into();
        Client {
            agent,
            url: url.into(),
            delay,
            backoff,
            last: None,
        }
    }

    /// The recordings MusicBrainz finds for this title and artist.
    fn search(&mut self, artist: &str, title: &str) -> Result<Vec<Recording>> {
        let query = format!(
            r#"recording:"{}" AND artist:"{}""#,
            escape(title),
            escape(artist)
        );
        let (mut wait, mut retries) = (self.backoff, RETRIES);
        loop {
            if let Some(last) = self.last {
                sleep(self.delay.saturating_sub(last.elapsed()));
            }
            self.last = Some(Instant::now());
            let mut resp = self
                .agent
                .get(&self.url)
                .query("query", &query)
                .query("fmt", "json")
                .query("limit", "25")
                .call()?;
            let status = resp.status();
            if status == 503 && retries > 0 {
                retries -= 1;
                sleep(wait);
                wait *= 2;
                continue;
            }
            if !status.is_success() {
                bail!("MusicBrainz answered {status}");
            }
            return Ok(resp.body_mut().read_json::<Search>()?.recordings);
        }
    }
}

/// Escapes the characters Lucene's query syntax gives a meaning to.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if r#"+-&|!(){}[]^"~*?:\/"#.contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Does the credit name this artist key, as a whole or as one of its artists?
fn credits(r: &Recording, artist: &str) -> bool {
    let whole: String = r
        .artist_credit
        .iter()
        .map(|c| format!("{}{}", c.name, c.joinphrase))
        .collect();
    artist_key(&whole) == artist
        || r.artist_credit
            .iter()
            .any(|c| artist_key(&c.name) == artist)
}

/// The lengths played, in seconds, and how the `check` wrote them: from
/// `seconds` for one full play, from the `check` for full plays that don't agree.
fn played(s: &SongLength) -> Option<(Vec<u64>, String)> {
    let check = s.check.as_deref()?;
    if check == ONE_PLAY {
        let seconds = s.seconds?;
        return Some((vec![seconds], mmss(seconds)));
    }
    let list = check.strip_prefix(FULL_PLAYS)?;
    let lengths = list
        .split(", ")
        .filter_map(|p| parse_length(p.split(" ×").next()?))
        .collect();
    Some((lengths, list.to_string()))
}

/// Gives the entry the length of the recordings whose title and artist match
/// it: the median, with the ID of the one closest to it. Removes the `check`,
/// unless there are lengths played and the median is far from all of them:
/// then the `check` says both. Returns whether any recording matched; if none
/// did, the `check` says so.
fn settle(s: &mut SongLength, recordings: &[Recording]) -> bool {
    let (artist, title) = (artist_key(&s.artist), title_key(&s.title));
    let found: Vec<(&Recording, u64)> = recordings
        .iter()
        .filter(|r| title_key(&r.title) == title && credits(r, &artist))
        .filter_map(|r| Some((r, r.length?)))
        .collect();
    if found.is_empty() {
        s.check = Some(format!("{}{NOT_FOUND}", s.check.as_deref().unwrap_or("")));
        return false;
    }
    let mut ms: Vec<u64> = found.iter().map(|(_, ms)| *ms).collect();
    ms.sort_unstable();
    let twice = twice_median(&ms);
    let (best, _) = found
        .iter()
        .min_by_key(|(_, ms)| (2 * ms).abs_diff(twice))
        .expect("found is not empty");
    let seconds = (twice + 1000) / 2000;
    s.check = played(s).and_then(|(lengths, text)| {
        let far = lengths.iter().all(|p| p.abs_diff(seconds) > FAR_S);
        far.then(|| format!("{LOOKED_UP} {}, played {text}", mmss(seconds)))
    });
    s.seconds = Some(seconds);
    s.mbid = Some(best.id.clone());
    s.source = Some(LOOKED_UP.into());
    true
}

/// What [`resolve`] did.
#[derive(Debug, Default)]
pub struct Resolved {
    /// Songs given a length with no `check` left.
    pub found: usize,
    /// "artist · title  (check)" of the songs that found a length far from the ones played.
    pub far: Vec<String>,
    /// "artist · title" of the songs MusicBrainz doesn't have.
    pub not_found: Vec<String>,
    /// Songs still to look up (past `limit`).
    pub left: usize,
}

/// Looks up in MusicBrainz the entries of the contribution file at `path` that
/// have a `check` and weren't looked up yet, at most `limit` of them, and edits
/// the file after each one, so a run can be stopped and continued.
pub fn resolve(path: &Path, client: &mut Client, limit: Option<usize>) -> Result<Resolved> {
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let mut songs: Vec<SongLength> = serde_json::from_str(&text)
        .with_context(|| format!("{} is not a contribution file", path.display()))?;
    let todo: Vec<usize> = (0..songs.len())
        .filter(|&i| {
            songs[i]
                .check
                .as_deref()
                .is_some_and(|c| !c.contains(LOOKED_UP))
        })
        .collect();
    let n = limit.unwrap_or(usize::MAX).min(todo.len());

    let mut out = Resolved {
        left: todo.len() - n,
        ..Default::default()
    };
    for &i in &todo[..n] {
        let s = &mut songs[i];
        let name = format!("{} · {}", s.artist, s.title);
        let recordings = client.search(&s.artist, &s.title).with_context(|| {
            format!(
                "cannot look up {name}; the songs looked up so far are saved, run again to continue"
            )
        })?;
        if !settle(s, &recordings) {
            out.not_found.push(name);
        } else if let Some(check) = &s.check {
            out.far.push(format!("{name}  ({check})"));
        } else {
            out.found += 1;
        }
        save(path, &songs)?;
    }
    Ok(out)
}

/// Writes the file through a temporary one, so stopping halfway never leaves it cut.
fn save(path: &Path, songs: &[SongLength]) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, serde_json::to_string_pretty(songs)? + "\n")
        .with_context(|| format!("cannot write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("cannot write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    fn rec(id: &str, artist: &str, title: &str, length: Option<u64>) -> Recording {
        Recording {
            id: id.into(),
            title: title.into(),
            length,
            artist_credit: vec![Credit {
                name: artist.into(),
                joinphrase: String::new(),
            }],
        }
    }

    fn entry(check: &str, seconds: Option<u64>) -> SongLength {
        SongLength {
            artist: "Mala Rodriguez".into(),
            title: "Por la noche".into(),
            check: Some(check.into()),
            seconds,
            ..Default::default()
        }
    }

    /// Results like MusicBrainz's for "Por la noche": two lengths, and versions
    /// with another title or no length.
    fn por_la_noche() -> Vec<Recording> {
        vec![
            rec("a", "Mala Rodríguez", "Por la noche", Some(222_800)),
            rec("b", "Mala Rodríguez", "Por la noche", Some(225_000)),
            rec("c", "Mala Rodríguez", "Por la noche", Some(230_000)),
            rec(
                "x",
                "Mala Rodríguez",
                "Por la noche (con Ari y Meko)",
                Some(300_000),
            ),
            rec("y", "Otra", "Por la noche", Some(100_000)),
            rec("z", "Mala Rodríguez", "Por la noche", None),
        ]
    }

    #[test]
    fn median_of_matching_candidates() {
        let mut s = entry("no length", None);
        assert!(settle(&mut s, &por_la_noche()));
        assert_eq!(s.seconds, Some(225));
        assert_eq!(s.mbid.as_deref(), Some("b"));
        assert_eq!(s.source.as_deref(), Some("musicbrainz"));
        assert_eq!(s.check, None);
    }

    #[test]
    fn matches_any_credited_artist() {
        let featuring = Recording {
            artist_credit: vec![
                Credit {
                    name: "Mala Rodríguez".into(),
                    joinphrase: " feat. ".into(),
                },
                Credit {
                    name: "Tote King".into(),
                    joinphrase: String::new(),
                },
            ],
            ..rec("f", "", "Por la noche", Some(200_000))
        };
        let mut s = entry("no length", None);
        assert!(settle(&mut s, &[featuring]));
        assert_eq!(s.mbid.as_deref(), Some("f"));
    }

    #[test]
    fn far_from_the_lengths_played_keeps_a_check() {
        // One full play 3 s away: far.
        let mut s = entry(ONE_PLAY, Some(222));
        settle(&mut s, &por_la_noche());
        assert_eq!(s.check.as_deref(), Some("musicbrainz 3:45, played 3:42"));
        assert_eq!(s.seconds, Some(225));
        // 2 s away: close enough.
        let mut s = entry(ONE_PLAY, Some(223));
        settle(&mut s, &por_la_noche());
        assert_eq!(s.check, None);
        // Full plays that don't agree: MusicBrainz picks the version.
        let mut s = entry("full plays: 3:00 ×2, 3:46 ×2", None);
        settle(&mut s, &por_la_noche());
        assert_eq!(s.check, None);
        let mut s = entry("full plays: 3:00 ×2, 4:00 ×2", None);
        settle(&mut s, &por_la_noche());
        assert_eq!(
            s.check.as_deref(),
            Some("musicbrainz 3:45, played 3:00 ×2, 4:00 ×2")
        );
    }

    #[test]
    fn not_found_says_so() {
        let mut s = SongLength {
            title: "Por la noche - Remix con Ari y Meko".into(),
            ..entry(ONE_PLAY, Some(250))
        };
        assert!(!settle(&mut s, &por_la_noche()));
        assert_eq!(
            s.check.as_deref(),
            Some("one full play; not in musicbrainz")
        );
        assert_eq!((s.seconds, s.mbid), (Some(250), None));
    }

    #[test]
    fn escapes_lucene() {
        assert_eq!(
            escape(r#"AC/DC: "Hi" (Live) - 1+1"#),
            r#"AC\/DC\: \"Hi\" \(Live\) \- 1\+1"#
        );
        assert_eq!(escape("Rosalía"), "Rosalía");
    }

    /// Each request a test server got, with when.
    type Requests = Arc<Mutex<Vec<(Instant, String)>>>;

    /// Answers each request with the next of `answers` (status, body), the last
    /// one again when they run out. Returns the URL and each request with its time.
    fn serve(answers: Vec<(u16, &'static str)>) -> (String, Requests) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/ws/2/recording", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(vec![]));
        let log = seen.clone();
        std::thread::spawn(move || {
            for (i, stream) in listener.incoming().enumerate() {
                let mut stream = stream.unwrap();
                let mut buf = [0; 4096];
                let n = stream.read(&mut buf).unwrap();
                log.lock().unwrap().push((
                    Instant::now(),
                    String::from_utf8_lossy(&buf[..n]).into_owned(),
                ));
                let (status, body) = answers[i.min(answers.len() - 1)];
                let response = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        (url, seen)
    }

    const FOUND: &str = r#"{"recordings": [{"id": "m1", "title": "T", "length": 200000,
        "artist-credit": [{"name": "A"}]}]}"#;
    const NONE: &str = r#"{"recordings": []}"#;

    #[test]
    fn waits_between_requests_and_retries_503() {
        let (url, seen) = serve(vec![(503, ""), (200, FOUND), (200, NONE)]);
        let delay = Duration::from_millis(150);
        let mut client = Client::new(&url, delay, Duration::from_millis(10));
        assert_eq!(client.search("A", "T").unwrap().len(), 1);
        assert_eq!(client.search("B", "U").unwrap().len(), 0);

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        for pair in seen.windows(2) {
            // The server notes a request a little after it was sent.
            let gap = pair[1].0 - pair[0].0;
            assert!(gap >= delay - Duration::from_millis(20), "{gap:?} apart");
        }
        let request = seen[0].1.to_lowercase();
        assert!(
            request.contains(&format!("user-agent: {}", USER_AGENT.to_lowercase())),
            "{request}"
        );
        assert!(request.contains("fmt=json"), "{request}");
    }

    #[test]
    fn gives_up_after_the_retries() {
        let (url, seen) = serve(vec![(503, "")]);
        let mut client = Client::new(&url, Duration::ZERO, Duration::ZERO);
        assert!(client.search("A", "T").is_err());
        assert_eq!(seen.lock().unwrap().len(), 1 + RETRIES as usize);
    }

    #[test]
    fn resolves_the_file_up_to_the_limit() {
        let (url, seen) = serve(vec![(200, FOUND), (200, NONE)]);
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("c.json");
        let song = |title: &str, check: Option<&str>| SongLength {
            artist: "A".into(),
            title: title.into(),
            check: check.map(Into::into),
            seconds: check.is_none().then_some(100),
            ..Default::default()
        };
        save(
            &file,
            &[
                song("Done", None),
                song("Tried", Some("no length; not in musicbrainz")),
                song("T", Some("no length")),
                song("Missing", Some("no length")),
                song("Later", Some("no length")),
            ],
        )
        .unwrap();
        let mut client = Client::new(&url, Duration::ZERO, Duration::ZERO);

        let r = resolve(&file, &mut client, Some(2)).unwrap();
        assert_eq!((r.found, r.left), (1, 1));
        assert_eq!(r.not_found, ["A · Missing"]);
        assert_eq!(seen.lock().unwrap().len(), 2);

        let got: Vec<SongLength> =
            serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(got[2].seconds, Some(200));
        assert_eq!(got[2].mbid.as_deref(), Some("m1"));
        assert_eq!(got[2].check, None);
        assert_eq!(
            got[3].check.as_deref(),
            Some("no length; not in musicbrainz")
        );
        assert_eq!(got[4].check.as_deref(), Some("no length"));

        // The next run looks up only what is left.
        let r = resolve(&file, &mut client, None).unwrap();
        assert_eq!((r.not_found.len(), r.left), (1, 0));
        assert_eq!(seen.lock().unwrap().len(), 3);
    }
}
