//! Builds the image: an SVG assembled as text and converted to PNG with resvg.
//!
//! Mobile (story) format: 1080×1920. The 250 px bands at the top and bottom are
//! where stories show their own UI: the top one is left empty, and only the
//! footer enters the bottom one, next to the reply box.

use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate};
use resvg::{tiny_skia, usvg};

use crate::stats::{RankBy, Ranked, Stats};

/// Inter (SIL Open Font License 1.1, see assets/OFL-Inter.txt), embedded so the image
/// looks the same on every machine.
const FONT_REGULAR: &[u8] = include_bytes!("../assets/Inter-Regular.otf");
/// Inter Bold, embedded like [`FONT_REGULAR`].
const FONT_BOLD: &[u8] = include_bytes!("../assets/Inter-Bold.otf");
/// Tiny5 Mono Regular (SIL Open Font License 1.1, see assets/OFL-Tiny5Mono.txt),
/// used only for the main figure. Its pixel rows are crisp at multiples of 8 px.
const FONT_NUMBER: &[u8] = include_bytes!("../assets/Tiny5Mono-Regular.otf");

/// Accent colors of the image (`--color`), by name, as SVG hex strings.
pub const ACCENTS: [(&str, &str); 7] = [
    ("orange", "#FF6B4A"),
    ("purple", "#B388FF"),
    ("blue", "#5AA9FF"),
    ("green", "#3DDC97"),
    ("pink", "#FF6FB5"),
    ("red", "#FF5A5F"),
    ("gray", "#E3E7EB"),
];

/// The accent color with this name.
pub fn accent(name: &str) -> Option<&'static str> {
    ACCENTS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, hex)| *hex)
}

/// Image width in px.
const W: f32 = 1080.0;
/// Image height in px.
const H: f32 = 1920.0;
/// Left limit of the text.
const LEFT: f32 = 60.0;
/// Right limit of the text.
const RIGHT: f32 = W - 60.0;
/// Horizontal center of the image.
const CENTER: f32 = W / 2.0;
/// Width of a Tiny5 Mono cell in font pixels. Each one ends in a blank pixel column,
/// so the main figure is shifted half a pixel to the right to look centered.
const NUMBER_CELL_PIXELS: f32 = 6.0;
/// Font pixels that [`tight_commas`] saves per comma.
const COMMA_SAVED_PIXELS: f32 = 3.0;
/// Where the chart and the artists column start; the chart ends as far from the right.
const INNER_LEFT: f32 = 90.0;

/// Background color.
const BG: &str = "#101418";
/// Color of the date, titles, names and ranks.
const INK: &str = "#AEB5BD";
/// Color of secondary text.
const QUIET: &str = "#8A929D";
/// Color of the chart bars other than the top one.
const BAR: &str = "#2C343D";

/// How many rows of each top list fit in the image.
const IMAGE_TOP: usize = 5;

/// Between the figure and the artist in a song's second line.
const INFO_SEP: &str = " · ";

/// Month abbreviations for dates.
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
/// Month labels for the chart.
const MONTH_INITIALS: [&str; 12] = ["J", "F", "M", "A", "M", "J", "J", "A", "S", "O", "N", "D"];

// ---------- text formatting ----------

