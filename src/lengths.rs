//! Song lengths measured from Spotify full plays, so they can be shared without
//! the listening history.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, de::Error};

use crate::model::Play;
use crate::normalize::track_key;

/// Full plays a song needs before its length is trusted.
const MIN_PLAYS: usize = 2;
/// How close to the median a full play must be to count as agreeing with it.
pub const AGREE_MS: u64 = 1_000;
/// `check` of a song with a single full play.
const ONE_PLAY: &str = "one full play";

/// One song of an export file: only what the song database needs.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct SongLength {
    pub artist: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    /// Seconds; read also as `"m:ss"`, for lengths typed by hand.
    #[serde(deserialize_with = "seconds_or_mmss")]
    pub seconds: u64,
    /// Why `seconds` is only a guess. Import skips the song while it is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<String>,
}

/// What [`measure`] found.
#[derive(Default)]
pub struct Measured {
    /// Every song `known` doesn't have, sorted by key; the unverified ones have `check`.
    pub songs: Vec<SongLength>,
    /// Songs with a single full play.
    pub single: usize,
    /// Songs with fewer than two full plays within [`AGREE_MS`] of the median.
    pub inconsistent: usize,
    /// Songs skipped because `known` already has them.
    pub known: usize,
}

/// Measures the song length of every song in `full_plays` that `known` (called
/// with the song key) doesn't have yet: the median of its full plays. It is
/// trusted when at least two of them are within [`AGREE_MS`] of it (others may be
/// off by a seek or a resume); otherwise the song gets a `check` saying why, with
/// the lengths played. Names and album are the most frequent spelling.
pub fn measure(full_plays: &[Play], known: impl Fn(&str) -> bool) -> Measured {
    let mut by_key: BTreeMap<String, Vec<&Play>> = BTreeMap::new();
    for p in full_plays {
        by_key
            .entry(track_key(&p.artist, &p.track))
            .or_default()
            .push(p);
    }

    let mut out = Measured::default();
    for (key, plays) in by_key {
        if known(&key) {
            out.known += 1;
            continue;
        }
        let (artist, title) = most_common(plays.iter().map(|p| (&p.artist, &p.track)))
            .map(|(a, t)| (a.clone(), t.clone()))
            .unwrap_or_default();
        let album = most_common(
            plays
                .iter()
                .filter_map(|p| p.album.as_deref().map(str::trim))
                .filter(|a| !a.is_empty()),
        )
        .map(str::to_string);
        let mut ms: Vec<u64> = plays.iter().map(|p| p.ms_played).collect();
        ms.sort_unstable();
        let twice = twice_median(&ms);
        let agreeing = ms
            .iter()
            .filter(|&&m| (2 * m).abs_diff(twice) <= 2 * AGREE_MS)
            .count();
        let check = if ms.len() < MIN_PLAYS {
            out.single += 1;
            Some(ONE_PLAY.to_string())
        } else if agreeing < MIN_PLAYS {
            out.inconsistent += 1;
            Some(format!("full plays: {}", played(&ms)))
        } else {
            None
        };
        out.songs.push(SongLength {
            artist,
            title,
            album,
            seconds: (twice + 1000) / 2000,
            check,
        });
    }
    out
}

/// The lengths played, by second, with how often when more than once:
/// `3:00 ×5, 4:00`.
fn played(sorted_ms: &[u64]) -> String {
    let mut counts: BTreeMap<u64, usize> = BTreeMap::new();
    for ms in sorted_ms {
        *counts.entry((ms + 500) / 1000).or_default() += 1;
    }
    let parts: Vec<String> = counts
        .into_iter()
        .map(|(s, n)| match n {
            1 => mmss(s),
            n => format!("{} ×{n}", mmss(s)),
        })
        .collect();
    parts.join(", ")
}

/// Seconds as `m:ss`.
pub fn mmss(seconds: u64) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// Parses a length as seconds (`229`) or minutes and seconds (`3:49`).
fn parse_length(s: &str) -> Option<u64> {
    let s = s.trim();
    match s.split_once(':') {
        Some((m, sec)) if sec.len() == 2 => {
            let sec: u64 = sec.parse().ok()?;
            (sec < 60).then_some(m.parse::<u64>().ok()? * 60 + sec)
        }
        Some(_) => None,
        None => s.parse().ok(),
    }
}

/// Reads `seconds` as a number or as a string [`parse_length`] understands.
fn seconds_or_mmss<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Length {
        Seconds(u64),
        Text(String),
    }
    match Length::deserialize(d)? {
        Length::Seconds(n) => Ok(n),
        Length::Text(t) => parse_length(&t)
            .ok_or_else(|| D::Error::custom(format!("invalid length {t:?}: use seconds or m:ss"))),
    }
}

/// Twice the median of sorted milliseconds (the median of an even count is the
/// mean of the two middle ones), so it stays an integer.
fn twice_median(sorted_ms: &[u64]) -> u64 {
    let n = sorted_ms.len();
    if n % 2 == 1 {
        2 * sorted_ms[n / 2]
    } else {
        sorted_ms[n / 2 - 1] + sorted_ms[n / 2]
    }
}

