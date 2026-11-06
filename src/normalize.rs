//! Normalized keys used to merge artists and songs across sources.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// Words that, inside parentheses or brackets, mark an add-on that is not part
/// of the real title ("(Official Video)", "[Audio]", "(Letra)"). Spanish variants
/// are included because titles come in the uploader's language.
const NOISE_WORDS: &[&str] = &[
    "official",
    "oficial",
    "video",
    "vídeo",
    "videoclip",
    "lyric",
    "lyrics",
    "letra",
    "audio",
    "visualizer",
    "visualiser",
    "hd",
    "4k",
    "mv",
    "m/v",
    "clip",
];

/// Strips YouTube channel add-ons: " - Topic" (auto-generated channels),
/// "VEVO" ("RepionVEVO") and " Official"/" Oficial" ("Melendi Oficial").
pub fn clean_artist(raw: &str) -> String {
    let mut t = raw.trim();
    t = t.strip_suffix(" - Topic").unwrap_or(t);
    if t.len() > 4 && t.ends_with("VEVO") {
        t = t[..t.len() - 4].trim_end();
    }
    for suffix in [" official", " oficial", " - official", " - oficial"] {
        if t.len() > suffix.len() && t.to_lowercase().ends_with(suffix) {
            t = t[..t.len() - suffix.len()].trim_end();
            break;
        }
    }
    collapse_spaces(t)
}

/// Removes a leading "Artist - " from the title when it matches the artist
/// ("Repion - Brillante" -> "Brillante"), common in videos uploaded by the channel.
pub fn strip_artist_prefix(title: &str, artist: &str) -> String {
    if let Some((head, rest)) = title.split_once(" - ")
        && !rest.trim().is_empty()
        && artist_key(head) == artist_key(artist)
    {
        return rest.trim().to_string();
    }
    title.to_string()
}

/// Removes noise in parentheses or brackets from a title.
/// "(Remix)" and "(feat. X)" are kept.
pub fn clean_title(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let close = match c {
            '(' => Some(')'),
            '[' => Some(']'),
            _ => None,
        };
        if let Some(close) = close
            && let Some(rel) = chars[i + 1..].iter().position(|&x| x == close)
        {
            let inner: String = chars[i + 1..i + 1 + rel].iter().collect();
            if is_noise(&inner) {
                i += rel + 2;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    let cleaned = collapse_spaces(&out);
    if cleaned.is_empty() {
        collapse_spaces(raw)
    } else {
        cleaned
    }
}

fn is_noise(inner: &str) -> bool {
    let lower = inner.to_lowercase();
    lower
        .split(|c: char| !c.is_alphanumeric() && c != '/')
        .any(|w| NOISE_WORDS.contains(&w))
}

fn collapse_spaces(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Comparison key: no accents, lowercase, normalized spaces.
pub fn key(s: &str) -> String {
    let no_marks: String = s.nfd().filter(|c| !is_combining_mark(*c)).collect();
    collapse_spaces(&no_marks.to_lowercase())
}

/// Artist key: also drops spaces and punctuation, so "TaylorSwift"
/// (from "TaylorSwiftVEVO") and "Taylor Swift" match.
pub fn artist_key(artist: &str) -> String {
    key(&clean_artist(artist))
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

pub fn track_key(artist: &str, track: &str) -> String {
    format!("{}\u{1f}{}", artist_key(artist), key(&clean_title(track)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_topic() {
        assert_eq!(clean_artist("Bad Bunny - Topic"), "Bad Bunny");
        assert_eq!(clean_artist("  Rosalía  "), "Rosalía");
    }

    #[test]
    fn strips_vevo_and_official() {
        assert_eq!(clean_artist("RepionVEVO"), "Repion");
        assert_eq!(clean_artist("Melendi Oficial"), "Melendi");
        assert_eq!(clean_artist("VEVO"), "VEVO");
        assert_eq!(artist_key("RepionVEVO"), artist_key("Repion - Topic"));
        assert_eq!(artist_key("REPION"), artist_key("Repion"));
        assert_eq!(artist_key("TaylorSwiftVEVO"), artist_key("Taylor Swift"));
    }

    #[test]
    fn strips_artist_from_title() {
        assert_eq!(
            strip_artist_prefix("Repion - Brillante", "RepionVEVO"),
            "Brillante"
        );
        assert_eq!(
            strip_artist_prefix("Repion - En todo momento", "REPION"),
            "En todo momento"
        );
        assert_eq!(
            strip_artist_prefix("Tu/Mi Colonia", "Repion"),
            "Tu/Mi Colonia"
        );
        assert_eq!(
            strip_artist_prefix("Otro - Canción", "Repion"),
            "Otro - Canción"
        );
    }

    #[test]
    fn same_key_ignoring_accents_and_case() {
        assert_eq!(artist_key("ROSALÍA - Topic"), artist_key("Rosalía"));
    }

    #[test]
    fn cleans_titles() {
        assert_eq!(clean_title("Despechá (Official Video)"), "Despechá");
        assert_eq!(clean_title("Song [Audio] (Lyrics)"), "Song");
        assert_eq!(clean_title("Song (Remix)"), "Song (Remix)");
        assert_eq!(clean_title("Song (feat. Someone)"), "Song (feat. Someone)");
    }

    #[test]
    fn song_key_merges_sources() {
        assert_eq!(
            track_key("Rosalía", "DESPECHÁ"),
            track_key("ROSALÍA - Topic", "Despechá (Video Oficial)")
        );
    }
}
