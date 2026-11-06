//! Readers for each service's export, which all produce [`Play`]s.

pub mod spotify;
pub mod ytmusic;

use crate::model::Play;

/// Result of loading a source: the plays and any non-fatal warnings.
#[derive(Default)]
pub struct Loaded {
    /// Plays read from the source.
    pub plays: Vec<Play>,
    /// Problems that didn't stop the load, such as a skipped file.
    pub warnings: Vec<String>,
}
