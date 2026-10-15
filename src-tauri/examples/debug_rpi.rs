//! Diagnostic: dump one program's full RPI breakdown with each
//! opponent's AWP contribution.
//!
//! Usage: `cargo run --example debug_rpi -- <slug>`
//! Default slug when none given: `michigan_state`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let target = std::env::args().nth(1).unwrap_or_else(|| "michigan_state".to_string());

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

    let reg_path = base.join("programs.ndjson");
    let reg_text = std::fs::read_to_string(&reg_path)?;
    let registry: Vec<(String, String)> = reg_text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).ok()?;
            Some((
                v.get("slug")?.as_str()?.to_string(),
                v.get("name")?.as_str()?.to_string(),
            ))
        })
        .collect();
    let seed: Vec<(&str, &str)> = registry
        .iter()
        .map(|(slug, name)| (name.as_str(), slug.as_str()))
        .collect();
    // name (as printed on another team's schedule) -> registry slug.
    // Build the same normalized map compute_rpi_full uses so we can
    // resolve opponents the same way the computation does.
    let name_to_slug: HashMap<String, String> = registry
        .iter()
        .map(|(slug, name)| (name.clone(), slug.clone()))
        .collect();

    let rpi = wsoccer_stats_lib::compute_rpi_full(&games, &seed);

    let target_comp = rpi.get(&target).ok_or_else(|| format!("no RPI for {target}"))?;

    println!("=== {} ===", target);
    println!("  AWP  = {:.5}", target_comp.wp);
    println!("  OWP  = {:.5}  (strength of schedule)", target_comp.owp);
    println!("  OOWP = {:.5}", target_comp.oowp);
    println!("  RPI  = 0.25*AWP + 0.50*OWP + 0.25*OOWP = {:.5}", target_comp.rpi);

    // Compute rank among all teams for context.
    let mut by_rpi: Vec<(&String, &wsoccer_stats_lib::RpiComponents)> = rpi.iter().collect();
    by_rpi.sort_by(|a, b| b.1.rpi.partial_cmp(&a.1.rpi).unwrap());
    let rank = by_rpi.iter().position(|(s, _)| **s == target).unwrap_or(0) + 1;
    println!("  rank among {} rated teams: {}", by_rpi.len(), rank);
    println!();

    // Walk opponents of target. We re-read games to extract the
    // ordered opponent list from target's own schedule rows.
    let target_opponents: Vec<&wsoccer_stats_lib::Game> = games
        .iter()
        .filter(|g| g.program_slug == target && g.result.is_some())
        .collect();

    println!("Opponents (from {}'s own schedule, {} played games):", target, target_opponents.len());
    println!("  {:<28} {:<10} {:<8} AWP     context", "opponent", "slug", "outcome");
    println!("  {}", "-".repeat(78));
    let mut seen: HashSet<String> = HashSet::new();
    let mut awp_contributions: Vec<(String, f64)> = Vec::new();
    for g in &target_opponents {
        let opp_norm = normalize(&g.opponent);
        if !seen.insert(opp_norm.clone()) {
            continue;
        }
        let outcome = g.result.as_ref().map(|r| r.outcome.as_str()).unwrap_or("?");
        let location = g.home_away.as_str();

        // Resolve opponent to a slug — same way compute_rpi_full does.
        let opp_slug_opt = name_to_slug.iter().find_map(|(n, s)| {
            if normalize(n) == opp_norm { Some(s.clone()) } else { None }
        });

        let (slug_label, awp_val): (String, Option<f64>) = match opp_slug_opt {
            Some(s) => {
                let awp = rpi.get(&s).map(|c| c.wp);
                (s, awp)
            }
            None => ("(non-DI)".into(), None),
        };
        let awp_str = awp_val.map(|v| format!("{:.4}", v)).unwrap_or_else(|| "—".into());
        let note = if awp_val.is_none() { "excluded from OWP" } else { "" };
        println!(
            "  {:<28} {:<10} {} {:<4}  {:<8} {}",
            truncate(&g.opponent, 28),
            truncate(&slug_label, 10),
            location.chars().next().unwrap_or(' '),
            outcome,
            awp_str,
            note
        );
        if let Some(v) = awp_val {
            awp_contributions.push((slug_label, v));
        }
    }

    if !awp_contributions.is_empty() {
        let mean: f64 = awp_contributions.iter().map(|(_, v)| v).sum::<f64>()
            / awp_contributions.len() as f64;
        println!();
        println!("  Mean opponent AWP (unexcluded) = {:.5}", mean);
        println!("  Note: compute_rpi_full subtracts target's games from each opponent's record");
        println!("        before computing their AWP, so the per-opponent AWPs above (full-season)");
        println!("        differ slightly from what feeds OWP(target).");
    }

    Ok(())
}

fn normalize(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .replace("#", "")
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { s.chars().take(n - 1).collect::<String>() + "…" }
}