/// 42310 -> "42,310"
pub fn fmt_int(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// (1, "play") -> "1 play"; (1200, "play") -> "1,200 plays"
pub fn fmt_count(n: u64, noun: &str) -> String {
    let s = if n == 1 { "" } else { "s" };
    format!("{} {noun}{s}", fmt_int(n))
}

/// Milliseconds to whole minutes, rounded to the nearest.
pub fn minutes(ms: u64) -> u64 {
    (ms + 30_000) / 60_000
}

/// "Jun 1, 2024"
fn fmt_date(d: NaiveDate) -> String {
    format!("{} {}, {}", MONTHS[d.month0() as usize], d.day(), d.year())
}

/// "Jun 1"
fn fmt_day_month(d: NaiveDate) -> String {
    format!("{} {}", MONTHS[d.month0() as usize], d.day())
}

/// "2025" for a whole calendar year, "Jun 1, 2024" for a single day; otherwise
/// "Jun 1, 2024 – May 31, 2025".
pub fn range_label(from: NaiveDate, to: NaiveDate) -> String {
    let whole_year = from.year() == to.year()
        && from.month() == 1
        && from.day() == 1
        && to.month() == 12
        && to.day() == 31;
    if whole_year {
        from.year().to_string()
    } else if from == to {
        fmt_date(from)
    } else {
        format!("{} – {}", fmt_date(from), fmt_date(to))
    }
}

/// Escapes text for use inside the SVG.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

// ---------- text measuring (to truncate with "…") ----------

/// The embedded fonts, parsed to measure text without rendering it.
struct Measure {
    regular: ttf_parser::Face<'static>,
    bold: ttf_parser::Face<'static>,
    number: ttf_parser::Face<'static>,
}

/// Width in px of `text` at `size` in a font. Characters the font lacks count as
/// 0.6 em.
fn face_width(face: &ttf_parser::Face, text: &str, size: f32) -> f32 {
    let upem = face.units_per_em() as f32;
    let units: f32 = text
        .chars()
        .map(|c| {
            face.glyph_index(c)
                .and_then(|g| face.glyph_hor_advance(g))
                .unwrap_or((upem * 0.6) as u16) as f32
        })
        .sum();
    units * size / upem
}

impl Measure {
    /// Parses the embedded fonts.
    fn new() -> Result<Self> {
        Ok(Self {
            regular: ttf_parser::Face::parse(FONT_REGULAR, 0).context("regular font")?,
            bold: ttf_parser::Face::parse(FONT_BOLD, 0).context("bold font")?,
            number: ttf_parser::Face::parse(FONT_NUMBER, 0).context("number font")?,
        })
    }

    /// Width in px of `text` in Inter, regular or bold.
    fn width(&self, text: &str, size: f32, bold: bool) -> f32 {
        face_width(if bold { &self.bold } else { &self.regular }, text, size)
    }

    /// Truncates the text so it fits in `max` px, adding "…".
    fn fit(&self, text: &str, size: f32, bold: bool, max: f32) -> String {
        if self.width(text, size, bold) <= max {
            return text.to_string();
        }
        let chars: Vec<char> = text.chars().collect();
        let mut n = chars.len();
        while n > 0 {
            let candidate: String =
                chars[..n].iter().collect::<String>().trim_end().to_string() + "…";
            if self.width(&candidate, size, bold) <= max {
                return candidate;
            }
            n -= 1;
        }
        "…".into()
    }
}

// ---------- SVG ----------

/// The SVG being assembled.
struct Svg {
    /// Elements drawn so far.
    body: String,
    /// Accent color chosen by the user.
    accent: &'static str,
}

impl Svg {
    /// Adds a line of Inter text with `y` as its baseline. `anchor` is the SVG
    /// `text-anchor`: "start", "middle" or "end".
    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        x: f32,
        y: f32,
        size: f32,
        bold: bool,
        fill: &str,
        anchor: &str,
        content: &str,
    ) {
        self.body.push_str(&format!(
            "<text x='{x:.1}' y='{y:.1}' font-size='{size}' font-weight='{}' fill='{fill}' text-anchor='{anchor}'>{}</text>\n",
            if bold { 700 } else { 400 },
            esc(content)
        ));
    }

    /// Adds a rectangle with corners of radius `r`.
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, fill: &str) {
        self.body.push_str(&format!(
            "<rect x='{x:.1}' y='{y:.1}' width='{w:.1}' height='{h:.1}' rx='{r}' fill='{fill}'/>\n"
        ));
    }
}

