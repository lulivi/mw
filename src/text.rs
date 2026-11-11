//! The recap as text: what `mw` prints and the web page shows under the image.

use std::fmt::Write;

use crate::render::{fmt_count, fmt_int, minutes, range_label};
use crate::stats::Stats;

/// Appends a line to the text; writing to a `String` can't fail.
macro_rules! w {
    ($out:expr, $($arg:tt)*) => {
        let _ = writeln!($out, $($arg)*);
    };
}

/// The full recap as the terminal shows it, including what the image leaves out (every
/// month, longer top lists and the favorite hour).
pub fn stats(s: &Stats) -> String {
    let mut out = String::new();
    let approx = if s.estimated_ms > 0 { "≈ " } else { "" };
    w!(out, "== {} ==", range_label(s.from, s.to));
    w!(
        out,
        "Minutes listened: {approx}{}  ({})",
        fmt_int(minutes(s.total_ms)),
        fmt_count(s.plays, "play")
    );
    if s.estimated_ms > 0 {
        w!(
            out,
            "  of which estimated (YouTube Music): {}",
            fmt_int(minutes(s.estimated_ms))
        );
    }
    for (src, ms) in &s.by_source {
        let pct = *ms as f64 * 100.0 / s.total_ms as f64;
        w!(
            out,
            "  {}: {} min ({pct:.0}%)",
            src.label(),
            fmt_int(minutes(*ms))
        );
    }

    match s.by_year() {
        Some(years) => {
            w!(out, "\nMinutes per year:");
            for (y, ms) in years {
                w!(out, "  {y}  {:>8} min", fmt_int(minutes(ms)));
            }
        }
        None => {
            w!(out, "\nMinutes per month:");
            for ((y, m), ms) in &s.by_month {
                w!(out, "  {y}-{m:02}  {:>8} min", fmt_int(minutes(*ms)));
            }
        }
    }

    w!(out, "\nTop artists:");
    for (i, a) in s.top_artists.iter().enumerate() {
        w!(
            out,
            "  {:>2}. {}  ({} min, {})",
            i + 1,
            a.name,
            fmt_int(minutes(a.ms)),
            fmt_count(a.plays, "play")
        );
    }
    w!(out, "\nTop songs:");
    for (i, t) in s.top_tracks.iter().enumerate() {
        let artist = t.detail.as_deref().unwrap_or("");
        w!(
            out,
            "  {:>2}. {} · {}  ({} min, {})",
            i + 1,
            t.name,
            artist,
            fmt_int(minutes(t.ms)),
            fmt_count(t.plays, "play")
        );
    }

    w!(
        out,
        "\nDistinct artists: {} · Distinct songs: {}",
        fmt_int(s.distinct_artists as u64),
        fmt_int(s.distinct_tracks as u64)
    );
    if let Some((day, ms)) = s.top_day {
        w!(out, "Top day: {day} ({} min)", fmt_int(minutes(ms)));
    }
    if let Some((h, _)) = s.top_hour {
        w!(out, "Favorite hour: {h:02}:00–{:02}:00", (h + 1) % 24);
    }
    out
}
