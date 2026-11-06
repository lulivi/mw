//! Filters and aggregates over the combined list of plays.

use std::collections::{BTreeMap, HashMap};

use chrono::{Datelike, NaiveDate, Timelike};
use chrono_tz::Tz;

use crate::model::{Play, Source};
use crate::normalize::{artist_key, clean_title, track_key};

/// Plays shorter than this count as skips (same as Spotify).
pub const MIN_MS: u64 = 30_000;

/// What orders the top lists; the other measure breaks ties.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RankBy {
    Minutes,
    Plays,
}

/// Which plays count towards the stats.
pub struct Filters {
    /// First day included, in the time zone `tz`. `None` = no lower limit.
    pub from: Option<NaiveDate>,
    /// Last day included, in the time zone `tz`. `None` = no upper limit.
    pub to: Option<NaiveDate>,
    /// `None` = all sources.
    pub source: Option<Source>,
    /// Time zone that decides the day, month and hour of each play.
    pub tz: Tz,
}

/// An entry of a top list: an artist or a song.
#[derive(Debug, Clone)]
pub struct Ranked {
    /// Display name: the most frequent spelling among its plays.
    pub name: String,
    /// Artist, for songs.
    pub detail: Option<String>,
    /// Total listening time in milliseconds.
    pub ms: u64,
    /// Number of plays.
    pub plays: u64,
}

/// The recap of the filtered plays.
#[derive(Debug)]
pub struct Stats {
    /// First day of the range: the filter if given, else the first play.
    pub from: NaiveDate,
    /// Last day of the range: the filter if given, else the last play.
    pub to: NaiveDate,
    /// Total listening time in milliseconds.
    pub total_ms: u64,
    /// The part of the total that is an estimate.
    pub estimated_ms: u64,
    /// Number of plays counted.
    pub plays: u64,
    /// Minutes per month, in order, with empty months set to 0.
    pub by_month: Vec<((i32, u32), u64)>,
    /// Top artists, best first.
    pub top_artists: Vec<Ranked>,
    /// Top songs, best first, with the artist in [`Ranked::detail`], spelled as in
    /// the top artists.
    pub top_tracks: Vec<Ranked>,
    /// How many different artists were played.
    pub distinct_artists: usize,
    /// How many different songs were played.
    pub distinct_tracks: usize,
    /// Day with the most listening time, and that time in milliseconds.
    pub top_day: Option<(NaiveDate, u64)>,
    /// Hour of the day (0-23) with the most listening time, and that time in
    /// milliseconds.
    pub top_hour: Option<(u32, u64)>,
    /// Listening time in milliseconds per service.
    pub by_source: Vec<(Source, u64)>,
}

/// Running totals for one artist or song while the plays are aggregated.
#[derive(Default)]
struct Acc {
    /// Listening time in milliseconds.
    ms: u64,
    /// Number of plays.
    plays: u64,
    /// How often each spelling of the name appeared.
    names: HashMap<String, u64>,
    /// How often each spelling of the detail (the artist, for songs) appeared.
    details: HashMap<String, u64>,
}

impl Acc {
    /// Adds one play to the totals.
    fn add(&mut self, ms: u64, name: &str, detail: Option<&str>) {
        self.ms += ms;
        self.plays += 1;
        *self.names.entry(name.to_string()).or_default() += 1;
        if let Some(d) = detail {
            *self.details.entry(d.to_string()).or_default() += 1;
        }
    }
}

/// The most frequent name for a key (ties: alphabetical order).
fn most_common(names: &HashMap<String, u64>) -> String {
    names
        .iter()
        .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
        .map(|(n, _)| n.clone())
        .unwrap_or_default()
}

