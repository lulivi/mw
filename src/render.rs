//! Builds the image: an SVG assembled as text and converted to PNG with resvg.
//!
//! Mobile (story) format: 1080×1920. The 250 px bands at the top and bottom are
//! left empty because that's where stories show their own UI.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate};
use resvg::{tiny_skia, usvg};

use crate::stats::{Ranked, Stats};

/// Inter (SIL Open Font License 1.1, see assets/OFL.txt), embedded so the image
/// looks the same on every machine.
const FONT_REGULAR: &[u8] = include_bytes!("../assets/Inter-Regular.otf");
const FONT_BOLD: &[u8] = include_bytes!("../assets/Inter-Bold.otf");
/// Tiny5 Mono Bold (SIL Open Font License 1.1, see assets/OFL-Tiny5Mono.txt),
/// used only for the main figure. Its pixel rows are crisp at multiples of 8 px.
const FONT_NUMBER: &[u8] = include_bytes!("../assets/Tiny5Mono-Bold.otf");

const W: f32 = 1080.0;
const H: f32 = 1920.0;
const LEFT: f32 = 90.0;
const RIGHT: f32 = W - 90.0;

const BG: &str = "#101418";
const INK: &str = "#F4F5F7";
const QUIET: &str = "#B8C0C8";
const ACCENT: &str = "#FF6B4A";
const BAR: &str = "#2C343D";

/// How many rows of each top list fit in the image.
const IMAGE_TOP: usize = 5;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
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

pub fn minutes(ms: u64) -> u64 {
    (ms + 30_000) / 60_000
}

fn fmt_date(d: NaiveDate) -> String {
    format!("{} {}, {}", MONTHS[d.month0() as usize], d.day(), d.year())
}

fn fmt_day_month(d: NaiveDate) -> String {
    format!("{} {}", MONTHS[d.month0() as usize], d.day())
}