/// The most frequent item; on a tie, the first one seen.
fn most_common<T: PartialEq>(items: impl Iterator<Item = T>) -> Option<T> {
    let mut counts: Vec<(T, usize)> = vec![];
    for item in items {
        match counts.iter_mut().find(|(t, _)| *t == item) {
            Some((_, n)) => *n += 1,
            None => counts.push((item, 1)),
        }
    }
    let mut best: Option<(T, usize)> = None;
    for (t, n) in counts {
        if best.as_ref().is_none_or(|(_, b)| n > *b) {
            best = Some((t, n));
        }
    }
    best.map(|(t, _)| t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Source;
    use chrono::Utc;

    fn play(artist: &str, track: &str, album: Option<&str>, ms: u64) -> Play {
        Play {
            ts: Utc::now(),
            track: track.into(),
            artist: artist.into(),
            album: album.map(Into::into),
            ms_played: ms,
            estimated: false,
            source: Source::Spotify,
            video_id: None,
        }
    }

    /// (title, seconds, check) of each song.
    fn summary(m: &Measured) -> Vec<(&str, u64, Option<&str>)> {
        m.songs
            .iter()
            .map(|s| (s.title.as_str(), s.seconds, s.check.as_deref()))
            .collect()
    }

    #[test]
    fn median_of_at_least_two_full_plays() {
        let plays = [
            // Odd count: the middle one.
            play("A", "Odd", None, 200_000),
            play("A", "Odd", None, 201_000),
            play("A", "Odd", None, 204_000),
            // Even count: the mean of the middle two, rounded.
            play("B", "Even", None, 180_000),
            play("B", "Even", None, 181_000),
            // One full play only: to check.
            play("C", "Once", None, 150_400),
        ];
        let m = measure(&plays, |_| false);
        assert_eq!(
            summary(&m),
            [
                ("Odd", 201, None),
                ("Even", 181, None),
                ("Once", 150, Some("one full play"))
            ]
        );
        assert_eq!((m.single, m.inconsistent), (1, 0));
    }

    #[test]
    fn needs_two_plays_near_the_median() {
        let plays = [
            // Two versions: no play near the median.
            play("A", "Edit", None, 180_000),
            play("A", "Edit", None, 180_000),
            play("A", "Edit", None, 240_000),
            play("A", "Edit", None, 240_000),
            // 3 s apart: neither within 1 s of the median.
            play("A", "Near", None, 180_000),
            play("A", "Near", None, 183_000),
            // Two agree; a seek back and a resume are ignored.
            play("A", "Seeked", None, 238_862),
            play("A", "Seeked", None, 227_803),
            play("A", "Seeked", None, 229_137),
            play("A", "Seeked", None, 229_137),
        ];
        let m = measure(&plays, |_| false);
        assert_eq!(
            summary(&m),
            [
                ("Edit", 210, Some("full plays: 3:00 ×2, 4:00 ×2")),
                ("Near", 182, Some("full plays: 3:00, 3:03")),
                ("Seeked", 229, None),
            ]
        );
        assert_eq!((m.single, m.inconsistent), (0, 2));
    }

    #[test]
    fn lengths_as_seconds_or_mmss() {
        let read = |seconds: &str| {
            serde_json::from_str::<SongLength>(&format!(
                r#"{{"artist": "A", "title": "T", "seconds": {seconds}}}"#
            ))
            .map(|s| s.seconds)
        };
        assert_eq!(read("229").unwrap(), 229);
        assert_eq!(read(r#""229""#).unwrap(), 229);
        assert_eq!(read(r#""3:49""#).unwrap(), 229);
        assert_eq!(read(r#"" 12:05 ""#).unwrap(), 725);
        for bad in [r#""3:5""#, r#""3:60""#, r#""abc""#, r#""""#, "-1"] {
            assert!(read(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn known_songs_are_left_out() {
        let plays = [play("A", "T", None, 1000), play("A", "T", None, 1000)];
        let m = measure(&plays, |k| k == track_key("A", "T"));
        assert!(m.songs.is_empty());
        assert_eq!(m.known, 1);
    }

    #[test]
    fn most_frequent_spelling_and_album() {
        let plays = [
            play("Rosalía", "Despechá", Some("Motomami +"), 157_000),
            play("ROSALÍA", "DESPECHÁ", Some("Motomami"), 157_000),
            play("ROSALÍA", "DESPECHÁ", Some("Motomami"), 157_000),
        ];
        let s = &measure(&plays, |_| false).songs[0];
        assert_eq!(
            (s.artist.as_str(), s.title.as_str(), s.album.as_deref()),
            ("ROSALÍA", "DESPECHÁ", Some("Motomami"))
        );
    }

    #[test]
    fn export_has_only_the_song_fields() {
        let songs = [
            SongLength {
                artist: "A".into(),
                title: "T".into(),
                album: Some("L".into()),
                seconds: 200,
                check: None,
            },
            SongLength {
                artist: "B".into(),
                title: "U".into(),
                album: None,
                seconds: 100,
                check: Some("one full play".into()),
            },
        ];
        let json = serde_json::to_value(&songs).unwrap();
        assert_eq!(
            json,
            serde_json::json!([
                {"artist": "A", "title": "T", "album": "L", "seconds": 200},
                {"artist": "B", "title": "U", "seconds": 100, "check": "one full play"}
            ])
        );
    }
}
