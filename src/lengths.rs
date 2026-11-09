//! Song lengths measured from Spotify full plays, so they can be shared without
//! the listening history.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::Play;
use crate::normalize::track_key;

/// Full plays a song needs before its length is trusted.
const MIN_PLAYS: usize = 2;
/// Most the full plays of one song may differ (longest − shortest).
pub const MAX_SPREAD_MS: u64 = 5_000;

/// One song of an export file: only what the song database needs.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct SongLength {
    pub artist: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    pub seconds: u64,
}

/// A song left out, with the shortest and longest of its full plays.
pub struct LeftOut {
    pub artist: String,
    pub title: String,
    pub min_ms: u64,
    pub max_ms: u64,
}

/// What [`measure`] found.
#[derive(Default)]
pub struct Measured {
    /// Songs with a trusted length, sorted by key.
    pub songs: Vec<SongLength>,
    /// Songs whose full plays differ by more than [`MAX_SPREAD_MS`].
    pub inconsistent: Vec<LeftOut>,
    /// Songs with a single full play.
    pub single: Vec<LeftOut>,
    /// Songs skipped because `known` already has them.
    pub known: usize,
}

/// Measures the song length of every song in `full_plays` that `known` (called
/// with the song key) doesn't have yet: the median of at least two full plays
/// that agree within [`MAX_SPREAD_MS`]. Names and album are the most frequent
/// spelling.
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
        let mut ms: Vec<u64> = plays.iter().map(|p| p.ms_played).collect();
        ms.sort_unstable();
        let (min_ms, max_ms) = (ms[0], ms[ms.len() - 1]);
        if ms.len() < MIN_PLAYS {
            out.single.push(LeftOut {
                artist,
                title,
                min_ms,
                max_ms,
            });
        } else if max_ms - min_ms > MAX_SPREAD_MS {
            out.inconsistent.push(LeftOut {
                artist,
                title,
                min_ms,
                max_ms,
            });
        } else {
            let album = most_common(
                plays
                    .iter()
                    .filter_map(|p| p.album.as_deref().map(str::trim))
                    .filter(|a| !a.is_empty()),
            )
            .map(str::to_string);
            out.songs.push(SongLength {
                artist,
                title,
                album,
                seconds: median_seconds(&ms),
            });
        }
    }
    out
}

/// Median of sorted milliseconds (the mean of the two middle ones for an even
/// count), rounded to the nearest second.
fn median_seconds(sorted_ms: &[u64]) -> u64 {
    let n = sorted_ms.len();
    let twice = if n % 2 == 1 {
        2 * sorted_ms[n / 2]
    } else {
        sorted_ms[n / 2 - 1] + sorted_ms[n / 2]
    };
    (twice + 1000) / 2000
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
            // One full play only.
            play("C", "Once", None, 150_000),
        ];
        let m = measure(&plays, |_| false);
        let got: Vec<_> = m
            .songs
            .iter()
            .map(|s| (s.title.as_str(), s.seconds))
            .collect();
        assert_eq!(got, [("Odd", 201), ("Even", 181)]);
        assert_eq!(m.single.len(), 1);
        assert_eq!(m.single[0].title, "Once");
    }

    #[test]
    fn plays_that_disagree_are_left_out() {
        let plays = [
            play("A", "Edit", None, 180_000),
            play("A", "Edit", None, 240_000),
            play("A", "Close", None, 180_000),
            play("A", "Close", None, 185_000),
        ];
        let m = measure(&plays, |_| false);
        assert_eq!(m.songs.len(), 1);
        assert_eq!(m.songs[0].title, "Close");
        assert_eq!(m.inconsistent[0].title, "Edit");
        assert_eq!(
            (m.inconsistent[0].min_ms, m.inconsistent[0].max_ms),
            (180_000, 240_000)
        );
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
            },
            SongLength {
                artist: "B".into(),
                title: "U".into(),
                album: None,
                seconds: 100,
            },
        ];
        let json = serde_json::to_value(&songs).unwrap();
        assert_eq!(
            json,
            serde_json::json!([
                {"artist": "A", "title": "T", "album": "L", "seconds": 200},
                {"artist": "B", "title": "U", "seconds": 100}
            ])
        );
    }
}