/// "2025" for a whole calendar year; otherwise "Jun 1, 2024 – May 31, 2025".
pub fn range_label(from: NaiveDate, to: NaiveDate) -> String {
    let whole_year = from.year() == to.year()
        && from.month() == 1
        && from.day() == 1
        && to.month() == 12
        && to.day() == 31;
    if whole_year {
        from.year().to_string()
    } else {
        format!("{} – {}", fmt_date(from), fmt_date(to))
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

// ---------- text measuring (to truncate with "…") ----------

struct Measure {
    regular: ttf_parser::Face<'static>,
    bold: ttf_parser::Face<'static>,
    number: ttf_parser::Face<'static>,
}

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
    fn new() -> Result<Self> {
        Ok(Self {
            regular: ttf_parser::Face::parse(FONT_REGULAR, 0).context("regular font")?,
            bold: ttf_parser::Face::parse(FONT_BOLD, 0).context("bold font")?,
            number: ttf_parser::Face::parse(FONT_NUMBER, 0).context("number font")?,
        })
    }

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

struct Svg {
    body: String,
}

impl Svg {
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

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, fill: &str) {
        self.body.push_str(&format!(
            "<rect x='{x:.1}' y='{y:.1}' width='{w:.1}' height='{h:.1}' rx='{r}' fill='{fill}'/>\n"
        ));
    }
}

/// Builds the full SVG for the image.
pub fn build_svg(s: &Stats) -> Result<String> {
    let m = Measure::new()?;
    let mut svg = Svg {
        body: String::new(),
    };
    let estimated = s.estimated_ms > 0;

    // Header
    svg.text(LEFT, 285.0, 34.0, true, ACCENT, "start", "YOUR MUSIC");
    let range = range_label(s.from, s.to);
    let range_size = if range.chars().count() > 8 {
        50.0
    } else {
        64.0
    };
    svg.text(
        LEFT,
        348.0,
        range_size,
        true,
        INK,
        "start",
        &m.fit(&range, range_size, true, RIGHT - LEFT),
    );

    // Main figure
    let number = fmt_int(minutes(s.total_ms));
    let mut big_size: f32 = 144.0;
    while face_width(&m.number, &number, big_size) > RIGHT - LEFT && big_size > 80.0 {
        big_size -= 8.0;
    }
    svg.body.push_str(&format!(
        "<text x='{LEFT}' y='495' font-size='{big_size}' font-family='Tiny5 Mono' font-weight='700' fill='{INK}'>{}</text>\n",
        esc(&number)
    ));
    svg.text(LEFT, 564.0, 38.0, false, INK, "start", "minutes listened");
    let hours = (s.total_ms as f64 / 3_600_000.0).round() as u64;
    svg.text(
        LEFT,
        606.0,
        32.0,
        false,
        QUIET,
        "start",
        &format!("{} hours · {} plays", fmt_int(hours), fmt_int(s.plays)),
    );

    // Chart by month (or by year for long ranges)
    chart(&mut svg, s, 664.0);

    // Tops
    let y = list(&mut svg, &m, "Top artists", &s.top_artists, 926.0);
    let y = list(&mut svg, &m, "Top songs", &s.top_tracks, y + 72.0);

    // Extra facts and footer, below the top lists
    let mut y = y + 52.0;
    let mut extra = format!(
        "{} artists · {} songs",
        fmt_int(s.distinct_artists as u64),
        fmt_int(s.distinct_tracks as u64)
    );
    if let Some((day, _)) = s.top_day {
        extra.push_str(&format!(" · top day: {}", fmt_day_month(day)));
    }
    svg.text(
        LEFT,
        y,
        32.0,
        false,
        QUIET,
        "start",
        &m.fit(&extra, 32.0, false, RIGHT - LEFT),
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
    // One line: split by source and, if needed, the estimate note.
    let foot = if estimated {
        format!("{split} · estimated minutes")
    } else {
        split
    };
    y += 40.0;
    svg.text(
        LEFT,
        y,
        32.0,
        false,
        QUIET,
        "start",
        &m.fit(&foot, 32.0, false, RIGHT - LEFT),
    );

    Ok(format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='{W}' height='{H}' viewBox='0 0 {W} {H}' font-family='Inter'>\n\
         <rect width='100%' height='100%' fill='{BG}'/>\n{}</svg>\n",
        svg.body
    ))
}

fn chart(svg: &mut Svg, s: &Stats, y0: f32) {
    // More than 24 months: group by year.
    let by_year = s.by_month.len() > 24;
    let mut bars: Vec<(String, u64)> = Vec::new();
    if by_year {
        for ((y, _), ms) in &s.by_month {
            match bars.last_mut() {
                Some((label, total)) if *label == y.to_string() => *total += ms,
                _ => bars.push((y.to_string(), *ms)),
            }
        }
    } else {
        bars = s
            .by_month
            .iter()
            .map(|((_, mo), ms)| (MONTH_INITIALS[(*mo - 1) as usize].to_string(), *ms))
            .collect();
    }

    let title = if by_year {
        "Minutes per year"
    } else {
        "Minutes per month"
    };
    svg.text(LEFT, y0, 34.0, true, INK, "start", title);

    let max = bars.iter().map(|b| b.1).max().unwrap_or(0).max(1);
    let max_i = bars.iter().position(|b| b.1 == max).unwrap_or(0);
    let top = y0 + 50.0;
    let base = y0 + 160.0;
    let area = RIGHT - LEFT;
    let slot = area / bars.len() as f32;
    let bw = (slot * 0.7).min(80.0);

    for (i, (label, ms)) in bars.iter().enumerate() {
        let cx = LEFT + slot * (i as f32 + 0.5);
        let h = ((*ms as f32 / max as f32) * (base - top)).max(if *ms > 0 { 6.0 } else { 2.0 });
        let fill = if i == max_i { ACCENT } else { BAR };
        svg.rect(cx - bw / 2.0, base - h, bw, h, (bw / 6.0).min(8.0), fill);
        let label_fill = if i == max_i { INK } else { QUIET };
        svg.text(
            cx,
            base + 42.0,
            32.0,
            i == max_i,
            label_fill,
            "middle",
            label,
        );
    }
    // Value of the top month (or year), above its bar.
    let cx = LEFT + slot * (max_i as f32 + 0.5);
    svg.text(
        cx,
        top - 14.0,
        32.0,
        true,
        ACCENT,
        "middle",
        &fmt_int(minutes(max)),
    );
}

/// Draws a top list and returns the y where it ends.
fn list(svg: &mut Svg, m: &Measure, title: &str, items: &[Ranked], y0: f32) -> f32 {
    svg.text(LEFT, y0, 40.0, true, ACCENT, "start", title);
    let row_h = 56.0;
    let mut y = y0 + 60.0;
    for (i, item) in items.iter().take(IMAGE_TOP).enumerate() {
        let num = format!("{}", i + 1);
        svg.text(LEFT, y, 36.0, true, ACCENT, "start", &num);

        let mins = format!("{} min", fmt_int(minutes(item.ms)));
        svg.text(RIGHT, y, 32.0, false, QUIET, "end", &mins);

        let x = LEFT + 52.0;
        let max_w = RIGHT - m.width(&mins, 32.0, false) - 28.0 - x;
        let name_w = m.width(&item.name, 36.0, false);
        match &item.detail {
            Some(detail) if name_w < max_w * 0.75 => {
                let rest = max_w - name_w;
                let detail = m.fit(&format!("\u{a0}·\u{a0}{detail}"), 32.0, false, rest);
                svg.body.push_str(&format!(
                    "<text x='{x:.1}' y='{y:.1}' font-size='36' fill='{INK}'>{}<tspan font-size='32' fill='{QUIET}'>{}</tspan></text>\n",
                    esc(&item.name),
                    esc(&detail)
                ));
            }
            _ => svg.text(
                x,
                y,
                36.0,
                false,
                INK,
                "start",
                &m.fit(&item.name, 36.0, false, max_w),
            ),
        }
        y += row_h;
    }
    y - row_h
}

/// Converts the SVG to PNG and saves it.
pub fn save_png(svg: &str, out: &Path) -> Result<()> {
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
    pixmap
        .save_png(out)
        .with_context(|| format!("cannot save {}", out.display()))?;
    Ok(())
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
    }

    #[test]
    fn range_labels() {
        let d = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
        assert_eq!(range_label(d(2025, 1, 1), d(2025, 12, 31)), "2025");
        assert_eq!(
            range_label(d(2024, 6, 1), d(2025, 5, 31)),
            "Jun 1, 2024 – May 31, 2025"
        );
    }
}
