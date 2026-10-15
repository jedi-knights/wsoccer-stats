//! Print RPI rankings top 50 — mirrors the computation `list_standings` uses
//! so we can compare our rankings against the NCAA's published RPI.

use std::collections::HashMap;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base = std::env::var("WSOCCER_STATS_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("athletics-ingest-platform")
                .join("output")
        });

    let games = wsoccer_stats_lib::read_ndjson_dir(&base.join("schedules"))?;

    // Build the registry seed the same way lib.rs does — opponent names
    // we didn't ingest a schedule for still need to resolve to a slug.
    let reg_path = base.join("programs.ndjson");
    let reg_text = std::fs::read_to_string(&reg_path)?;
    let registry: Vec<(String, String, String)> = reg_text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).ok()?;
            Some((
                v.get("slug")?.as_str()?.to_string(),
                v.get("name")?.as_str()?.to_string(),
                v.get("conference").and_then(|c| c.as_str()).unwrap_or("").to_string(),
            ))
        })
        .collect();
    let seed: Vec<(&str, &str)> = registry
        .iter()
        .map(|(slug, name, _)| (name.as_str(), slug.as_str()))
        .collect();

    let rpi = wsoccer_stats_lib::compute_rpi_full(&games, &seed);

    // Program records (W-L-T) for display.
    let mut records: HashMap<String, (u32, u32, u32)> = HashMap::new();
    for g in &games {
        let Some(res) = &g.result else { continue };
        let r = records.entry(g.program_slug.clone()).or_default();
        match res.outcome.as_str() {
            "W" => r.0 += 1,
            "L" => r.1 += 1,
            "T" => r.2 += 1,
            _ => {}
        }
    }

    let name_by_slug: HashMap<String, (String, String)> = registry
        .into_iter()
        .map(|(slug, name, conf)| (slug, (name, conf)))
        .collect();

    let mut rows: Vec<(String, String, f64, (u32, u32, u32))> = rpi
        .iter()
        .map(|(slug, c)| {
            let (name, conf) = name_by_slug
                .get(slug)
                .cloned()
                .unwrap_or_else(|| (slug.clone(), String::new()));
            let _ = conf;
            let rec = records.get(slug).copied().unwrap_or((0, 0, 0));
            (slug.clone(), name, c.rpi, rec)
        })
        .collect();
    // Sort by RPI descending.
    rows.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());

    println!("rank  slug                    name                       W-L-T    RPI");
    println!("{}", "-".repeat(80));
    for (i, (slug, name, rpi, rec)) in rows.iter().take(60).enumerate() {
        println!(
            "{:>4}  {:<22} {:<26} {:>2}-{:>2}-{:>2}  {:.5}",
            i + 1,
            slug,
            name,
            rec.0,
            rec.1,
            rec.2,
            rpi
        );
    }
    Ok(())
}
