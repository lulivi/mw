//! Readers for each service's export, which all produce [`Play`]s.
//!
//! They read [`File`]s already in memory, so the same code serves the command
//! line (files read from disk with [`read_path`]) and the web page (files the
//! user added).

pub mod spotify;
pub mod ytmusic;

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::model::Play;

/// Result of loading a source: the plays and any non-fatal warnings.
#[derive(Default)]
pub struct Loaded {
    /// Plays read from the source.
    pub plays: Vec<Play>,
    /// Problems that didn't stop the load, such as a skipped file.
    pub warnings: Vec<String>,
}

/// A file of an export, in memory.
pub struct File {
    /// Its path, as shown in messages; the last component is the file name.
    pub name: String,
    /// Its contents.
    pub bytes: Vec<u8>,
}

impl File {
    /// The file name, without the folders.
    pub fn file_name(&self) -> &str {
        self.name.rsplit(['/', '\\']).next().unwrap_or(&self.name)
    }

    /// The contents as text; a file that isn't UTF-8 fails with its name.
    pub fn text(&self) -> Result<&str> {
        std::str::from_utf8(&self.bytes).with_context(|| format!("cannot read {}", self.name))
    }
}

/// Reads a file, or every `.json` file in a folder and its subfolders (sorted
/// by path, since Spotify's ZIP contains a folder).
pub fn read_path(path: &Path) -> Result<Vec<File>> {
    let read = |p: &Path| -> Result<File> {
        Ok(File {
            name: p.display().to_string(),
            bytes: fs::read(p).with_context(|| format!("cannot read {}", p.display()))?,
        })
    };
    if !path.is_dir() {
        return Ok(vec![read(path)?]);
    }
    let mut paths = vec![];
    let mut dirs = vec![path.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir).with_context(|| format!("cannot read {}", dir.display()))? {
            let p = entry?.path();
            if p.is_dir() {
                dirs.push(p);
            } else if p
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("json"))
            {
                paths.push(p);
            }
        }
    }
    paths.sort();
    paths.iter().map(|p| read(p)).collect()
}
