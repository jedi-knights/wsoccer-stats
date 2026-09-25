mod data;
mod roster;
mod standings;

use std::path::PathBuf;

pub use data::{read_ndjson_dir, Game, GameResult};
pub use roster::{read_roster_file, Player};
pub use standings::{compute_standings, Standing};

/// Base directory the app reads NDJSON from (holds `schedules/` and
/// `rosters/` subdirectories).
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

/// Return one program's roster, sorted by jersey number then by name.
///
/// Jersey numbers are strings (a few players wear "GK" or non-numeric),
/// so numeric-when-possible sort with a lexicographic fallback.
#[tauri::command]
fn list_roster(slug: String) -> Result<Vec<Player>, String> {
    if slug.is_empty() || slug.contains('/') || slug.contains('\\') {
        return Err(format!("invalid program slug: {slug:?}"));
    }
    let path = rosters_dir().join(format!("{slug}.ndjson"));
    if !path.exists() {
        return Err(format!("no roster on disk for program: {slug}"));
    }
    let mut players = roster::read_roster_file(&path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    players.sort_by(|a, b| {
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
    Ok(players)
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
