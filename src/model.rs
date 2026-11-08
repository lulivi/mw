//! The data model shared by all sources: one [`Play`] per listen.

use chrono::{DateTime, Utc};

/// Service a play comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    Spotify,
    YouTubeMusic,
}

impl Source {
    /// Name of the service, as shown to the user.
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
    /// When it was played, in UTC.
    pub ts: DateTime<Utc>,
    /// Song title as the source reports it.
    pub track: String,
    /// Artist or channel name, without YouTube channel add-ons.
    pub artist: String,
    /// Album name, when the source provides it.
    #[allow(dead_code)]
    pub album: Option<String>,
    /// Listening time in milliseconds.
    pub ms_played: u64,
    /// `true` when the length is an estimate (YouTube Music).
    pub estimated: bool,
    /// Service it comes from.
    pub source: Source,
    /// YouTube video ID (YouTube Music only), used to look up the song database.
    pub video_id: Option<String>,
}
