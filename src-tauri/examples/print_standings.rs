//! Print standings to stdout — useful for verifying the read/aggregate loop
//! without launching the Tauri app. Reads from WSOCCER_STATS_DATA_DIR or the
//! sibling-repo fallback (same resolution as the running app).

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::var("WSOCCER_STATS_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("athletics-ingest-platform")
                .join("output")
                .join("schedules")
        });
    println!("Reading from: {}", dir.display());
    let games = wsoccer_stats_lib::read_ndjson_dir(&dir)?;
    let standings = wsoccer_stats_lib::compute_standings(&games);
    println!("Loaded {} games across {} programs", games.len(), standings.len());
    println!();
    println!("  Program              GP   W   L   T    GF-GA   GD");
    println!("  {}", "-".repeat(54));
    for s in &standings {
        let gd = s.goals_for as i32 - s.goals_against as i32;
        println!(
            "  {:20} {:>3} {:>3} {:>3} {:>3}   {:>3}-{:<3}  {:>+3}",
            s.program_slug,
            s.games_played,
            s.wins,
            s.losses,
            s.ties,
            s.goals_for,
            s.goals_against,
            gd
        );
    }
    Ok(())
}
