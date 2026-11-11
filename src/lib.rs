//! Music Wrapped (`mw`): a listening recap from Spotify and YouTube Music data
//! exports.
//!
//! The library is everything but the command line: reading the exports,
//! computing the stats and drawing the image. The `mw` program and the web page
//! both use it.

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