/// Turns the totals into a top list: sorted by `by`, with the other measure and
/// then the name breaking ties, and cut to `top` entries.
fn rank(map: HashMap<String, Acc>, top: usize, by: RankBy) -> Vec<Ranked> {
    let mut items: Vec<Ranked> = map
        .into_values()
        .map(|a| Ranked {
            name: most_common(&a.names),
            detail: if a.details.is_empty() {
                None
            } else {
                Some(most_common(&a.details))
            },
            ms: a.ms,
            plays: a.plays,
        })
        .collect();
    items.sort_by(|a, b| {
        let (pa, pb) = match by {
            RankBy::Minutes => ((a.ms, a.plays), (b.ms, b.plays)),
            RankBy::Plays => ((a.plays, a.ms), (b.plays, b.ms)),
        };
        pb.cmp(&pa).then_with(|| a.name.cmp(&b.name))
    });
    items.truncate(top);
    items
}

/// The (year, month) a day belongs to.
fn first_of_month(d: NaiveDate) -> (i32, u32) {
    (d.year(), d.month())
}

/// The (year, month) after the given one.
fn next_month((y, m): (i32, u32)) -> (i32, u32) {
    if m == 12 { (y + 1, 1) } else { (y, m + 1) }
}

/// Computes the stats. Returns `None` if no plays are left after filtering.
///
/// Plays with a real length under [`MIN_MS`] are skips and don't count; estimated
/// plays always do.
pub fn compute(plays: &[Play], f: &Filters, top: usize, by: RankBy) -> Option<Stats> {
    let mut total_ms = 0;
    let mut estimated_ms = 0;
    let mut count = 0;
    let mut months: BTreeMap<(i32, u32), u64> = BTreeMap::new();
    let mut days: HashMap<NaiveDate, u64> = HashMap::new();
    let mut hours = [0u64; 24];
    let mut artists: HashMap<String, Acc> = HashMap::new();
    let mut tracks: HashMap<String, Acc> = HashMap::new();
    let mut sources: BTreeMap<Source, u64> = BTreeMap::new();
    let mut first: Option<NaiveDate> = None;
    let mut last: Option<NaiveDate> = None;

    for p in plays {
        if f.source.is_some_and(|s| s != p.source) {
            continue;
        }
        let local = p.ts.with_timezone(&f.tz);
        let day = local.date_naive();
        if f.from.is_some_and(|d| day < d) || f.to.is_some_and(|d| day > d) {
            continue;
        }
        if !p.estimated && p.ms_played < MIN_MS {
            continue;
        }

        let ms = p.ms_played;
        total_ms += ms;
        count += 1;
        if p.estimated {
            estimated_ms += ms;
        }
        *months.entry(first_of_month(day)).or_default() += ms;
        *days.entry(day).or_default() += ms;
        hours[local.hour() as usize] += ms;
        *sources.entry(p.source).or_default() += ms;
        first = Some(first.map_or(day, |d| d.min(day)));
        last = Some(last.map_or(day, |d| d.max(day)));

        artists
            .entry(artist_key(&p.artist))
            .or_default()
            .add(ms, &p.artist, None);
        tracks
            .entry(track_key(&p.artist, &p.track))
            .or_default()
            .add(ms, &clean_title(&p.track), Some(&p.artist));
    }

    if count == 0 {
        return None;
    }

    let from = f.from.unwrap_or(first?);
    let to = f.to.unwrap_or(last?);

    let mut by_month = Vec::new();
    let end = first_of_month(to);
    let mut cur = first_of_month(from);
    while cur <= end {
        by_month.push((cur, months.get(&cur).copied().unwrap_or(0)));
        cur = next_month(cur);
    }

    let top_day = days
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)));
    let top_hour = hours
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(&a.0)))
        .filter(|(_, ms)| **ms > 0)
        .map(|(h, ms)| (h as u32, *ms));

    Some(Stats {
        from,
        to,
        total_ms,
        estimated_ms,
        plays: count,
        by_month,
        distinct_artists: artists.len(),
        distinct_tracks: tracks.len(),
        top_tracks: {
            let names: HashMap<String, String> = artists
                .iter()
                .map(|(k, a)| (k.clone(), most_common(&a.names)))
                .collect();
            let mut t = rank(tracks, top, by);
            for item in &mut t {
                if let Some(d) = item.detail.as_mut()
                    && let Some(n) = names.get(&artist_key(d))
                {
                    *d = n.clone();
                }
            }
            t
        },
        top_artists: rank(artists, top, by),
        top_day,
        top_hour,
        by_source: sources.into_iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    fn play(ts: &str, artist: &str, track: &str, ms: u64, src: Source) -> Play {
        Play {
            ts: NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M")
                .unwrap()
                .and_utc(),
            track: track.into(),
            artist: artist.into(),
            album: None,
            ms_played: ms,
            estimated: src == Source::YouTubeMusic,
            source: src,
            video_id: None,
        }
    }

    fn filters() -> Filters {
        Filters {
            from: None,
            to: None,
            source: None,
            tz: chrono_tz::Europe::Madrid,
        }
    }

    #[test]
    fn drops_skips_and_merges_sources() {
        let plays = vec![
            play(
                "2025-01-10 10:00",
                "Rosalía",
                "Despechá",
                180_000,
                Source::Spotify,
            ),
            play(
                "2025-01-10 11:00",
                "Rosalía",
                "Despechá",
                10_000,
                Source::Spotify,
            ), // skip
            play(
                "2025-02-01 10:00",
                "ROSALÍA - Topic",
                "DESPECHÁ (Official Video)",
                210_000,
                Source::YouTubeMusic,
            ),
        ];
        let s = compute(&plays, &filters(), 5, RankBy::Minutes).unwrap();
        assert_eq!(s.plays, 2);
        assert_eq!(s.total_ms, 390_000);
        assert_eq!(s.estimated_ms, 210_000);
        assert_eq!(s.top_artists.len(), 1);
        assert_eq!(s.top_artists[0].ms, 390_000);
        assert_eq!(s.top_tracks.len(), 1);
        assert_eq!(s.distinct_tracks, 1);
    }

    #[test]
    fn filters_by_local_date() {
        // Dec 31 23:30 UTC = Jan 1 00:30 in Madrid.
        let plays = vec![play("2024-12-31 23:30", "A", "T", 200_000, Source::Spotify)];
        let f = Filters {
            from: NaiveDate::from_ymd_opt(2025, 1, 1),
            to: NaiveDate::from_ymd_opt(2025, 12, 31),
            ..filters()
        };
        let s = compute(&plays, &f, 5, RankBy::Minutes).unwrap();
        assert_eq!(s.plays, 1);
        assert_eq!(s.by_month.len(), 12);
        assert_eq!(s.by_month[0], ((2025, 1), 200_000));
    }

    #[test]
    fn filters_by_source() {
        let plays = vec![
            play("2025-01-10 10:00", "A", "T", 200_000, Source::Spotify),
            play("2025-01-10 11:00", "B", "U", 210_000, Source::YouTubeMusic),
        ];
        let f = Filters {
            source: Some(Source::YouTubeMusic),
            ..filters()
        };
        let s = compute(&plays, &f, 5, RankBy::Minutes).unwrap();
        assert_eq!(s.plays, 1);
        assert_eq!(s.top_artists[0].name, "B");
    }

    #[test]
    fn empty_range_returns_none() {
        let plays = vec![play("2025-01-10 10:00", "A", "T", 200_000, Source::Spotify)];
        let f = Filters {
            from: NaiveDate::from_ymd_opt(2026, 1, 1),
            ..filters()
        };
        assert!(compute(&plays, &f, 5, RankBy::Minutes).is_none());
    }

    #[test]
    fn ranks_by_plays() {
        let plays = vec![
            play("2025-01-10 10:00", "Long", "X", 600_000, Source::Spotify),
            play("2025-01-10 11:00", "Short", "Y", 60_000, Source::Spotify),
            play("2025-01-10 12:00", "Short", "Y", 60_000, Source::Spotify),
        ];
        let s = compute(&plays, &filters(), 5, RankBy::Plays).unwrap();
        assert_eq!(s.top_artists[0].name, "Short");
        let s = compute(&plays, &filters(), 5, RankBy::Minutes).unwrap();
        assert_eq!(s.top_artists[0].name, "Long");
    }
}
