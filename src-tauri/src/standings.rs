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

/// NCAA Division I women's soccer Rating Percentage Index for each
/// program that has at least one played DI game in `all_games`.
///
/// Formula (matches the NCAA's published methodology since 2013):
///
/// `RPI = 0.25·AWP + 0.50·OWP + 0.25·OOWP` where
/// - `AWP(T)` = **adjusted** winning percentage, with each game weighted
///   by location: road wins and home losses count more; home wins and
///   road losses count less. See `location_weights` below.
/// - `OWP(T)` = mean, over each DI opponent `O` of `T`, of `O`'s AWP
///   **with the games between `O` and `T` removed** (so the team's own
///   results don't inflate its opponents' rating)
/// - `OOWP(T)` = mean, over each DI opponent `O` of `T`, of `O`'s AWP
///   from `O`'s own opponents' perspective
///
/// **DI-only scope.** Games against opponents not in `extra_names` are
/// excluded entirely — not just from OWP/OOWP but from the team's own
/// AWP record. Pass the full program registry as `extra_names` so every
/// DI opponent resolves, and non-DI games (scrimmages, exhibitions,
/// cross-divisional contests) are dropped. This matches NCAA scope:
/// RPI counts only DI-vs-DI results.
///
/// The three RPI components for one program — AWP, OWP (= strength of
/// schedule), OOWP — plus the combined RPI. Callers that need SoS
/// specifically read `.owp`. `wp` holds the adjusted winning percentage.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct RpiComponents {
    pub rpi: f64,
    pub wp: f64,
    pub owp: f64,
    pub oowp: f64,
}

/// Location weights applied to each game before summing into AWP. Returns
/// `(win_weight, loss_weight)` for a game played at the given location.
///
/// A win contributes `+win_weight` to adjusted wins; a loss contributes
/// `+loss_weight` to adjusted losses; a tie contributes `0.5·win_weight`
/// to adjusted wins and `0.5·loss_weight` to adjusted losses (half-win
/// plus half-loss at the game's location).
///
/// Road play is rewarded: an away win is worth more than a home win, and
/// an away loss is punished less than a home loss. Neutral-site games
/// (conference tournaments, NCAA tournament) carry no adjustment. A
/// missing or unrecognized `home_away` falls back to neutral so a game
/// without clean location metadata contributes a baseline value rather
/// than being dropped — the schedule parser emits "home", "away", or
/// "neutral", so this fallback only fires on malformed input.
fn location_weights(home_away: &str) -> (f64, f64) {
    match home_away {
        "home" => (0.6, 1.4),
        "away" => (1.4, 0.6),
        _ => (1.0, 1.0),
    }
}