/// Digit groups with their commas moved closer: in a monospaced font a comma
/// takes a whole cell, which leaves a gap around it. `px` is one font pixel wide.
fn tight_commas(number: &str, px: f32) -> String {
    number
        .split(',')
        .enumerate()
        .map(|(i, group)| {
            if i == 0 {
                group.to_string()
            } else {
                format!(
                    "<tspan dx='{:.1}'>,</tspan><tspan dx='{:.1}'>{group}</tspan>",
                    -px,
                    -2.0 * px
                )
            }
        })
        .collect()
}

/// Builds the full SVG for the image. From top to bottom: the header, the main
/// figure, the chart, the top lists and the footer, which is centered between the
/// lists and the bottom of the image. The footer's last line shows the split by
/// source and, if needed, the estimate note.
pub fn build_svg(s: &Stats, by: RankBy, accent: &'static str) -> Result<String> {
    let m = Measure::new()?;
    let mut svg = Svg {
        body: String::new(),
        accent,
    };
    let estimated = s.estimated_ms > 0;

    // Header
    svg.text(CENTER, 279.0, 38.0, true, INK, "middle", "YOUR MUSIC");
    let range = range_label(s.from, s.to);
    let range_size = if range.chars().count() > 8 {
        56.0
    } else {
        72.0
    };
    svg.text(
        CENTER,
        350.0,
        range_size,
        true,
        INK,
        "middle",
        &m.fit(&range, range_size, true, RIGHT - LEFT),
    );

    // Main figure
    let number = fmt_int(minutes(s.total_ms));
    let mut big_size: f32 = 240.0;
    let commas = number.matches(',').count() as f32;
    let pixel_w = |size: f32| face_width(&m.number, "0", size) / NUMBER_CELL_PIXELS;
    while face_width(&m.number, &number, big_size) - commas * COMMA_SAVED_PIXELS * pixel_w(big_size)
        > RIGHT - LEFT
        && big_size > 80.0
    {
        big_size -= 8.0;
    }
    svg.body.push_str(&format!(
        "<text x='{:.1}' y='557' font-size='{big_size}' font-family='Tiny5 Mono' font-weight='400' fill='{}' text-anchor='middle'>{}</text>\n",
        CENTER + pixel_w(big_size) / 2.0,
        svg.accent,
        tight_commas(&number, pixel_w(big_size))
    ));
    svg.text(
        CENTER,
        636.0,
        44.0,
        false,
        INK,
        "middle",
        "minutes listened",
    );
    let hours = (s.total_ms as f64 / 3_600_000.0).round() as u64;
    svg.text(
        CENTER,
        682.0,
        36.0,
        false,
        QUIET,
        "middle",
        &format!(
            "{} · {}",
            fmt_count(hours, "hour"),
            fmt_count(s.plays, "play")
        ),
    );

    // Chart by month (or by year for long ranges)
    chart(&mut svg, &m, s, 778.0);

    // Tops
    const LISTS_Y: f32 = 1140.0;
    const SONGS_X: f32 = CENTER;
    let artists_end = list(
        &mut svg,
        &m,
        "Top artists",
        &s.top_artists,
        by,
        INNER_LEFT,
        SONGS_X - INNER_LEFT - 20.0,
        LISTS_Y,
    );
    let songs_end = list(
        &mut svg,
        &m,
        "Top songs",
        &s.top_tracks,
        by,
        SONGS_X,
        RIGHT - SONGS_X,
        LISTS_Y,
    );

    // Extra facts and footer, below the top lists
    const FOOT_CAP_H: f32 = 25.0;
    const FOOT_LINE_H: f32 = 44.0;
    let lists_end = artists_end.max(songs_end);
    let foot_h = FOOT_CAP_H + FOOT_LINE_H;
    let mut y = lists_end + (H - lists_end - foot_h) / 2.0 + FOOT_CAP_H;
    let mut extra = format!(
        "{} · {}",
        fmt_count(s.distinct_artists as u64, "artist"),
        fmt_count(s.distinct_tracks as u64, "song")
    );
    if let Some((day, _)) = s.top_day {
        extra.push_str(&format!(" · top day: {}", fmt_day_month(day)));
    }
    svg.text(
        CENTER,
        y,
        34.0,
        false,
        QUIET,
        "middle",
        &m.fit(&extra, 34.0, false, RIGHT - LEFT),
    );

    let split = if s.by_source.len() > 1 {
        s.by_source
            .iter()
            .map(|(src, ms)| {
                let pct = (*ms as f64 * 100.0 / s.total_ms as f64).round();
                format!("{pct}% {}", src.label())
            })
            .collect::<Vec<_>>()
            .join(" · ")
    } else {
        s.by_source
            .first()
            .map(|(src, _)| format!("{} only", src.label()))
            .unwrap_or_default()
    };
    let foot = if estimated {
        format!("{split} · estimated minutes")
    } else {
        split
    };
    y += FOOT_LINE_H;
    svg.text(
        CENTER,
        y,
        34.0,
        false,
        QUIET,
        "middle",
        &m.fit(&foot, 34.0, false, RIGHT - LEFT),
    );

    Ok(format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='{W}' height='{H}' viewBox='0 0 {W} {H}' font-family='Inter'>\n\
         <rect width='100%' height='100%' fill='{BG}'/>\n{}</svg>\n",
        svg.body
    ))
}

