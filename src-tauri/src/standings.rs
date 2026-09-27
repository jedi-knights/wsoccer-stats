//! Aggregate a program's own W-L-T record from their schedule games.
//!
//! Each `Game` in the input is one row of one program's schedule page,
//! from that program's perspective — so `result.team_score` is the
//! program's goals, `result.opponent_score` is the opponent's. Standings
//! are grouped by `program_slug`. Games without a result (future games)
//! are ignored.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::data::Game;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Standing {
    pub program_slug: String,
    pub program_name: String,
    pub conference: String,
    pub wins: u32,
    pub losses: u32,
    pub ties: u32,
    pub goals_for: u32,
    pub goals_against: u32,
    pub games_played: u32,
    /// Soccer points: 3 per win, 1 per tie, 0 per loss.
    pub points: u32,
    /// Rank of this team by RPI (1 = best) across every program loaded on
    /// disk, not just the ones in the current filter. Ties share the same
    /// rank (standard competition ranking — 1, 2, 2, 4).
    /// 0 when the team has no played games we can rank.
    #[serde(default)]
    pub rpi_rank: u32,
    /// Raw RPI value in [0, 1] (0 = worst, ~0.75 = elite). Powers the
    /// Y-axis of the SoS-vs-RPI scatter.
    #[serde(default)]
    pub rpi: f64,
    /// Strength of Schedule — the OWP component of RPI: mean opponent
    /// winning percentage with games vs this team excluded. Higher =
    /// tougher schedule. Powers the X-axis of the SoS-vs-RPI scatter.
    #[serde(default)]
    pub sos: f64,
    /// False for zero-record stubs filled from the registry to represent
    /// a conference member whose schedule wasn't ingested (Nuxt SPA,
    /// unknown CMS, etc.). Frontends render such rows with em-dashes to
    /// distinguish "no data ingested" from "played 0 games in this view."
    #[serde(default = "default_true")]
    pub has_schedule_data: bool,
}

fn default_true() -> bool {
    true
}

/// Aggregate `games` into per-program standings, sorted by wins desc,
/// then goal differential desc, then program slug asc.
pub fn compute_standings(games: &[Game]) -> Vec<Standing> {
    let mut by_program: HashMap<String, Standing> = HashMap::new();
    for game in games {
        let Some(result) = &game.result else { continue };
        let (w, l, t) = match result.outcome.as_str() {
            "W" => (1, 0, 0),
            "L" => (0, 1, 0),
            "T" => (0, 0, 1),
            _ => continue, // unknown outcome — bad input, skip rather than misattribute
        };
        let entry = by_program
            .entry(game.program_slug.clone())
            .or_insert_with(|| Standing {
                program_slug: game.program_slug.clone(),
                program_name: game.program_name.clone(),
                conference: game.conference.clone(),
                has_schedule_data: true,
                ..Standing::default()
            });
        entry.wins += w;
        entry.losses += l;
        entry.ties += t;
        entry.goals_for += result.team_score;
        entry.goals_against += result.opponent_score;
        entry.games_played += 1;
        entry.points = entry.wins * 3 + entry.ties;

        // Invariant: every counted game bumped exactly one of W/L/T.
        debug_assert_eq!(entry.wins + entry.losses + entry.ties, entry.games_played);
    }
    let mut standings: Vec<Standing> = by_program.into_values().collect();
    standings.sort_by(|a, b| {
        let gd_a = a.goals_for as i32 - a.goals_against as i32;
        let gd_b = b.goals_for as i32 - b.goals_against as i32;
        b.points
            .cmp(&a.points)
            .then_with(|| gd_b.cmp(&gd_a))
            .then_with(|| a.program_slug.cmp(&b.program_slug))
    });
    standings
}

