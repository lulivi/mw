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
/// Inter's cap height, as a fraction of the font size.
const INTER_CAP_H: f32 = 0.727;
/// Height of a Tiny5 Mono digit, as a fraction of the font size: 5 of the 8
/// font pixels of an em.
const NUMBER_CAP_H: f32 = 5.0 / 8.0;
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

/// The sizes of the top lists the image can show. The longer one condenses the
/// header, the main figure and the chart to make room.
pub const IMAGE_TOPS: [usize; 2] = [5, 10];

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

/// The main figure: 53747 -> "53,747"; from a million, 5000571 -> "5.0M",
/// so it keeps a large size.
fn fmt_figure(minutes: u64) -> String {
    if minutes >= 1_000_000 {
        format!("{:.1}M", minutes as f64 / 1_000_000.0)
    } else {
        fmt_int(minutes)
    }
}

/// Digit groups with their commas and decimal point moved closer: in a
/// monospaced font they take a whole cell, which leaves a gap around them. `px`
/// is one font pixel wide.
fn tight_commas(number: &str, px: f32) -> String {
    let mut out = String::new();
    let mut in_group = false;
    for c in number.chars() {
        if c == ',' || c == '.' {
            if in_group {
                out.push_str("</tspan>");
            }
            out.push_str(&format!(
                "<tspan dx='{:.1}'>{c}</tspan><tspan dx='{:.1}'>",
                -px,
                -2.0 * px
            ));
            in_group = true;
        } else {
            out.push(c);
        }
    }
    if in_group {
        out.push_str("</tspan>");
    }
    out
}

/// Builds the full SVG for the image. From top to bottom: the date, the main
/// figure with the totals, the chart and the top lists, which show every entry
/// of `s`. `top` (one of [`IMAGE_TOPS`]) picks the layout: above 5, the date and
/// the totals go in a column beside a smaller figure, and the chart shrinks, so
/// the lists end at the same height in both.
pub fn build_svg(s: &Stats, by: RankBy, accent: &'static str, top: usize) -> Result<String> {
    let condensed = top > IMAGE_TOPS[0];
    let m = Measure::new()?;
    let mut svg = Svg {
        body: String::new(),
        accent,
    };

    let range = range_label(s.from, s.to);
    let number = fmt_figure(minutes(s.total_ms));
    let hours = (s.total_ms as f64 / 3_600_000.0).round() as u64;
    let totals = format!(
        "{} · {}",
        fmt_count(hours, "hour"),
        fmt_count(s.plays, "play")
    );
    let range_size: f32 = if range.chars().count() > 8 {
        56.0
    } else {
        72.0
    };
    // The main figure's font size: the largest, from `size` down, at which its
    // widest form, "999,999", fits in `max_w`; every figure gets that size, so
    // 53,747, 686,596 and 5.0M are all the same height.
    let pixel_w = |size: f32| face_width(&m.number, "0", size) / NUMBER_CELL_PIXELS;
    let figure_size = |mut size: f32, max_w: f32| {
        while face_width(&m.number, "999,999", size) - COMMA_SAVED_PIXELS * pixel_w(size) > max_w
            && size > 80.0
        {
            size -= 8.0;
        }
        size
    };
    // Draws the main figure at `size` with its baseline at `y`, and at `x` with
    // the SVG `anchor`.
    let figure = |svg: &mut Svg, size: f32, x: f32, y: f32, anchor: &str| {
        // Each cell ends in a blank pixel column: move right to make up for it.
        let shift = match anchor {
            "end" => pixel_w(size),
            _ => pixel_w(size) / 2.0,
        };
        svg.body.push_str(&format!(
            "<text x='{:.1}' y='{y}' font-size='{size}' font-family='Tiny5 Mono' font-weight='400' fill='{}' text-anchor='{anchor}'>{}</text>\n",
            x + shift,
            svg.accent,
            tight_commas(&number, pixel_w(size))
        ));
    };

    // Where the content starts, below the band stories keep for their own UI.
    const TOP: f32 = 272.0;
    if condensed {
        // Two columns: the date and the totals on the left, the main figure on
        // the right. The figure always has the width of "999,999" at a fixed
        // size; the left column shrinks its text to fit the rest.
        const COL_END: f32 = W - INNER_LEFT;
        const FIGURE_Y: f32 = 372.0;
        const TOTALS_Y: f32 = 424.0;
        let size = (FIGURE_Y - TOP) / NUMBER_CAP_H;
        let figure_w = face_width(&m.number, "999,999", size) - COMMA_SAVED_PIXELS * pixel_w(size);
        let left_w = COL_END - INNER_LEFT - figure_w - 32.0;
        // The date sits centered between the figure's top and the totals' top;
        // a range of two dates takes two lines.
        let band = (TOTALS_Y - 32.0 * INTER_CAP_H) - TOP;
        let mut date = |size: f32, lines: &[&str]| {
            let gap = size * 1.2;
            let h = gap * (lines.len() - 1) as f32 + size * INTER_CAP_H;
            let first_y = TOP + (band - h) / 2.0 + size * INTER_CAP_H;
            for (i, line) in lines.iter().enumerate() {
                svg.text(
                    INNER_LEFT,
                    first_y + gap * i as f32,
                    size,
                    true,
                    INK,
                    "start",
                    line,
                );
            }
        };
        match range.split_once(" – ") {
            Some((from, to)) => {
                let from = format!("{from} –");
                let widest = m.width(&from, 44.0, true).max(m.width(to, 44.0, true));
                date(44.0_f32.min(44.0 * left_w / widest), &[&from, to]);
            }
            None => {
                let size = range_size.min(range_size * left_w / m.width(&range, range_size, true));
                date(size, &[&range]);
            }
        }
        let totals_size = 32.0_f32.min(32.0 * left_w / m.width(&totals, 32.0, false));
        svg.text(
            INNER_LEFT,
            TOTALS_Y,
            totals_size,
            false,
            QUIET,
            "start",
            &totals,
        );
        figure(&mut svg, size, COL_END, FIGURE_Y, "end");
        svg.text(
            COL_END,
            TOTALS_Y,
            36.0,
            false,
            INK,
            "end",
            "minutes listened",
        );
    } else {
        svg.text(
            CENTER,
            TOP + range_size * INTER_CAP_H,
            range_size,
            true,
            INK,
            "middle",
            &m.fit(&range, range_size, true, RIGHT - LEFT),
        );
        // Smaller than full size, it stays centered on the same band.
        let size = figure_size(352.0, RIGHT - LEFT);
        let gap = (352.0 - size) * NUMBER_CAP_H;
        figure(&mut svg, size, CENTER, 604.0 - gap / 2.0, "middle");
        svg.text(
            CENTER,
            684.0,
            36.0,
            false,
            INK,
            "middle",
            "minutes listened",
        );
        svg.text(CENTER, 728.0, 32.0, false, QUIET, "middle", &totals);
    }

    // Chart by month (or by year for long ranges)
    let (chart_y, bars_h, lists_y) = if condensed {
        (524.0, 64.0, 780.0)
    } else {
        (852.0, 206.0, 1260.0)
    };
    chart(&mut svg, &m, s, chart_y, bars_h);

    // Tops
    const SONGS_X: f32 = CENTER;
    list(
        &mut svg,
        &m,
        "Artists",
        s.distinct_artists,
        &s.top_artists,
        by,
        INNER_LEFT,
        SONGS_X - INNER_LEFT - 20.0,
        lists_y,
    );
    list(
        &mut svg,
        &m,
        "Songs",
        s.distinct_tracks,
        &s.top_tracks,
        by,
        SONGS_X,
        RIGHT - SONGS_X,
        lists_y,
    );

    Ok(format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='{W}' height='{H}' viewBox='0 0 {W} {H}' font-family='Inter'>\n\
         <rect width='100%' height='100%' fill='{BG}'/>\n{}</svg>\n",
        svg.body
    ))
}