/// Draws the chart at `y0`: minutes per month, or per year for more than 24 months.
/// The bars fill the width up to a maximum, centered as a group, and the top one
/// shows its value above it in the accent color.
fn chart(svg: &mut Svg, m: &Measure, s: &Stats, y0: f32) {
    let years = s.by_year();
    let bars: Vec<(String, u64)> = match &years {
        Some(years) => years.iter().map(|(y, ms)| (y.to_string(), *ms)).collect(),
        None => s
            .by_month
            .iter()
            .map(|((_, mo), ms)| (MONTH_INITIALS[(*mo - 1) as usize].to_string(), *ms))
            .collect(),
    };

    let title = if years.is_some() {
        "Minutes per year"
    } else {
        "Minutes per month"
    };
    svg.text(CENTER, y0, 38.0, true, INK, "middle", title);

    let max = bars.iter().map(|b| b.1).max().unwrap_or(0).max(1);
    let max_i = bars.iter().position(|b| b.1 == max).unwrap_or(0);
    let top = y0 + 66.0;
    let base = y0 + 226.0;
    const GAP: f32 = 16.0;
    const MAX_BAR_W: f32 = 180.0;
    let n = bars.len() as f32;
    let area = W - 2.0 * INNER_LEFT;
    let bw = ((area + GAP) / n - GAP).min(MAX_BAR_W);
    let slot = bw + GAP;
    let start = CENTER - (n * slot - GAP) / 2.0;
    let (labels, label_size) = chart_labels(m, bars.iter().map(|b| b.0.clone()).collect(), slot);

    for (i, ((_, ms), label)) in bars.iter().zip(&labels).enumerate() {
        let cx = start + slot * i as f32 + bw / 2.0;
        let h = ((*ms as f32 / max as f32) * (base - top)).max(if *ms > 0 { 6.0 } else { 2.0 });
        let fill = if i == max_i { svg.accent } else { BAR };
        svg.rect(cx - bw / 2.0, base - h, bw, h, (bw / 6.0).min(8.0), fill);
        let label_fill = if i == max_i { svg.accent } else { QUIET };
        svg.text(
            cx,
            base + 44.0,
            label_size,
            i == max_i,
            label_fill,
            "middle",
            label,
        );
    }
    let cx = start + slot * max_i as f32 + bw / 2.0;
    svg.text(
        cx,
        top - 14.0,
        36.0,
        true,
        svg.accent,
        "middle",
        &fmt_int(minutes(max)),
    );
}

