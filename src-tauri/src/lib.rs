mod data;
mod roster;
mod standings;
mod stats;

use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;

pub use data::{read_ndjson_dir, read_ndjson_file, Game, GameResult};
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
fn list_standings(
    conference: Option<String>,
    mode: Option<String>,
) -> Result<Vec<Standing>, String> {
    let all_games = load_games()?;

    // Build a program_name → conference lookup from every schedule we have
    // on disk. Used to classify each opponent as conference or non-conference
    // relative to the schedule owner.
    let mut name_to_conf: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for g in &all_games {
        if !g.program_name.is_empty() && !g.conference.is_empty() {
            name_to_conf
                .entry(g.program_name.to_lowercase())
                .or_insert_with(|| g.conference.clone());
        }
    }
    let is_conference_game = |g: &Game| -> bool {
        name_to_conf
            .get(&g.opponent.to_lowercase())
            .is_some_and(|opp_conf| opp_conf == &g.conference)
    };

    let mut games = all_games;
    if let Some(c) = conference.as_deref().filter(|c| !c.is_empty()) {
        games.retain(|g| g.conference == c);
    }
    match mode.as_deref().unwrap_or("all") {
        "conference" => games.retain(is_conference_game),
        "non_conference" => games.retain(|g| !is_conference_game(g)),
        _ => {}
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
    pub roster_url: String,
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

    // Stats are optional. Roster and stats parsers occasionally disagree on
    // the ``#`` prefix on jersey numbers (some WMT variants keep it,
    // sidearm strips it, the WMT API returns the plain digit) — normalise
    // both sides before joining.
    fn norm(j: &str) -> String {
        j.trim_start_matches('#').trim().to_string()
    }
    let stats_path = stats_dir().join(format!("{slug}.ndjson"));
    let stats_by_jersey: HashMap<String, PlayerStats> = if stats_path.exists() {
        let rows = stats::read_stats_file(&stats_path)
            .map_err(|e| format!("failed to read {}: {e}", stats_path.display()))?;
        rows.into_iter().map(|s| (norm(&s.jersey_number), s)).collect()
    } else {
        HashMap::new()
    };

    let mut entries: Vec<RosterEntry> = players
        .into_iter()
        .map(|p| {
            let s = stats_by_jersey.get(&norm(&p.jersey_number));
            RosterEntry {
                program_slug: p.program_slug,
                program_name: p.program_name,
                conference: p.conference,
                roster_url: p.roster_url,
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

/// One row in the leaders table — a player plus their team + the ranked stat.
#[derive(Debug, Clone, Serialize)]
pub struct LeaderRow {
    pub program_slug: String,
    pub program_name: String,
    pub conference: String,
    pub name: String,
    pub jersey_number: String,
    pub games_played: u32,
    pub games_started: u32,
    pub minutes: u32,
    pub goals: u32,
    pub assists: u32,
}

/// Return the top-N players across the loaded stats data, optionally
/// restricted to a single conference. `category` picks the ranking key —
/// "goals", "assists", "points" (G+A), "minutes", "games_played",
/// "games_started". Ties broken by name asc.
#[tauri::command]
fn list_leaders(
    category: String,
    conference: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<LeaderRow>, String> {
    let dir = stats_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut rows: Vec<PlayerStats> = Vec::new();
    for entry in std::fs::read_dir(&dir)
        .map_err(|e| format!("failed to read stats dir {}: {e}", dir.display()))?
    {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("ndjson") {
            continue;
        }
        let file_rows = stats::read_stats_file(&path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        rows.extend(file_rows);
    }
    if let Some(c) = conference.as_deref().filter(|c| !c.is_empty()) {
        rows.retain(|r| r.conference == c);
    }

    let sort_key: Box<dyn Fn(&PlayerStats) -> u32> = match category.as_str() {
        "goals" => Box::new(|r: &PlayerStats| r.goals),
        "assists" => Box::new(|r: &PlayerStats| r.assists),
        "points" => Box::new(|r: &PlayerStats| r.goals * 2 + r.assists),
        "minutes" => Box::new(|r: &PlayerStats| r.minutes),
        "games_played" => Box::new(|r: &PlayerStats| r.games_played),
        "games_started" => Box::new(|r: &PlayerStats| r.games_started),
        other => return Err(format!("unknown leaders category: {other:?}")),
    };

    let mut ranked: Vec<PlayerStats> = rows.into_iter().filter(|r| sort_key(r) > 0).collect();
    ranked.sort_by(|a, b| sort_key(b).cmp(&sort_key(a)).then_with(|| a.name.cmp(&b.name)));

    let cap = limit.unwrap_or(25);
    ranked.truncate(cap);

    Ok(ranked
        .into_iter()
        .map(|r| LeaderRow {
            program_slug: r.program_slug,
            program_name: r.program_name,
            conference: r.conference,
            name: r.name,
            jersey_number: r.jersey_number,
            games_played: r.games_played,
            games_started: r.games_started,
            minutes: r.minutes,
            goals: r.goals,
            assists: r.assists,
        })
        .collect())
}

/// Return the ordered list of games for one program's season schedule.
///
/// Sorted by ISO date ascending — the producer typically writes them that
/// way already, but re-sort explicitly so the caller doesn't depend on
/// filesystem line order.
#[tauri::command]
fn list_schedule(slug: String) -> Result<Vec<Game>, String> {
    validate_slug(&slug)?;
    let path = schedules_dir().join(format!("{slug}.ndjson"));
    if !path.exists() {
        return Err(format!("no schedule on disk for program: {slug}"));
    }
    let mut games = data::read_ndjson_file(&path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    games.sort_by(|a, b| a.date.cmp(&b.date));
    Ok(games)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            list_standings,
            list_conferences,
            list_roster,
            list_schedule,
            list_leaders
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
