mod data;
mod roster;
mod standings;
mod stats;

use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;

pub use data::{read_ndjson_dir, Game, GameResult};
pub use roster::{read_roster_file, Player};
pub use standings::{compute_standings, Standing};
pub use stats::{read_stats_file, PlayerStats};

/// Base directory the app reads NDJSON from (holds `schedules/`, `rosters/`,
/// and `stats/` subdirectories).
///
/// Resolution order:
/// 1. `WSOCCER_STATS_DATA_DIR` environment variable (absolute or CWD-relative).
/// 2. Compile-time fallback: sibling repo `athletics-ingest-platform/output`.
fn data_dir() -> PathBuf {
    if let Ok(v) = std::env::var("WSOCCER_STATS_DATA_DIR") {
        return PathBuf::from(v);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("athletics-ingest-platform")
        .join("output")
}

fn schedules_dir() -> PathBuf {
    data_dir().join("schedules")
}

fn rosters_dir() -> PathBuf {
    data_dir().join("rosters")
}

fn stats_dir() -> PathBuf {
    data_dir().join("stats")
}

fn load_games() -> Result<Vec<Game>, String> {
    let dir = schedules_dir();
    data::read_ndjson_dir(&dir)
        .map_err(|e| format!("failed to read schedule data from {}: {e}", dir.display()))
}

#[tauri::command]
fn list_standings(conference: Option<String>) -> Result<Vec<Standing>, String> {
    let mut games = load_games()?;
    if let Some(c) = conference.as_deref().filter(|c| !c.is_empty()) {
        games.retain(|g| g.conference == c);
    }
    Ok(standings::compute_standings(&games))
}

/// Distinct conference slugs found in the loaded schedule data, sorted.
/// Powers the frontend's conference filter dropdown.
#[tauri::command]
fn list_conferences() -> Result<Vec<String>, String> {
    let games = load_games()?;
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for g in &games {
        if !g.conference.is_empty() {
            seen.insert(g.conference.clone());
        }
    }
    Ok(seen.into_iter().collect())
}

/// One row in the roster view — biographical Player fields plus optional
/// per-player season stats when a stats file is available for the program.
#[derive(Debug, Clone, Serialize)]
pub struct RosterEntry {
    pub program_slug: String,
    pub program_name: String,
    pub conference: String,
    pub cms: String,
    pub name: String,
    pub jersey_number: String,
    pub position: String,
    pub class_year: String,
    pub height: String,
    pub hometown: String,
    /// Populated when a stats row matches this player by jersey number.
    /// Absent when the program has no stats file or the player didn't
    /// appear in the stats table.
    pub games_played: Option<u32>,
    pub games_started: Option<u32>,
    pub minutes: Option<u32>,
    pub goals: Option<u32>,
    pub assists: Option<u32>,
}

fn validate_slug(slug: &str) -> Result<(), String> {
    if slug.is_empty() || slug.contains('/') || slug.contains('\\') {
        return Err(format!("invalid program slug: {slug:?}"));
    }
    Ok(())
}

/// Return one program's roster enriched with per-player stats where available.
/// Sorted by jersey number (numeric where possible, lexicographic fallback).
#[tauri::command]
fn list_roster(slug: String) -> Result<Vec<RosterEntry>, String> {
    validate_slug(&slug)?;

    let roster_path = rosters_dir().join(format!("{slug}.ndjson"));
    if !roster_path.exists() {
        return Err(format!("no roster on disk for program: {slug}"));
    }
    let players = roster::read_roster_file(&roster_path)
        .map_err(|e| format!("failed to read {}: {e}", roster_path.display()))?;

    // Stats are optional — a Sidearm-only feature today; WMT programs won't
    // have a file. Load if present, ignore if not.
    let stats_path = stats_dir().join(format!("{slug}.ndjson"));
    let stats_by_jersey: HashMap<String, PlayerStats> = if stats_path.exists() {
        let rows = stats::read_stats_file(&stats_path)
            .map_err(|e| format!("failed to read {}: {e}", stats_path.display()))?;
        rows.into_iter().map(|s| (s.jersey_number.clone(), s)).collect()
    } else {
        HashMap::new()
    };

    let mut entries: Vec<RosterEntry> = players
        .into_iter()
        .map(|p| {
            let s = stats_by_jersey.get(&p.jersey_number);
            RosterEntry {
                program_slug: p.program_slug,
                program_name: p.program_name,
                conference: p.conference,
                cms: p.cms,
                name: p.name,
                jersey_number: p.jersey_number,
                position: p.position,
                class_year: p.class_year,
                height: p.height,
                hometown: p.hometown,
                games_played: s.map(|s| s.games_played),
                games_started: s.map(|s| s.games_started),
                minutes: s.map(|s| s.minutes),
                goals: s.map(|s| s.goals),
                assists: s.map(|s| s.assists),
            }
        })
        .collect();

    entries.sort_by(|a, b| {
        let na = a.jersey_number.parse::<u32>().ok();
        let nb = b.jersey_number.parse::<u32>().ok();
        match (na, nb) {
            (Some(x), Some(y)) => x.cmp(&y).then_with(|| a.name.cmp(&b.name)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a
                .jersey_number
                .cmp(&b.jersey_number)
                .then_with(|| a.name.cmp(&b.name)),
        }
    });
    Ok(entries)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            list_standings,
            list_conferences,
            list_roster
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
