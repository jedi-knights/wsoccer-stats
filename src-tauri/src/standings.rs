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

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Standing {
    pub program_slug: String,
    pub wins: u32,
    pub losses: u32,
    pub ties: u32,
    pub goals_for: u32,
    pub goals_against: u32,
    pub games_played: u32,
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
                ..Standing::default()
            });
        entry.wins += w;
        entry.losses += l;
        entry.ties += t;
        entry.goals_for += result.team_score;
        entry.goals_against += result.opponent_score;
        entry.games_played += 1;

        // Invariant: every counted game bumped exactly one of W/L/T.
        debug_assert_eq!(entry.wins + entry.losses + entry.ties, entry.games_played);
    }
    let mut standings: Vec<Standing> = by_program.into_values().collect();
    standings.sort_by(|a, b| {
        let gd_a = a.goals_for as i32 - a.goals_against as i32;
        let gd_b = b.goals_for as i32 - b.goals_against as i32;
        b.wins
            .cmp(&a.wins)
            .then_with(|| gd_b.cmp(&gd_a))
            .then_with(|| a.program_slug.cmp(&b.program_slug))
    });
    standings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::GameResult;

    fn played(program: &str, outcome: &str, team: u32, opp: u32) -> Game {
        Game {
            program_slug: program.into(),
            cms: "sidearm".into(),
            date: "2026-08-12".into(),
            opponent: "someone".into(),
            home_away: "home".into(),
            result: Some(GameResult {
                outcome: outcome.into(),
                team_score: team,
                opponent_score: opp,
            }),
        }
    }

    fn future(program: &str) -> Game {
        Game {
            program_slug: program.into(),
            cms: "sidearm".into(),
            date: "2026-10-01".into(),
            opponent: "someone".into(),
            home_away: "home".into(),
            result: None,
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
        assert_eq!(duke.wins, 2);
        assert_eq!(duke.losses, 1);
        assert_eq!(duke.ties, 1);
        assert_eq!(duke.goals_for, 8);
        assert_eq!(duke.goals_against, 4);
        assert_eq!(duke.games_played, 4);
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
