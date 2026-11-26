//! Music Wrapped (`mw`): a listening recap from Spotify and YouTube Music data
//! exports.
//!
//! The library is everything but the command line: reading the exports,
//! computing the stats and drawing the image. The `mw` program and the web page
//! both use it.

/// The URL of `$path` in this repository on `main`, where `mw` and the web page
/// download the song database and the fallback fonts from. A macro, so the
/// URLs stay `&'static str` constants (`concat!` takes only literals).
macro_rules! repo_url {
    ($path:literal) => {
        concat!("https://raw.githubusercontent.com/lulivi/mw/main/", $path)
    };
}

pub mod lengths;
pub mod model;
#[cfg(feature = "cli")]
pub mod musicbrainz;
pub mod normalize;
pub mod render;
pub mod songdb;
pub mod sources;
pub mod stats;
pub mod text;
