pub mod spotify;
pub mod ytmusic;

use crate::model::Play;

/// Result of loading a source: the plays and any non-fatal warnings.
#[derive(Default)]
pub struct Loaded {
    pub plays: Vec<Play>,
    pub warnings: Vec<String>,
}