/// Standard competition ranking (1, 2, 2, 4) over the RPI map — best
/// RPI ranks 1, ties share a rank, next distinct value skips ranks.
/// Slug ascending as the final tie-break for determinism.
///
/// Called by both ``list_standings`` and ``list_conference_summary`` so
/// the RPI column and the Conferences tab's "avg RPI rank" agree
/// exactly. Extracted here so a change to the tie epsilon / tie-break
/// order fixes both sites at once.
pub fn rank_by_slug(rpi_by_slug: &HashMap<String, f64>) -> HashMap<String, u32> {
    // Two f64 values that differ by less than this are treated as tied.
    // 1e-9 leaves 5+ significant digits between distinct RPIs and is
    // well above the ULP of a normal RPI value in [0, 1]; the earlier
    // 1e-12 was below ULP for pairs where the two paths through the
    // formula rounded differently.
    const TIE_EPSILON: f64 = 1e-9;

    let mut pairs: Vec<(&String, &f64)> = rpi_by_slug.iter().collect();
    pairs.sort_by(|a, b| {
        b.1.partial_cmp(a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(b.0))
    });
    let mut ranks = HashMap::new();
    let mut prev_rpi: Option<f64> = None;
    let mut current_rank: u32 = 0;
    for (i, (slug, rpi)) in pairs.into_iter().enumerate() {
        let rank = match prev_rpi {
            Some(p) if (p - *rpi).abs() < TIE_EPSILON => current_rank,
            _ => (i as u32) + 1,
        };
        ranks.insert(slug.clone(), rank);
        current_rank = rank;
        prev_rpi = Some(*rpi);
    }
    ranks
}

/// Extra `(normalized_name, slug)` entries to seed into the RPI
/// opponent lookup. Callers pass this when they know about programs
/// that have no games on disk (typically the program registry) so
/// games against those programs are still counted in OWP / OOWP
/// averages rather than silently skipped.
///
/// Each element is `(program_name, program_slug)` — the caller has
/// NOT normalized the name; ``compute_rpi`` runs its own
/// ``normalize_team_name`` on both sides so the seeding key matches
/// the lookup key exactly.
pub type NameSlugSeed<'a> = &'a [(&'a str, &'a str)];

/// Classical NCAA-style Rating Percentage Index for each program that has
/// at least one played game in `all_games`.
///
/// `RPI = 0.25·WP + 0.50·OWP + 0.25·OOWP` where
/// - `WP(T)` = `(wins + 0.5·ties) / gp`
/// - `OWP(T)` = mean, over each opponent `O` of `T`, of `O`'s winning
///   percentage **with the games between `O` and `T` removed** (so the
///   team's own results don't inflate its opponents' rating)
/// - `OOWP(T)` = mean, over each opponent `O` of `T`, of `O`'s standard OWP
///
/// Opponents whose program schedule we haven't loaded AND aren't in
/// `extra_names` are skipped from the averages rather than assigned a
/// baseline. Pass the full program registry as `extra_names` so games
/// against programs whose schedule failed to ingest (Nuxt SPAs) still
/// count in OWP/OOWP.
///
/// The three RPI components for one program — WP, OWP (= strength of
/// schedule), OOWP — plus the combined RPI. Callers that need SoS
/// specifically read `.owp`. `wp` and `oowp` are exposed for future
/// consumers even if no current caller reads them.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct RpiComponents {
    pub rpi: f64,
    pub wp: f64,
    pub owp: f64,
    pub oowp: f64,
}

