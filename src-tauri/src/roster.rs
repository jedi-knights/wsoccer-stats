//! Read roster NDJSON produced by `athletics-ingest-platform`.
//!
//! Same NDJSON-per-program shape as schedules (one file per program, one
//! record per line), but the record is a `Player` (biographical fields
//! only — no per-game or season stats).

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    pub program_slug: String,
    #[serde(default)]
    pub program_name: String,
    #[serde(default)]
    pub conference: String,
    pub cms: String,
    pub name: String,
    pub jersey_number: String,
    pub position: String,
    #[serde(default)]
    pub class_year: String,
    #[serde(default)]
    pub height: String,
    #[serde(default)]
    pub hometown: String,
}

/// Read one program's roster NDJSON into a `Vec<Player>`.
pub fn read_roster_file(path: &Path) -> io::Result<Vec<Player>> {
    let content = fs::read_to_string(path)?;
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<Player>(line).map_err(io::Error::other))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_player_record() {
        let line = r#"{"program_slug":"duke","conference":"acc","cms":"sidearm","name":"Molly Vapensky","jersey_number":"1","position":"GK","class_year":"So.","height":"5' 9''","hometown":"Evanston, Ill."}"#;
        let player: Player = serde_json::from_str(line).unwrap();
        assert_eq!(player.program_slug, "duke");
        assert_eq!(player.conference, "acc");
        assert_eq!(player.name, "Molly Vapensky");
        assert_eq!(player.jersey_number, "1");
        assert_eq!(player.position, "GK");
        assert_eq!(player.class_year, "So.");
    }

    #[test]
    fn missing_optional_fields_default_to_empty() {
        // Some sites omit hometown, height, class year. Missing => "".
        let line = r#"{"program_slug":"lsu","conference":"sec","cms":"wmt","name":"Someone","jersey_number":"9","position":"F"}"#;
        let player: Player = serde_json::from_str(line).unwrap();
        assert_eq!(player.hometown, "");
        assert_eq!(player.height, "");
        assert_eq!(player.class_year, "");
    }
}