/// Draws the chart with the tallest bar's top at `y0` and bars up to `bars_h`
/// tall: minutes per month, or per year for more than 24 months. The bars fill
/// the width up to a maximum, centered as a group; the top one shows its value
/// above it in the accent color, and the title goes under the labels.
fn chart(svg: &mut Svg, m: &Measure, s: &Stats, y0: f32, bars_h: f32) {
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
        "minutes per year"
    } else {
        "minutes per month"
    };

    let max = bars.iter().map(|b| b.1).max().unwrap_or(0).max(1);
    let max_i = bars.iter().position(|b| b.1 == max).unwrap_or(0);
    let top = y0;
    let base = top + bars_h;
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
    svg.text(CENTER, base + 96.0, 36.0, false, INK, "middle", title);
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

/// Draws a top list as a column, under its title and the `total` of distinct
/// entries. Songs add a second line with their artist and the figure they rank
/// by. Ranks and single-line names sit halfway down a two-line song row.
#[allow(clippy::too_many_arguments)]
fn list(
    svg: &mut Svg,
    m: &Measure,
    title: &str,
    total: usize,
    items: &[Ranked],
    by: RankBy,
    x0: f32,
    w: f32,
    y0: f32,
) {
    const NAME: f32 = 36.0;
    const INFO: f32 = 28.0;
    const INFO_DY: f32 = 40.0;
    const ROW_H: f32 = 96.0;
    const RANK: f32 = 42.0;
    const ARTIST: f32 = 40.0;

    svg.body.push_str(&format!(
        "<text x='{x0:.1}' y='{y0:.1}' font-size='44' font-weight='700' fill='{INK}'>{title}<tspan font-size='32' font-weight='400' fill='{QUIET}' dx='12'>({} total)</tspan></text>\n",
        fmt_int(total as u64)
    ));
    // Wide enough for the last rank: "10" needs more room than "5".
    let rank_w = (m.width(&items.len().to_string(), RANK, true) + 16.0).max(50.0);
    let x = x0 + rank_w;
    let max_w = w - rank_w;
    let mut y = y0 + 64.0;
    for (i, item) in items.iter().enumerate() {
        let row_mid = y + (INFO_DY - NAME * INTER_CAP_H) / 2.0;
        let centered = |size: f32| row_mid + size * INTER_CAP_H / 2.0;
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
        if let Some(detail) = &item.detail {
            let (artist, number, unit) = song_info(m, detail, item, by, INFO, max_w);
            let accent = svg.accent;
            svg.body.push_str(&format!(
                "<text x='{x:.1}' y='{:.1}' font-size='{INFO}' fill='{QUIET}'><tspan font-weight='700' fill='{accent}'>{}</tspan>{unit}{INFO_SEP}{}</text>\n",
                y + INFO_DY,
                esc(&number),
                esc(&artist)
            ));
        }
        y += ROW_H;
    }
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
    fn tightens_the_decimal_point() {
        assert_eq!(
            tight_commas("5.0M", 10.0),
            "5<tspan dx='-10.0'>.</tspan><tspan dx='-20.0'>0M</tspan>"
        );
    }

    #[test]
    fn main_figure_shortens_from_a_million() {
        assert_eq!(fmt_figure(53_747), "53,747");
        assert_eq!(fmt_figure(999_999), "999,999");
        assert_eq!(fmt_figure(1_000_000), "1.0M");
        assert_eq!(fmt_figure(5_000_571), "5.0M");
        assert_eq!(fmt_figure(3_111_333), "3.1M");
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