/// The chart's labels and their font size, so that none is wider than its
/// `slot`. Years that don't fit become "’19"; if those still
/// don't fit, the font shrinks. Widths are measured in bold, as the highlighted
/// label is.
fn chart_labels(m: &Measure, mut labels: Vec<String>, slot: f32) -> (Vec<String>, f32) {
    const SIZE: f32 = 34.0;
    let widest = |ls: &[String]| {
        ls.iter()
            .map(|l| m.width(l, SIZE, true))
            .fold(0.0, f32::max)
    };
    if widest(&labels) > slot {
        // Only years are this wide: month labels are single initials.
        labels = labels
            .iter()
            .map(|l| format!("’{}", &l[l.len() - 2..]))
            .collect();
    }
    let w = widest(&labels);
    (labels, if w > slot { SIZE * slot / w } else { SIZE })
}

/// The number, unit and artist for a song's second line: "**7** plays · Artist"
/// or "**21** min · Artist", depending on what the list ranks by. The artist is
/// cut on the right if it doesn't fit after the figure.
fn song_info(
    m: &Measure,
    artist: &str,
    item: &Ranked,
    by: RankBy,
    size: f32,
    max_w: f32,
) -> (String, String, &'static str) {
    let (number, unit) = match by {
        RankBy::Plays if item.plays == 1 => (item.plays, " play"),
        RankBy::Plays => (item.plays, " plays"),
        RankBy::Minutes => (minutes(item.ms), " min"),
    };
    let number = fmt_int(number);
    let artist_w = max_w
        - m.width(INFO_SEP, size, false)
        - m.width(&number, size, true)
        - m.width(unit, size, false);
    (m.fit(artist, size, false, artist_w), number, unit)
}

/// Draws a top list as a column and returns the y where it ends. Songs add a
/// second line with their artist and the figure they rank by. Ranks and single-line
/// names sit halfway down a two-line song row.
#[allow(clippy::too_many_arguments)]
fn list(
    svg: &mut Svg,
    m: &Measure,
    title: &str,
    items: &[Ranked],
    by: RankBy,
    x0: f32,
    w: f32,
    y0: f32,
) -> f32 {
    const NAME: f32 = 36.0;
    const INFO: f32 = 28.0;
    const INFO_DY: f32 = 40.0;
    const ROW_H: f32 = 96.0;
    const RANK: f32 = 42.0;
    const ARTIST: f32 = 40.0;
    const RANK_W: f32 = 50.0;
    /// Inter's cap height, as a fraction of the font size.
    const CAP_H: f32 = 0.727;

    svg.text(x0, y0, 44.0, true, INK, "start", title);
    let x = x0 + RANK_W;
    let max_w = w - RANK_W;
    let mut y = y0 + 64.0;
    let mut end = y;
    for (i, item) in items.iter().take(IMAGE_TOP).enumerate() {
        let row_mid = y + (INFO_DY - NAME * CAP_H) / 2.0;
        let centered = |size: f32| row_mid + size * CAP_H / 2.0;
        svg.text(
            x0,
            centered(RANK),
            RANK,
            true,
            INK,
            "start",
            &(i + 1).to_string(),
        );
        let (name_size, name_y) = match item.detail {
            Some(_) => (NAME, y),
            None => (ARTIST, centered(ARTIST)),
        };
        let name = m.fit(&item.name, name_size, false, max_w);
        svg.text(x, name_y, name_size, false, INK, "start", &name);
        end = name_y;
        if let Some(detail) = &item.detail {
            let (artist, number, unit) = song_info(m, detail, item, by, INFO, max_w);
            let accent = svg.accent;
            svg.body.push_str(&format!(
                "<text x='{x:.1}' y='{:.1}' font-size='{INFO}' fill='{QUIET}'><tspan font-weight='700' fill='{accent}'>{}</tspan>{unit}{INFO_SEP}{}</text>\n",
                y + INFO_DY,
                esc(&number),
                esc(&artist)
            ));
            end = y + INFO_DY;
        }
        y += ROW_H;
    }
    end
}

