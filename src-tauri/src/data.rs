//! Read schedule NDJSON produced by `athletics-ingest-platform`.
//!
//! The producer emits one game per line, with `result` populated for
//! played games and `null` for scheduled-but-unplayed ones. This module
//! mirrors that shape as Rust types and offers helpers to read a single
//! file or every `.ndjson` in a directory.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameResult {
    pub outcome: String, // "W", "L", or "T"
    pub team_score: u32,
    pub opponent_score: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Game {
    pub program_slug: String,
    /// Display name for the program (e.g. "North Carolina"). Empty string
    /// when the producer omitted the field.
    #[serde(default)]
    pub program_name: String,
    /// Conference slug (e.g. "acc", "sec"). Empty string when the producer
    /// omitted the field — accept older NDJSON without failing to load.
    #[serde(default)]
    pub conference: String,
    pub cms: String,
    pub date: String, // ISO YYYY-MM-DD
    pub opponent: String,
    pub home_away: String, // "home", "away", or "neutral"
    pub result: Option<GameResult>,
}

/// Parse a single NDJSON line into a `Game`.
pub fn parse_line(line: &str) -> Result<Game, serde_json::Error> {
    serde_json::from_str(line)
}

/// Read one NDJSON file into a `Vec<Game>`. Empty lines are skipped;
/// malformed lines error out (the producer is trusted to emit valid JSON).
pub fn read_ndjson_file(path: &Path) -> io::Result<Vec<Game>> {
    let content = fs::read_to_string(path)?;
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| parse_line(line).map_err(io::Error::other))
        .collect()
}

/// Read every `*.ndjson` file in `dir` and concatenate the games.
/// Sub-directories are ignored. Order across files is filesystem-defined.
pub fn read_ndjson_dir(dir: &Path) -> io::Result<Vec<Game>> {
    let mut games = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("ndjson") {
            continue;
        }
        games.extend(read_ndjson_file(&path)?);
    }
    Ok(games)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_line_completed_game() {
        let line = r#"{"program_slug":"duke","conference":"acc","cms":"sidearm","date":"2026-08-12","opponent":"Southern California","home_away":"away","result":{"outcome":"W","team_score":3,"opponent_score":1}}"#;
        let game = parse_line(line).unwrap();
        assert_eq!(game.program_slug, "duke");
        assert_eq!(game.conference, "acc");
        assert_eq!(game.opponent, "Southern California");
        assert_eq!(game.home_away, "away");
        assert_eq!(
            game.result,
            Some(GameResult {
                outcome: "W".into(),
                team_score: 3,
                opponent_score: 1
            })
        );
    }

    #[test]
    fn parse_line_missing_conference_defaults_to_empty() {
        // Older NDJSON produced before the conference field was added must still load.
        let line = r#"{"program_slug":"duke","cms":"sidearm","date":"2026-08-12","opponent":"USC","home_away":"away","result":null}"#;
        let game = parse_line(line).unwrap();
        assert_eq!(game.conference, "");
    }

    #[test]
    fn parse_line_future_game_has_null_result() {
        let line = r#"{"program_slug":"duke","cms":"sidearm","date":"2026-10-03","opponent":"California","home_away":"home","result":null}"#;
        let game = parse_line(line).unwrap();
        assert_eq!(game.opponent, "California");
        assert_eq!(game.result, None);
    }

    #[test]
    fn parse_line_malformed_errors() {
        assert!(parse_line("not json").is_err());
    }

    #[test]
    fn read_ndjson_file_skips_blank_lines() {
        let tmp = tempdir();
        let path = tmp.join("duke.ndjson");
        let content = format!(
            "{}\n\n{}\n",
            r#"{"program_slug":"duke","cms":"sidearm","date":"2026-08-12","opponent":"USC","home_away":"away","result":null}"#,
            r#"{"program_slug":"duke","cms":"sidearm","date":"2026-08-16","opponent":"UCLA","home_away":"away","result":null}"#
        );
        fs::write(&path, content).unwrap();
        let games = read_ndjson_file(&path).unwrap();
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].opponent, "USC");
        assert_eq!(games[1].opponent, "UCLA");
    }

    #[test]
    fn read_ndjson_dir_concatenates_ndjson_only() {
        let tmp = tempdir();
        fs::write(
            tmp.join("duke.ndjson"),
            r#"{"program_slug":"duke","cms":"sidearm","date":"2026-08-12","opponent":"USC","home_away":"away","result":null}"#,
        )
        .unwrap();
        fs::write(
            tmp.join("clemson.ndjson"),
            r#"{"program_slug":"clemson","cms":"wmt","date":"2026-08-12","opponent":"Auburn","home_away":"away","result":null}"#,
        )
        .unwrap();
        fs::write(tmp.join("readme.txt"), "not ndjson").unwrap();
        let games = read_ndjson_dir(&tmp).unwrap();
        assert_eq!(games.len(), 2);
    }

    /// Cheap unique temp dir under $TMPDIR — avoids pulling in the tempfile crate
    /// for a two-test dependency.
    fn tempdir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("wsoccer-stats-test-{pid}-{n}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
