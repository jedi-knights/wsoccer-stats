mod data;
mod standings;

use std::path::PathBuf;

pub use data::{read_ndjson_dir, Game, GameResult};
pub use standings::{compute_standings, Standing};

/// Directory the app reads schedule NDJSON from.
///
/// Resolution order:
/// 1. `WSOCCER_STATS_DATA_DIR` environment variable (absolute or CWD-relative).
/// 2. Compile-time fallback: sibling repo `athletics-ingest-platform/output/schedules`.
fn data_dir() -> PathBuf {
    if let Ok(v) = std::env::var("WSOCCER_STATS_DATA_DIR") {
        return PathBuf::from(v);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("athletics-ingest-platform")
        .join("output")
        .join("schedules")
}

#[tauri::command]
fn list_standings() -> Result<Vec<Standing>, String> {
    let dir = data_dir();
    let games = data::read_ndjson_dir(&dir).map_err(|e| {
        format!(
            "failed to read schedule data from {}: {e}",
            dir.display()
        )
    })?;
    Ok(standings::compute_standings(&games))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![list_standings])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