/// Compute per-program RPI components from the full set of games and a
/// registry seed. See the `RpiComponents` doc comment for the formula.
pub fn compute_rpi_full(
    all_games: &[Game],
    extra_names: NameSlugSeed,
) -> HashMap<String, RpiComponents> {
    // Build the DI name→slug map from the registry seed. Opponents whose
    // normalized name doesn't appear here are treated as non-DI and
    // excluded entirely from the RPI computation.
    //
    // Also register common suffix aliases (`<Name> University`,
    // `<Name> College`): schedules routinely spell "Princeton
    // University", "Dartmouth College" where the registry stores just
    // "Princeton" / "Dartmouth". First-wins semantics keep direct
    // matches like "Boston University" (a distinct program with the
    // suffix in its canonical name) ahead of any alias that would
    // otherwise collide.
    const ALIAS_SUFFIXES: &[&str] = &[" University", " College"];
    let mut name_to_slug: HashMap<String, String> = HashMap::new();
    // First pass: direct names. These must win over aliases.
    for (name, slug) in extra_names {
        if name.is_empty() || slug.is_empty() {
            continue;
        }
        let norm = crate::normalize_team_name(name);
        name_to_slug
            .entry(norm)
            .or_insert_with(|| (*slug).to_string());
    }
    // Second pass: suffix aliases. Only insert when the canonical name
    // doesn't already carry the suffix (so `Boston College` doesn't
    // self-alias) and when the alias slot is unclaimed by a direct
    // entry.
    for (name, slug) in extra_names {
        if name.is_empty() || slug.is_empty() {
            continue;
        }
        let name_lower = name.to_lowercase();
        for suf in ALIAS_SUFFIXES {
            let suf_lower = suf.to_lowercase();
            if name_lower.ends_with(suf_lower.as_str()) {
                continue;
            }
            let alias = crate::normalize_team_name(&format!("{name}{suf}"));
            name_to_slug
                .entry(alias)
                .or_insert_with(|| (*slug).to_string());
        }
    }

    #[derive(Default, Clone)]
    struct Record {
        adj_w: f64,
        adj_l: f64,
    }
    impl Record {
        fn awp(&self) -> f64 {
            let denom = self.adj_w + self.adj_l;
            if denom == 0.0 { 0.0 } else { self.adj_w / denom }
        }
    }

    // Per-program adjusted record across DI-vs-DI played games.
    let mut records: HashMap<String, Record> = HashMap::new();
    // Per-program (opponent_normalized_name, outcome, home_away) — needed
    // to re-weight the opponent's record from a different perspective
    // inside `awp_excluding`.
    let mut opponents: HashMap<String, Vec<(String, char, String)>> = HashMap::new();

    for g in all_games {
        let Some(res) = &g.result else { continue };
        let outcome = match res.outcome.as_str() {
            "W" => 'W',
            "L" => 'L',
            "T" => 'T',
            _ => continue,
        };
        // DI filter: skip the game entirely if the opponent isn't in the
        // registry. Without this, non-DI results would inflate team
        // records and distort every downstream OWP/OOWP average.
        let opp_norm = crate::normalize_team_name(&g.opponent);
        if !name_to_slug.contains_key(&opp_norm) {
            continue;
        }
        let (win_w, loss_w) = location_weights(&g.home_away);
        let rec = records.entry(g.program_slug.clone()).or_default();
        match outcome {
            'W' => rec.adj_w += win_w,
            'L' => rec.adj_l += loss_w,
            'T' => {
                rec.adj_w += 0.5 * win_w;
                rec.adj_l += 0.5 * loss_w;
            }
            _ => unreachable!(),
        }
        opponents
            .entry(g.program_slug.clone())
            .or_default()
            .push((opp_norm, outcome, g.home_away.clone()));
    }

    // O's AWP with every game against the program identified by
    // `target_slug` removed. Returns None when O ends up with 0
    // counted games after the exclusion.
    //
    // Exclusion is slug-based (not name-based) so an alias like
    // "Princeton University" → `princeton` and a direct "Princeton"
    // → `princeton` both filter correctly. The earlier name-based
    // version missed exclusions when a schedule spelled the target's
    // name with a suffix (e.g. "Dartmouth College") that the registry
    // stored without ("Dartmouth").
    let awp_excluding = |o_slug: &str, target_slug: &str| -> Option<f64> {
        let games = opponents.get(o_slug)?;
        let mut adj_w = 0.0f64;
        let mut adj_l = 0.0f64;
        for (opp_name, outcome, ha) in games {
            if name_to_slug.get(opp_name).map(String::as_str) == Some(target_slug) {
                continue;
            }
            let (win_w, loss_w) = location_weights(ha);
            match outcome {
                'W' => adj_w += win_w,
                'L' => adj_l += loss_w,
                'T' => {
                    adj_w += 0.5 * win_w;
                    adj_l += 0.5 * loss_w;
                }
                _ => {}
            }
        }
        let denom = adj_w + adj_l;
        if denom == 0.0 {
            return None;
        }
        Some(adj_w / denom)
    };

    // Standard OWP(O) — mean of O's opponents' AWP, each excluding games
    // vs O. Cached because OOWP(T) queries the same OWP many times.
    let mut owp_cache: HashMap<String, f64> = HashMap::new();
    let mut owp_of = |o_slug: &str| -> f64 {
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
        for (opp_name, _, _) in o_opps {
            if !seen.insert(opp_name.as_str()) {
                continue;
            }
            let Some(opp_slug) = name_to_slug.get(opp_name) else {
                continue;
            };
            if let Some(wp) = awp_excluding(opp_slug, o_slug) {
                sum += wp;
                n += 1;
            }
        }
        let v = if n == 0 { 0.0 } else { sum / n as f64 };
        owp_cache.insert(o_slug.to_string(), v);
        v
    };

    let mut out: HashMap<String, RpiComponents> = HashMap::new();
    for (t_slug, t_record) in &records {
        let wp = t_record.awp();

        let Some(t_opps) = opponents.get(t_slug) else {
            out.insert(
                t_slug.clone(),
                RpiComponents { rpi: 0.25 * wp, wp, owp: 0.0, oowp: 0.0 },
            );
            continue;
        };

        let mut owp_sum = 0.0;
        let mut owp_n = 0;
        let mut oowp_sum = 0.0;
        let mut oowp_n = 0;
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for (opp_name, _, _) in t_opps {
            if !seen.insert(opp_name.as_str()) {
                continue;
            }
            let Some(opp_slug) = name_to_slug.get(opp_name) else {
                continue;
            };
            if let Some(w) = awp_excluding(opp_slug, t_slug) {
                owp_sum += w;
                owp_n += 1;
            }
            oowp_sum += owp_of(opp_slug);
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
        game_at(program, opponent, outcome, team, opp, "neutral")
    }

    fn game_at(
        program: &str,
        opponent: &str,
        outcome: &str,
        team: u32,
        opp: u32,
        home_away: &str,
    ) -> Game {
        Game {
            program_slug: program.into(),
            program_name: program.into(),
            conference: "acc".into(),
            cms: "sidearm".into(),
            date: "2026-08-12".into(),
            opponent: opponent.into(),
            home_away: home_away.into(),
            result: Some(GameResult {
                outcome: outcome.into(),
                team_score: team,
                opponent_score: opp,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn rpi_matches_hand_computed_value_on_neutral_games() {
        // Three teams playing each other once on neutral sites. With
        // neutral location weights (1.0, 1.0) the adjusted winning
        // percentage collapses to plain WP, so the arithmetic below is
        // closed-form.
        //
        //   A beat B  (A: 1-0, B: 0-1)
        //   A beat C  (A: 2-0, C: 0-1)
        //   B beat C  (B: 1-1, C: 0-2)
        //
        // AWP(A) = 2/2 = 1.0, AWP(B) = 1/2 = 0.5, AWP(C) = 0/2 = 0.0
        // OWP(A) = mean(AWP_B\A, AWP_C\A) = mean(1/1, 0/1) = 0.5
        // OWP(B) = mean(AWP_A\B, AWP_C\B) = mean(1/1, 0/1) = 0.5
        // OWP(C) = mean(AWP_A\C, AWP_B\C) = mean(1/1, 0/1) = 0.5
        // OOWP(A) = mean(OWP(B), OWP(C)) = 0.5 (same for B, C)
        //
        // RPI(A) = 0.25·1.0 + 0.50·0.5 + 0.25·0.5 = 0.625
        // RPI(B) = 0.25·0.5 + 0.50·0.5 + 0.25·0.5 = 0.500
        // RPI(C) = 0.25·0.0 + 0.50·0.5 + 0.25·0.5 = 0.375
        let games = vec![
            game("a", "b", "W", 1, 0),
            game("a", "c", "W", 2, 0),
            game("b", "a", "L", 0, 1),
            game("b", "c", "W", 1, 0),
            game("c", "a", "L", 0, 2),
            game("c", "b", "L", 0, 1),
        ];
        let seed = [("a", "a"), ("b", "b"), ("c", "c")];
        let rpi = compute_rpi_full(&games, &seed);
        assert!((rpi["a"].rpi - 0.625).abs() < 1e-9, "a RPI: {}", rpi["a"].rpi);
        assert!((rpi["b"].rpi - 0.500).abs() < 1e-9, "b RPI: {}", rpi["b"].rpi);
        assert!((rpi["c"].rpi - 0.375).abs() < 1e-9, "c RPI: {}", rpi["c"].rpi);
    }

    #[test]
    fn rpi_excludes_non_di_opponents_entirely() {
        // A plays two games: beats b (in the DI seed) and beats
        // non_di_team (NOT in the seed). The second game must be
        // dropped from every component of the RPI, not just OWP/OOWP.
        //
        // After DI filter: A has 1 game (W vs b); b has 1 game (L vs A).
        // AWP(A) = 1/1 = 1.0, AWP(B) = 0/1 = 0.0
        // OWP(A) = AWP(B) with games vs A removed = None (b has no
        //   other DI games) → OWP(A) = 0.0 (empty average)
        // OOWP(A) = OWP(B) = 0.0 (same reason)
        // RPI(A) = 0.25·1.0 + 0 + 0 = 0.25
        let games = vec![
            game("a", "b", "W", 1, 0),
            game("a", "non_di_team", "W", 3, 0),
            game("b", "a", "L", 0, 1),
        ];
        let seed = [("a", "a"), ("b", "b")];
        let rpi = compute_rpi_full(&games, &seed);
        assert!((rpi["a"].rpi - 0.25).abs() < 1e-9, "a RPI: {}", rpi["a"].rpi);
        assert!((rpi["a"].wp - 1.0).abs() < 1e-9, "a AWP: {}", rpi["a"].wp);
    }

    #[test]
    fn rpi_resolves_suffix_aliases() {
        // Harvard's WMT-API opponents arrive as "Princeton University",
        // "Dartmouth College", "Cornell University" etc. The registry
        // stores just "Princeton", "Dartmouth", "Cornell". Without
        // alias resolution those opponents were marked non-DI and
        // dropped from OWP; the alias seed restores them.
        //
        // Setup: a beats "Princeton University" (which resolves to b),
        // and b has its own game vs a. Both teams play exactly one DI
        // game, so AWP(a)=1, AWP(b)=0. OWP(a) should include b's AWP
        // (0.0, not Nothing).
        let games = vec![
            game("a", "Princeton University", "W", 1, 0),
            game("b", "A", "L", 0, 1),
        ];
        let seed = [("A", "a"), ("Princeton", "b")];
        let rpi = compute_rpi_full(&games, &seed);
        // AWP(a) = 1/1 = 1.0
        // AWP(b) = 0/1 = 0.0
        // b did resolve via "Princeton University" -> "Princeton" alias.
        // OWP(a) = AWP(b excluding games vs a) = None → 0.0
        // OOWP(a) = OWP(b) = AWP(a excluding games vs b) → None → 0.0
        // RPI(a) = 0.25*1.0 = 0.25
        assert!(
            (rpi["a"].rpi - 0.25).abs() < 1e-9,
            "a RPI (with alias resolution): {}",
            rpi["a"].rpi
        );
        assert!(rpi.contains_key("b"), "b should also be ranked");
    }

    #[test]
    fn rpi_suffix_alias_does_not_shadow_direct_match() {
        // "Boston University" is a distinct DI program; the alias
        // mechanism for "Boston" (hypothetical shorter program) must
        // not win over the direct entry.
        let games = vec![
            game("boston_u", "A", "W", 1, 0),
            game("a", "Boston University", "L", 0, 1),
        ];
        // Seed with Boston University as itself. Even if some other
        // program named "Boston" were registered first with alias
        // "Boston University", the direct entry must win.
        let seed = [("Boston University", "boston_u"), ("A", "a")];
        let rpi = compute_rpi_full(&games, &seed);
        // a's opponent must resolve to boston_u, not fail or alias
        // to some phantom "Boston" slug.
        assert!(
            (rpi["a"].wp - 0.0).abs() < 1e-9,
            "a's AWP should be 0 (lost its one game)",
        );
        assert!(
            (rpi["boston_u"].wp - 1.0).abs() < 1e-9,
            "boston_u's AWP should be 1 (won its one game)",
        );
    }

    #[test]
    fn location_weighting_rewards_road_wins() {
        // Two teams, each 1-1 overall. A wins at home and loses on the
        // road; B wins on the road and loses at home. Plain WP has them
        // tied at 0.5, but location-weighted AWP favors B.
        //
        // A: home W (adj_w=0.6), away L (adj_l=0.6) → AWP = 0.5
        // B: away W (adj_w=1.4), home L (adj_l=1.4) → AWP = 0.5
        //
        // Both still 0.5 — identical schedules just mirrored. Swap:
        // give A one home W and one home L; B one away W and one away L.
        //
        // A: home W (0.6), home L (1.4) → AWP = 0.6/2.0 = 0.30
        // B: away W (1.4), away L (0.6) → AWP = 1.4/2.0 = 0.70
        let games = vec![
            game_at("a", "x", "W", 1, 0, "home"),
            game_at("a", "y", "L", 0, 1, "home"),
            game_at("b", "x", "W", 1, 0, "away"),
            game_at("b", "y", "L", 0, 1, "away"),
            // Opponents x and y need DI records so the main RPI pipeline
            // doesn't skip A/B's games; one neutral tie each keeps them
            // on the ledger.
            game_at("x", "y", "T", 0, 0, "neutral"),
            game_at("y", "x", "T", 0, 0, "neutral"),
        ];
        let seed = [("a", "a"), ("b", "b"), ("x", "x"), ("y", "y")];
        let rpi = compute_rpi_full(&games, &seed);
        assert!(
            (rpi["a"].wp - 0.30).abs() < 1e-9,
            "A AWP should be 0.30, got {}",
            rpi["a"].wp
        );
        assert!(
            (rpi["b"].wp - 0.70).abs() < 1e-9,
            "B AWP should be 0.70, got {}",
            rpi["b"].wp
        );
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