/// Like `compute_rpi_seeded` but returns the full breakdown so
/// callers can surface strength-of-schedule (== OWP) alongside RPI.
pub fn compute_rpi_full(
    all_games: &[Game],
    extra_names: NameSlugSeed,
) -> HashMap<String, RpiComponents> {
    #[derive(Default, Clone)]
    struct Record {
        w: u32,
        l: u32,
        t: u32,
    }
    impl Record {
        fn gp(&self) -> u32 {
            self.w + self.l + self.t
        }
        fn wp(&self) -> f64 {
            let gp = self.gp();
            if gp == 0 {
                return 0.0;
            }
            (self.w as f64 + 0.5 * self.t as f64) / gp as f64
        }
    }

    // Per-program record aggregated across every played game.
    let mut records: HashMap<String, Record> = HashMap::new();
    // Per-program list of (opponent_name_lower, outcome_from_our_pov).
    // Used both to identify each program's opponent set and to strip
    // "games against T" when computing OWP(O) from T's perspective.
    let mut opponents: HashMap<String, Vec<(String, char)>> = HashMap::new();
    // program_name (lowercase) → program_slug so we can look up an
    // opponent's own record when we have it.
    let mut name_to_slug: HashMap<String, String> = HashMap::new();

    for g in all_games {
        let Some(res) = &g.result else { continue };
        let outcome = match res.outcome.as_str() {
            "W" => 'W',
            "L" => 'L',
            "T" => 'T',
            _ => continue,
        };
        let rec = records.entry(g.program_slug.clone()).or_default();
        match outcome {
            'W' => rec.w += 1,
            'L' => rec.l += 1,
            'T' => rec.t += 1,
            _ => unreachable!(),
        }
        opponents
            .entry(g.program_slug.clone())
            .or_default()
            .push((crate::normalize_team_name(&g.opponent), outcome));
        if !g.program_name.is_empty() {
            name_to_slug
                .entry(crate::normalize_team_name(&g.program_name))
                .or_insert_with(|| g.program_slug.clone());
        }
    }
    // Seed extra name → slug pairs from the caller (typically the
    // program registry) so opponents whose own schedule failed to
    // ingest still resolve to a slug and contribute to OWP / OOWP.
    // Without this, games against Oklahoma / Notre Dame / Iowa etc.
    // silently drop from every SEC/ACC/Big-Ten opponent's rating.
    for (name, slug) in extra_names {
        if name.is_empty() || slug.is_empty() {
            continue;
        }
        name_to_slug
            .entry(crate::normalize_team_name(name))
            .or_insert_with(|| (*slug).to_string());
    }

    // O's WP with every game against a team named `target_name_lower` removed.
    // Returns None when O ends up with 0 games after the exclusion.
    let wp_excluding = |o_slug: &str, target_name_lower: &str| -> Option<f64> {
        let games = opponents.get(o_slug)?;
        let mut w = 0u32;
        let mut l = 0u32;
        let mut t = 0u32;
        for (opp_name, outcome) in games {
            if opp_name == target_name_lower {
                continue;
            }
            match outcome {
                'W' => w += 1,
                'L' => l += 1,
                'T' => t += 1,
                _ => {}
            }
        }
        let gp = w + l + t;
        if gp == 0 {
            return None;
        }
        Some((w as f64 + 0.5 * t as f64) / gp as f64)
    };

    // Standard OWP(O) — mean of O's opponents' WP, each excluding games vs O.
    // Cached because OOWP(T) queries the same OWP many times.
    let mut owp_cache: HashMap<String, f64> = HashMap::new();
    let mut owp_of = |o_slug: &str, o_name_lower: &str| -> f64 {
        if let Some(&v) = owp_cache.get(o_slug) {
            return v;
        }
        let Some(o_opps) = opponents.get(o_slug) else {
            owp_cache.insert(o_slug.to_string(), 0.0);
            return 0.0;
        };
        let mut sum = 0.0;
        let mut n = 0;
        // Deduplicate opponent identities so a team we played twice doesn't
        // double-count in the average.
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for (opp_name, _) in o_opps {
            if !seen.insert(opp_name.as_str()) {
                continue;
            }
            let Some(opp_slug) = name_to_slug.get(opp_name) else {
                continue;
            };
            if let Some(wp) = wp_excluding(opp_slug, o_name_lower) {
                sum += wp;
                n += 1;
            }
        }
        let v = if n == 0 { 0.0 } else { sum / n as f64 };
        owp_cache.insert(o_slug.to_string(), v);
        v
    };

    // Reverse index so we can pass each program's lowercase name to
    // wp_excluding / owp_of without regenerating it.
    let slug_to_name_lower: HashMap<String, String> = name_to_slug
        .iter()
        .map(|(name, slug)| (slug.clone(), name.clone()))
        .collect();

    let mut out: HashMap<String, RpiComponents> = HashMap::new();
    for (t_slug, t_record) in &records {
        let wp = t_record.wp();
        let t_name_lower = slug_to_name_lower
            .get(t_slug)
            .cloned()
            .unwrap_or_default();

        let Some(t_opps) = opponents.get(t_slug) else {
            out.insert(
                t_slug.clone(),
                RpiComponents { rpi: 0.25 * wp, wp, owp: 0.0, oowp: 0.0 },
            );
            continue;
        };

        // OWP(T): mean of each opponent's WP excluding games vs T.
        let mut owp_sum = 0.0;
        let mut owp_n = 0;
        // OOWP(T): mean of each opponent's standard OWP.
        let mut oowp_sum = 0.0;
        let mut oowp_n = 0;
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for (opp_name, _) in t_opps {
            if !seen.insert(opp_name.as_str()) {
                continue;
            }
            let Some(opp_slug) = name_to_slug.get(opp_name) else {
                continue;
            };
            if let Some(w) = wp_excluding(opp_slug, &t_name_lower) {
                owp_sum += w;
                owp_n += 1;
            }
            let opp_name_lower = slug_to_name_lower
                .get(opp_slug)
                .cloned()
                .unwrap_or_else(|| opp_name.clone());
            oowp_sum += owp_of(opp_slug, &opp_name_lower);
            oowp_n += 1;
        }
        let owp = if owp_n == 0 { 0.0 } else { owp_sum / owp_n as f64 };
        let oowp = if oowp_n == 0 { 0.0 } else { oowp_sum / oowp_n as f64 };

        out.insert(
            t_slug.clone(),
            RpiComponents {
                rpi: 0.25 * wp + 0.50 * owp + 0.25 * oowp,
                wp,
                owp,
                oowp,
            },
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::GameResult;

    fn played(program: &str, outcome: &str, team: u32, opp: u32) -> Game {
        Game {
            program_slug: program.into(),
            program_name: program.into(),
            conference: "acc".into(),
            cms: "sidearm".into(),
            date: "2026-08-12".into(),
            opponent: "someone".into(),
            home_away: "home".into(),
            result: Some(GameResult {
                outcome: outcome.into(),
                team_score: team,
                opponent_score: opp,
            }),
            ..Default::default()
        }
    }

    fn future(program: &str) -> Game {
        Game {
            program_slug: program.into(),
            program_name: program.into(),
            conference: "acc".into(),
            cms: "sidearm".into(),
            date: "2026-10-01".into(),
            opponent: "someone".into(),
            home_away: "home".into(),
            result: None,
            ..Default::default()
        }
    }

    #[test]
    fn empty_input_returns_empty_standings() {
        assert!(compute_standings(&[]).is_empty());
    }

    #[test]
    fn aggregates_wins_losses_ties_and_goals() {
        let games = vec![
            played("duke", "W", 3, 1),
            played("duke", "L", 0, 2),
            played("duke", "T", 1, 1),
            played("duke", "W", 4, 0),
        ];
        let standings = compute_standings(&games);
        assert_eq!(standings.len(), 1);
        let duke = &standings[0];
        assert_eq!(duke.program_slug, "duke");
        assert_eq!(duke.conference, "acc");
        assert_eq!(duke.wins, 2);
        assert_eq!(duke.losses, 1);
        assert_eq!(duke.ties, 1);
        assert_eq!(duke.goals_for, 8);
        assert_eq!(duke.goals_against, 4);
        assert_eq!(duke.games_played, 4);
        // 2 wins * 3 + 1 tie * 1 = 7 points
        assert_eq!(duke.points, 7);
    }

    #[test]
    fn future_games_are_ignored() {
        let games = vec![played("duke", "W", 3, 1), future("duke"), future("duke")];
        let standings = compute_standings(&games);
        assert_eq!(standings[0].games_played, 1);
    }

    #[test]
    fn sorts_by_wins_then_goal_differential_then_slug() {
        let games = vec![
            // duke: 2W, GD=+4
            played("duke", "W", 3, 1),
            played("duke", "W", 2, 0),
            // unc: 2W, GD=+2  (fewer GD than duke → ranks below)
            played("unc", "W", 1, 0),
            played("unc", "W", 1, 0),
            // wake: 1W, GD=+3 (fewer wins → ranks below both)
            played("wake", "W", 4, 1),
            // avery: 2W, GD=+4 (same wins & GD as duke → tiebreak by slug asc)
            played("avery", "W", 3, 1),
            played("avery", "W", 2, 0),
        ];
        let order: Vec<_> = compute_standings(&games)
            .into_iter()
            .map(|s| s.program_slug)
            .collect();
        assert_eq!(order, vec!["avery", "duke", "unc", "wake"]);
    }

    fn game(program: &str, opponent: &str, outcome: &str, team: u32, opp: u32) -> Game {
        Game {
            program_slug: program.into(),
            program_name: program.into(),
            conference: "acc".into(),
            cms: "sidearm".into(),
            date: "2026-08-12".into(),
            opponent: opponent.into(),
            home_away: "home".into(),
            result: Some(GameResult {
                outcome: outcome.into(),
                team_score: team,
                opponent_score: opp,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn rpi_matches_hand_computed_value() {
        // Three teams playing each other exactly once. Every game has a
        // known result, so RPI has a unique closed-form value for each
        // team.
        //
        //   A beat B  (A: 1-0, B: 0-1)
        //   A beat C  (A: 2-0, C: 0-1)
        //   B beat C  (B: 1-1, C: 0-2)
        //
        // WP(A) = 2/2 = 1.0
        // WP(B) = 1/2 = 0.5
        // WP(C) = 0/2 = 0.0
        //
        // OWP(A) = mean of B's WP excluding vs A, C's WP excluding vs A
        //        = mean(1/1, 0/1) = 0.5
        // OWP(B) = mean of A's WP excluding vs B, C's WP excluding vs B
        //        = mean(1/1, 0/1) = 0.5
        // OWP(C) = mean of A's WP excluding vs C, B's WP excluding vs C
        //        = mean(1/1, 0/1) = 0.5
        //
        // OOWP(A) = mean of OWP(B), OWP(C) = 0.5
        // OOWP(B) = mean of OWP(A), OWP(C) = 0.5
        // OOWP(C) = mean of OWP(A), OWP(B) = 0.5
        //
        // RPI(A) = 0.25*1.0 + 0.50*0.5 + 0.25*0.5 = 0.625
        // RPI(B) = 0.25*0.5 + 0.50*0.5 + 0.25*0.5 = 0.500
        // RPI(C) = 0.25*0.0 + 0.50*0.5 + 0.25*0.5 = 0.375
        let games = vec![
            game("a", "b", "W", 1, 0),
            game("a", "c", "W", 2, 0),
            game("b", "a", "L", 0, 1),
            game("b", "c", "W", 1, 0),
            game("c", "a", "L", 0, 2),
            game("c", "b", "L", 0, 1),
        ];
        let rpi = compute_rpi_full(&games, &[]);
        assert!((rpi["a"].rpi - 0.625).abs() < 1e-9, "a RPI: {}", rpi["a"].rpi);
        assert!((rpi["b"].rpi - 0.500).abs() < 1e-9, "b RPI: {}", rpi["b"].rpi);
        assert!((rpi["c"].rpi - 0.375).abs() < 1e-9, "c RPI: {}", rpi["c"].rpi);
    }

    #[test]
    fn rpi_skips_unknown_opponents_from_averages() {
        // A plays two games: beats a program we HAVE loaded (b), then beats
        // a program we do NOT have loaded (unknown_team). Only b should
        // contribute to A's OWP; the missing opponent is skipped rather
        // than assigned a baseline.
        let games = vec![
            game("a", "b", "W", 1, 0),
            game("a", "unknown_team", "W", 3, 0),
            game("b", "a", "L", 0, 1),
        ];
        let rpi = compute_rpi_full(&games, &[]);
        // WP(A) = 2/2 = 1.0
        // OWP(A) uses only b (WP excl A = 0/1 = 0.0) → 0.0
        // OOWP(A) uses only b's OWP: b's opponents = {a}, wp_excl_b(a) = 1/1
        //   → OWP(b) = 1.0 → OOWP(A) = 1.0
        // RPI(A) = 0.25*1.0 + 0.5*0.0 + 0.25*1.0 = 0.5
        assert!((rpi["a"].rpi - 0.5).abs() < 1e-9, "a RPI: {}", rpi["a"].rpi);
    }

    #[test]
    fn unknown_outcome_is_skipped() {
        let games = vec![
            played("duke", "W", 3, 1),
            played("duke", "?", 9, 9), // malformed — should not affect counts
        ];
        let standings = compute_standings(&games);
        assert_eq!(standings[0].games_played, 1);
        assert_eq!(standings[0].wins, 1);
        assert_eq!(standings[0].goals_for, 3);
    }
}