/// Converts the SVG to PNG.
pub fn png(svg: &str) -> Result<Vec<u8>> {
    let mut opt = usvg::Options::default();
    {
        let db = Arc::make_mut(&mut opt.fontdb);
        db.load_font_data(FONT_REGULAR.to_vec());
        db.load_font_data(FONT_BOLD.to_vec());
        db.load_font_data(FONT_NUMBER.to_vec());
    }
    let tree = usvg::Tree::from_str(svg, &opt).context("invalid SVG")?;
    let mut pixmap =
        tiny_skia::Pixmap::new(W as u32, H as u32).context("cannot create the image")?;
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
    pixmap.encode_png().context("cannot encode the image")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_numbers() {
        assert_eq!(fmt_int(0), "0");
        assert_eq!(fmt_int(999), "999");
        assert_eq!(fmt_int(42310), "42,310");
        assert_eq!(fmt_int(1234567), "1,234,567");
        assert_eq!(fmt_count(1, "play"), "1 play");
        assert_eq!(fmt_count(0, "play"), "0 plays");
        assert_eq!(fmt_count(1200, "hour"), "1,200 hours");
    }

    #[test]
    fn song_info_shows_the_ranked_figure() {
        let m = Measure::new().unwrap();
        let item = Ranked {
            name: String::new(),
            detail: None,
            ms: 60_000,
            plays: 7,
        };
        let info = |by, w| song_info(&m, "Repion", &item, by, 28.0, w);
        let parts = |a: &str, n: &str, u| (a.to_string(), n.to_string(), u);
        assert_eq!(
            info(RankBy::Plays, 10_000.0),
            parts("Repion", "7", " plays")
        );
        assert_eq!(
            info(RankBy::Minutes, 10_000.0),
            parts("Repion", "1", " min")
        );
        let narrow = m.width("Repion · 7 plays", 28.0, false) - 40.0;
        let (artist, number, unit) = info(RankBy::Plays, narrow);
        assert!(artist.ends_with('…'), "{artist}");
        assert_eq!((number.as_str(), unit), ("7", " plays"));
    }

    #[test]
    fn chart_labels_fit_their_slots() {
        let m = Measure::new().unwrap();
        // The slot `chart` gives each of `n` bars.
        let slot = |n: i32| (W - 2.0 * INNER_LEFT + 16.0) / n as f32;
        let labels = |n: i32| {
            chart_labels(
                &m,
                (0..n).map(|i| (2016 + i).to_string()).collect(),
                slot(n),
            )
        };

        let (ls, size) = labels(10);
        assert_eq!((ls[9].as_str(), size), ("2025", 34.0));
        let (ls, size) = labels(11);
        assert_eq!((ls[10].as_str(), size), ("’26", 34.0));
        let (ls, size) = labels(40);
        assert!(size < 34.0);
        for l in &ls {
            assert!(m.width(l, size, true) <= slot(40) + 0.01, "{l} at {size}");
        }
    }

    #[test]
    fn tightens_each_comma() {
        assert_eq!(tight_commas("73", 10.0), "73");
        assert_eq!(
            tight_commas("1,234,567", 10.0),
            "1<tspan dx='-10.0'>,</tspan><tspan dx='-20.0'>234</tspan>\
             <tspan dx='-10.0'>,</tspan><tspan dx='-20.0'>567</tspan>"
        );
    }

    #[test]
    fn range_labels() {
        let d = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
        assert_eq!(range_label(d(2025, 1, 1), d(2025, 12, 31)), "2025");
        assert_eq!(range_label(d(2026, 11, 6), d(2026, 11, 6)), "Nov 6, 2026");
        assert_eq!(
            range_label(d(2024, 6, 1), d(2025, 5, 31)),
            "Jun 1, 2024 – May 31, 2025"
        );
    }
}
