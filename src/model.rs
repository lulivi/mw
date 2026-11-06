use chrono::{DateTime, Utc};

/// Service a play comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    Spotify,
    YouTubeMusic,
}

impl Source {
    pub fn label(&self) -> &'static str {
        match self {
            Source::Spotify => "Spotify",
            Source::YouTubeMusic => "YouTube Music",
        }
    }
}

/// A single play, in the same shape whatever its source.
#[derive(Debug, Clone)]
pub struct Play {
    pub ts: DateTime<Utc>,
    pub track: String,
    pub artist: String,
    #[allow(dead_code)]
    pub album: Option<String>,
    pub ms_played: u64,
    /// `true` when the length is an estimate (YouTube Music).
    pub estimated: bool,
    pub source: Source,
    /// YouTube video ID (YouTube Music only), used to fetch its real length.
    pub video_id: Option<String>,
}
