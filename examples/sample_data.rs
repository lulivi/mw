//! Writes a fictional listening history (2021–2025) for the sample images:
//! `<dir>/spotify/Streaming_History_Audio_2021-2025_0.json` and
//! `<dir>/watch-history.json` (YouTube Music, from 2024 on).
//!
//! Artists and songs are made up. A fixed seed makes every run write the same data.
//!
//! Usage: `cargo run --example sample_data -- <dir>`

use std::fs;
use std::path::PathBuf;

use chrono::{Datelike, Duration, NaiveDate};
use serde_json::{Value, json};

/// (artist, base weight, yearly trend, songs as (title, weight, seconds)).
type Artist = (&'static str, f64, f64, &'static [(&'static str, f64, u64)]);

const ARTISTS: &[Artist] = &[
    (
        "Luna Vale",
        9.0,
        0.10,
        &[
            ("Paper Lanterns", 5.0, 214),
            ("Midnight Ferry", 3.0, 241),
            ("Salt & Static", 2.0, 198),
            ("Glasshouse", 1.0, 226),
        ],
    ),
    (
        "The Copper Pines",
        8.0,
        -0.12,
        &[
            ("Northbound", 4.0, 252),
            ("Woodsmoke", 3.0, 237),
            (
                "Everything We Left Behind on the Coast Road (Acoustic Version)",
                2.5,
                289,
            ),
            ("Ember", 1.0, 205),
        ],
    ),
    (
        "Kairo Blue",
        7.0,
        0.25,
        &[
            ("Neon Tide", 5.0, 187),
            ("Overclock", 2.0, 176),
            ("Blue Hour", 2.0, 203),
        ],
    ),
    (
        "Marisol Ortega",
        6.0,
        0.05,
        &[
            ("Cielo Roto", 4.0, 222),
            ("Agua de Mayo", 3.0, 209),
            ("Volver a Empezar", 2.0, 248),
            ("Lumbre", 1.0, 190),
            ("Brisa", 1.0, 175),
        ],
    ),
    (
        "Static Orchard",
        5.0,
        0.00,
        &[
            ("Fieldnotes", 3.0, 264),
            ("Low Sun", 2.0, 231),
            ("Orchard Lights", 2.0, 245),
        ],
    ),
    (
        "Juno & the Night Shift",
        4.5,
        0.30,
        &[
            ("Graveyard Shift", 4.0, 199),
            ("Coffee at 3 a.m.", 2.0, 182),
            ("Taillights", 2.0, 211),
        ],
    ),
    (
        "Okapi",
        4.0,
        -0.05,
        &[
            ("Savanna Radio", 3.0, 233),
            ("Tall Grass", 2.0, 218),
            ("Dry Season", 1.0, 256),
        ],
    ),
    (
        "Nora Lindqvist",
        3.5,
        0.15,
        &[
            ("Frost on the Window", 3.0, 240),
            ("Archipelago", 2.0, 267),
            ("Polar Night", 1.0, 228),
        ],
    ),
    (
        "Velvet Arcade",
        3.0,
        0.00,
        &[
            ("High Score", 3.0, 171),
            ("Insert Coin", 2.0, 164),
            ("Continue?", 1.0, 193),
        ],
    ),
    (
        "Señor Tormenta",
        2.5,
        0.20,
        &[
            ("Relámpago", 3.0, 201),
            ("Tormenta de Verano", 2.0, 215),
            ("Calma", 1.0, 232),
        ],
    ),
];

/// Small fixed-seed PRNG (SplitMix64), so the data needs no extra dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    fn f(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in [lo, hi).
    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next() % (hi - lo)
    }

    /// Index picked with probability proportional to its weight.
    fn pick(&mut self, weights: &[f64]) -> usize {
        let mut x = self.f() * weights.iter().sum::<f64>();
        for (i, w) in weights.iter().enumerate() {
            if x < *w {
                return i;
            }
            x -= w;
        }
        weights.len() - 1
    }
}

fn main() -> std::io::Result<()> {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: sample_data <output dir>"),
    );
    let mut rng = Rng(2);
    let mut spotify: Vec<Value> = Vec::new();
    let mut ytm: Vec<Value> = Vec::new();

    let first = NaiveDate::from_ymd_opt(2021, 1, 1).unwrap();
    let last = NaiveDate::from_ymd_opt(2025, 12, 31).unwrap();
    let mut day = first;
    while day <= last {
        let years = (day.year() - 2021) as f64;
        // More listening each year, a summer peak and a December bump.
        let season = match day.month() {
            6..=8 => 1.4,
            12 => 1.25,
            2 | 3 => 0.8,
            _ => 1.0,
        };
        let spike = if day == NaiveDate::from_ymd_opt(2025, 7, 19).unwrap() {
            4.0
        } else {
            1.0
        };
        let mean = 14.0 * (1.0 + 0.15 * years) * season * spike;
        let plays = (mean * (0.4 + 1.2 * rng.f())) as u64;

        let weights: Vec<f64> = ARTISTS
            .iter()
            .map(|(_, w, trend, _)| (w * (1.0 + trend * years)).max(0.2))
            .collect();
        for _ in 0..plays {
            let ai = rng.pick(&weights);
            let (artist, _, _, songs) = ARTISTS[ai];
            let song_weights: Vec<f64> = songs.iter().map(|s| s.1).collect();
            let si = rng.pick(&song_weights);
            let (title, _, secs) = songs[si];
            let ts = day.and_hms_opt(0, 0, 0).unwrap()
                // Before 22:00 UTC, so no play crosses midnight in Europe/Madrid.
                + Duration::seconds(rng.range(7 * 3600, 22 * 3600) as i64);

            // From 2024 on, about a third of the listening moves to YouTube Music.
            if day.year() >= 2024 && rng.f() < 0.35 {
                let id = format!("{:011}", ai * 10 + si);
                ytm.push(json!({
                    "header": "YouTube Music",
                    "title": format!("Watched {title}"),
                    "titleUrl": format!("https://music.youtube.com/watch?v={id}"),
                    "subtitles": [{"name": format!("{artist} - Topic")}],
                    "time": ts.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                    "products": ["YouTube"],
                }));
            } else {
                // Some plays are skips (under 30 s, not counted) or partial listens.
                let r = rng.f();
                let ms = if r < 0.12 {
                    rng.range(2_000, 29_000)
                } else if r < 0.25 {
                    rng.range(30_000, secs * 1000)
                } else {
                    secs * 1000
                };
                spotify.push(json!({
                    "ts": ts.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                    "platform": "android",
                    "ms_played": ms,
                    "master_metadata_track_name": title,
                    "master_metadata_album_artist_name": artist,
                    "master_metadata_album_album_name": format!("{artist} Sessions"),
                    "skipped": ms < 30_000,
                }));
            }
        }
        day += Duration::days(1);
    }

    fs::create_dir_all(dir.join("spotify"))?;
    fs::write(
        dir.join("spotify/Streaming_History_Audio_2021-2025_0.json"),
        serde_json::to_string(&spotify)?,
    )?;
    fs::write(dir.join("watch-history.json"), serde_json::to_string(&ytm)?)?;
    println!(
        "{} Spotify and {} YouTube Music plays in {}",
        spotify.len(),
        ytm.len(),
        dir.display()
    );
    Ok(())
}
