//! Read per-player stats NDJSON produced by `athletics-ingest-platform`.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerStats {
    pub program_slug: String,
    #[serde(default)]
    pub program_name: String,
    #[serde(default)]
    pub conference: String,
    pub cms: String,
    /// Name as printed on the stats page (often "Last, First"). Kept as a
    /// tiebreaker when jersey numbers collide (they shouldn't per program).
    pub name: String,
    pub jersey_number: String,
    pub games_played: u32,
    pub games_started: u32,
    pub minutes: u32,
    pub goals: u32,
    pub assists: u32,
    /// Total shot attempts. Older NDJSON produced before this field
    /// was added loads as 0 via ``#[serde(default)]``.
    #[serde(default)]
    pub shots: u32,
    /// Shots on goal — subset of `shots` that hit the frame. The
    /// frontend derives missed = shots - shots_on_goal and
    /// SOG% = shots_on_goal / shots for display.
    #[serde(default)]
    pub shots_on_goal: u32,
}

/// Read one program's stats NDJSON into a `Vec<PlayerStats>`.
pub fn read_stats_file(path: &Path) -> io::Result<Vec<PlayerStats>> {
    let content = fs::read_to_string(path)?;
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<PlayerStats>(line).map_err(io::Error::other))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_stats_row() {
        let line = r#"{"program_slug":"duke","program_name":"Duke","conference":"acc","cms":"sidearm","name":"Minestrella, Mia","jersey_number":"13","games_played":9,"games_started":9,"minutes":630,"goals":10,"assists":2}"#;
        let s: PlayerStats = serde_json::from_str(line).unwrap();
        assert_eq!(s.name, "Minestrella, Mia");
        assert_eq!(s.jersey_number, "13");
        assert_eq!(s.games_played, 9);
        assert_eq!(s.goals, 10);
    }
}
