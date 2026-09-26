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

/// Normalize a team name for cross-schedule lookup.
///
/// Athletics sites decorate opponent names with poll positions
/// (`#8 Tennessee`, `#T19 South Carolina`, `#8/4 Tennessee`, `#RV TCU`,
/// `RV Texas`, `T3 Florida State`). The owner's own `program_name` never
/// carries these, so the raw string comparison misses conference matches
/// — pass both sides through this before comparing.
///
/// Also collapses the ``St.`` ↔ ``State`` abbreviation drift: Missouri's
/// own schedule writes opponents as ``Missouri St.`` while the registry
/// name is ``Missouri State`` (same for Boise St., Fresno St., Mississippi
/// St., etc.). Without this both lookups miss.
///
/// **Note:** operates on ASCII byte-substrings for the prefix strippers.
/// Team names with non-ASCII characters (a hypothetical ``UCF-Osceola``
/// with an accent) are left byte-identical after the strippers — only
/// the final ``to_lowercase()`` sees Unicode.
pub(crate) fn normalize_team_name(name: &str) -> String {
    let trimmed = name.trim();
    // Drop as many leading ranking tokens as we see. Each token is either
    // ``#<something>`` (any non-space run after ``#`` — covers `#8`,
    // `#T19`, `#8/4`, `#RV`), a parenthesized ``(#N)`` or ``(N)`` used by
    // Texas A&M, a bare `RV` (receiving votes), or a bare `T<n>` tie
    // marker. Keep chewing tokens until the next word looks like a real
    // name.
    let mut rest = trimmed;
    loop {
        let stripped = if let Some(after) = rest.strip_prefix('#') {
            // ``#<token> <name>``
            let (_, tail) = split_once_ws(after);
            tail
        } else if let Some(after) = strip_paren_ranking(rest) {
            after
        } else if let Some(after) = strip_word_ci(rest, "RV") {
            after
        } else if let Some(after) = strip_t_number_prefix(rest) {
            after
        } else {
            break;
        };
        if stripped.is_empty() {
            break;
        }
        rest = stripped;
    }
    let lower = rest.to_lowercase();
    // Collapse "St." → "State" so "Missouri St." matches "Missouri State".
    // Match whole words only: replace " st." and " st" at end-of-string
    // (case-insensitive, but ``lower`` is already lowercased).
    expand_st_abbreviation(&lower)
}

fn expand_st_abbreviation(s: &str) -> String {
    // Only the trailing "St." / "St" gets expanded to "state" — that's
    // the case where the abbreviation stands for "State" (Missouri St.,
    // Boise St., Fresno St.). A leading "St." means "Saint" (St. John's,
    // St. Thomas) and MUST be left alone.
    //
    // Also require at least one preceding word so a program literally
    // named just "St" doesn't get eaten.
    let stripped = s.strip_suffix('.').unwrap_or(s);
    if let Some(head) = stripped.strip_suffix(" st") {
        if !head.is_empty() {
            return format!("{head} state");
        }
    }
    s.to_string()
}

/// Strip a leading parenthesized ranking marker: ``(#19) TCU``,
/// ``(#T3) Florida State``, ``(19) TCU``, ``(RV) Texas``. Returns the
/// remainder after the marker + its trailing whitespace, or None if the
/// input doesn't start with one.
fn strip_paren_ranking(s: &str) -> Option<&str> {
    let rest = s.strip_prefix('(')?;
    let close = rest.find(')')?;
    let inside = rest[..close].trim();
    if inside.is_empty() {
        return None;
    }
    // Accept only content that looks like a rank marker so we don't chew
    // legitimate parenthetical opponents like ``(Ohio)`` off "Miami (Ohio)".
    let looks_like_rank = inside.starts_with('#')
        || inside.eq_ignore_ascii_case("RV")
        || inside
            .trim_start_matches(|c: char| c == 'T' || c == 't')
            .chars()
            .all(|c| c.is_ascii_digit() || c == '/')
            && inside.chars().any(|c| c.is_ascii_digit());
    if !looks_like_rank {
        return None;
    }
    let after_paren = &rest[close + 1..];
    let after_ws = after_paren.trim_start();
    if after_ws.is_empty() {
        None
    } else {
        Some(after_ws)
    }
}

fn split_once_ws(s: &str) -> (&str, &str) {
    match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], s[i..].trim_start()),
        None => (s, ""),
    }
}

fn strip_word_ci<'a>(s: &'a str, word: &str) -> Option<&'a str> {
    let (head, tail) = split_once_ws(s);
    if head.eq_ignore_ascii_case(word) && !tail.is_empty() {
        Some(tail)
    } else {
        None
    }
}

/// Add `days` to an ISO-8601 `YYYY-MM-DD` date string and return the
/// result as ISO. Used only for the conference-play cluster window; a
/// simple non-leap-aware calendar walk is sufficient because the window
/// is small (~7 days) and callers only use the result for lexicographic
/// string comparison, not for real date arithmetic.
fn add_days_iso(date: &str, days: u32) -> String {
    // Parse YYYY-MM-DD without pulling in a date crate. Bail out with
    // the input unchanged on any parse failure — the caller falls back
    // to accepting the game.
    let bytes = date.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return date.to_string();
    }
    let year: i32 = match date[0..4].parse() {
        Ok(y) => y,
        Err(_) => return date.to_string(),
    };
    let month: u32 = match date[5..7].parse() {
        Ok(m) => m,
        Err(_) => return date.to_string(),
    };
    let day: u32 = match date[8..10].parse() {
        Ok(d) => d,
        Err(_) => return date.to_string(),
    };
    let (mut y, mut m, mut d) = (year, month, day);
    for _ in 0..days {
        d += 1;
        let dim = days_in_month(y, m);
        if d > dim {
            d = 1;
            m += 1;
            if m > 12 {
                m = 1;
                y += 1;
            }
        }
    }
    format!("{y:04}-{m:02}-{d:02}")
}

fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            let leap = (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0);
            if leap {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

fn strip_t_number_prefix(s: &str) -> Option<&str> {
    let (head, tail) = split_once_ws(s);
    if tail.is_empty() {
        return None;
    }
    let mut chars = head.chars();
    let first = chars.next()?;
    if first != 'T' && first != 't' {
        return None;
    }
    let rest: String = chars.collect();
    if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
        Some(tail)
    } else {
        None
    }
}

fn load_games() -> Result<Vec<Game>, String> {
    let dir = schedules_dir();
    data::read_ndjson_dir(&dir)
        .map_err(|e| format!("failed to read schedule data from {}: {e}", dir.display()))
}

/// A single row from the athletics-ingest-platform program registry
/// (`output/programs.ndjson`). We only need the three fields the app
/// consults — slug for standings identity, name for opponent lookup,
/// and conference for filtering.
#[derive(Debug, Clone)]
struct RegistryProgram {
    slug: String,
    name: String,
    conference: String,
}

/// Load `output/programs.ndjson` from the data dir, silently returning
/// empty when the file doesn't exist (older ingest runs won't have it).
fn load_program_registry() -> Vec<RegistryProgram> {
    let path = data_dir().join("programs.ndjson");
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    contents
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let entry: serde_json::Value = serde_json::from_str(line).ok()?;
            let slug = entry.get("slug")?.as_str()?.to_string();
            let name = entry.get("name")?.as_str()?.to_string();
            let conference = entry.get("conference")?.as_str()?.to_string();
            if slug.is_empty() || name.is_empty() || conference.is_empty() {
                return None;
            }
            Some(RegistryProgram {
                slug,
                name,
                conference,
            })
        })
        .collect()
}

#[tauri::command]
fn list_standings(
    conference: Option<String>,
    mode: Option<String>,
) -> Result<Vec<Standing>, String> {
    let all_games = load_games()?;

    // Build a program_name → conference lookup used to classify each
    // opponent as conference or non-conference relative to the schedule
    // owner.
    //
    // Seed order matters:
    // 1. Every played game (fast, and picks up any program with a schedule
    //    on disk regardless of whether it's on the P4/WCC list).
    // 2. `programs.ndjson` — every program the aip registry knows about,
    //    including ones whose own schedule failed to ingest. Without this,
    //    a game against Oklahoma looks like a non-conference opponent
    //    just because Oklahoma's SPA scrape is unhandled.
    let registry = load_program_registry();

    let mut name_to_conf: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for g in &all_games {
        if !g.program_name.is_empty() && !g.conference.is_empty() {
            name_to_conf
                .entry(normalize_team_name(&g.program_name))
                .or_insert_with(|| g.conference.clone());
        }
    }
    for p in &registry {
        name_to_conf
            .entry(normalize_team_name(&p.name))
            .or_insert_with(|| p.conference.clone());
    }
    let is_same_conf = |g: &Game| -> bool {
        name_to_conf
            .get(&normalize_team_name(&g.opponent))
            .is_some_and(|opp_conf| opp_conf == &g.conference)
    };

    // Preseason rivalry games between conference members (Texas A&M vs
    // Texas on Aug 8, 2026 is the canonical case) should NOT count as
    // conference play. Conferences start "real" conference play on a
    // specific weekend — we identify it as the earliest date where the
    // conference has at least THRESHOLD same-conf games clustered.
    //
    // Any same-conf game that lands before its conference's earliest
    // cluster date is treated as non-conference.
    const CLUSTER_THRESHOLD: usize = 3;
    let mut same_conf_dates_by_conf: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for g in &all_games {
        if g.result.is_some() && is_same_conf(g) {
            same_conf_dates_by_conf
                .entry(g.conference.clone())
                .or_default()
                .push(g.date.clone());
        }
    }
    let conf_play_start: std::collections::HashMap<String, String> = same_conf_dates_by_conf
        .into_iter()
        .filter_map(|(conf, mut dates)| {
            dates.sort();
            // First date d such that ≥ CLUSTER_THRESHOLD games fall in
            // [d, d + 7 days]. Compare on ISO strings — since every date
            // is `YYYY-MM-DD`, lexicographic compare + a naive 7-day
            // window suffices (crossing month/year is rare for a soccer
            // season and the cluster is by count, not by exact days).
            for i in 0..dates.len() {
                let window_end = add_days_iso(&dates[i], 7);
                let n = dates[i..]
                    .iter()
                    .take_while(|d| **d <= window_end)
                    .count();
                if n >= CLUSTER_THRESHOLD {
                    return Some((conf, dates[i].clone()));
                }
            }
            None
        })
        .collect();

    let is_conference_game = |g: &Game| -> bool {
        if !is_same_conf(g) {
            return false;
        }
        // If we couldn't identify a cluster start (conference has < 3
        // same-conf games total), fall back to accepting all same-conf
        // games — better to overcount than to drop legit conference
        // play in a small conference.
        match conf_play_start.get(&g.conference) {
            Some(start) => g.date.as_str() >= start.as_str(),
            None => true,
        }
    };

    // RPI is a season-wide metric — always compute from the full loaded
    // graph regardless of the requested conference/mode filter, then rank
    // every ranked program relative to the full universe so a team's rank
    // is the same whether the user is viewing SEC-only or all conferences.
    // Seed the RPI opponent lookup from the registry so games against
    // programs whose schedule failed to ingest still count in OWP/OOWP.
    let registry_seed: Vec<(&str, &str)> = registry
        .iter()
        .map(|p| (p.name.as_str(), p.slug.as_str()))
        .collect();
    let rpi_full = standings::compute_rpi_full(&all_games, &registry_seed);
    let rpi_by_slug: std::collections::HashMap<String, f64> =
        rpi_full.iter().map(|(k, v)| (k.clone(), v.rpi)).collect();
    let rank_by_slug = standings::rank_by_slug(&rpi_by_slug);

    let mut games = all_games;
    if let Some(c) = conference.as_deref().filter(|c| !c.is_empty()) {
        games.retain(|g| g.conference == c);
    }
    match mode.as_deref().unwrap_or("all") {
        "conference" => games.retain(is_conference_game),
        "non_conference" => games.retain(|g| !is_conference_game(g)),
        _ => {}
    }
    let mut standings = standings::compute_standings(&games);
    for s in &mut standings {
        s.rpi_rank = rank_by_slug.get(&s.program_slug).copied().unwrap_or(0);
        if let Some(comp) = rpi_full.get(&s.program_slug) {
            s.rpi = comp.rpi;
            s.sos = comp.owp;
        }
    }

    // A team with no games we could ingest (Oklahoma's Nuxt SPA is the
    // canonical case) should still appear in its conference's standings
    // so users can see the roster is incomplete rather than silently
    // dropping the program. Fill in zeros from the registry.
    let have_slugs: std::collections::HashSet<String> =
        standings.iter().map(|s| s.program_slug.clone()).collect();
    for p in &registry {
        if have_slugs.contains(&p.slug) {
            continue;
        }
        // Respect the same conference filter used for played games —
        // an unfiltered request shows every conference member; a
        // conference-scoped request only fills in that conference.
        if let Some(c) = conference.as_deref().filter(|c| !c.is_empty()) {
            if p.conference != *c {
                continue;
            }
        }
        standings.push(Standing {
            program_slug: p.slug.clone(),
            program_name: p.name.clone(),
            conference: p.conference.clone(),
            rpi_rank: rank_by_slug.get(&p.slug).copied().unwrap_or(0),
            has_schedule_data: false,
            ..Standing::default()
        });
    }
    // Re-sort so the zero-record stubs slot into the right position by
    // the same rules `compute_standings` uses (points desc → GD desc →
    // slug asc). Zeros land at the bottom of a filled conference.
    standings.sort_by(|a, b| {
        let gd_a = a.goals_for as i32 - a.goals_against as i32;
        let gd_b = b.goals_for as i32 - b.goals_against as i32;
        b.points
            .cmp(&a.points)
            .then_with(|| gd_b.cmp(&gd_a))
            .then_with(|| a.program_slug.cmp(&b.program_slug))
    });
    Ok(standings)
}

/// One conference the app knows about, with its display label.
#[derive(Debug, Clone, Serialize)]
pub struct ConferenceEntry {
    pub slug: String,
    /// Short display label ("SEC", "ACC", "A-10"). Sourced from
    /// `output/conferences.ndjson` when present; falls back to the raw
    /// slug when the file is missing.
    pub label: String,
    /// Full formal name ("Southeastern Conference"). Empty when only
    /// the slug is known.
    pub full_name: String,
}

fn load_conference_labels() -> std::collections::HashMap<String, (String, String)> {
    // slug → (label, full_name). Empty map when the file isn't present
    // — the frontend falls back to a slug-based title-case.
    let path = data_dir().join("conferences.ndjson");
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return std::collections::HashMap::new();
    };
    let mut out = std::collections::HashMap::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let slug = entry
            .get("slug")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let short = entry
            .get("short_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let full = entry
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if !slug.is_empty() {
            out.insert(slug, (short, full));
        }
    }
    out
}

/// Every conference the app knows about, sorted alphabetically by
/// display label so the dropdown reads naturally to a user scanning it.
///
/// Prefers the program registry (every conference with at least one
/// registered program shows up, even if no schedule ingest succeeded
/// for it). Falls back to the schedule-data union when the registry
/// isn't present.
#[tauri::command]
fn list_conferences() -> Result<Vec<ConferenceEntry>, String> {
    let labels = load_conference_labels();
    let mut slugs: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for p in &load_program_registry() {
        if !p.conference.is_empty() {
            slugs.insert(p.conference.clone());
        }
    }
    if slugs.is_empty() {
        let games = load_games()?;
        for g in &games {
            if !g.conference.is_empty() {
                slugs.insert(g.conference.clone());
            }
        }
    }
    let mut entries: Vec<ConferenceEntry> = slugs
        .into_iter()
        .map(|slug| {
            let (label, full_name) = labels
                .get(&slug)
                .cloned()
                .unwrap_or_else(|| (slug.clone(), String::new()));
            let label = if label.is_empty() { slug.clone() } else { label };
            ConferenceEntry {
                slug,
                label,
                full_name,
            }
        })
        .collect();
    entries.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
    Ok(entries)
}

/// One played game between two members of the same conference, from
/// the schedule owner's perspective. Used by the head-to-head matrix.
#[derive(Debug, Clone, Serialize)]
pub struct H2HGame {
    /// Schedule owner (rows in the H2H matrix are keyed by this).
    pub program_slug: String,
    /// Opponent resolved via the registry / games name_to_slug lookup.
    /// Empty when the opponent name couldn't be matched to a registry
    /// program — such games are skipped by this endpoint, so this
    /// field is always non-empty in the returned rows.
    pub opponent_slug: String,
    pub date: String,
    pub home_away: String,
    pub outcome: String,
    pub team_score: u32,
    pub opponent_score: u32,
}

/// Head-to-head games between members of one conference — one row per
/// game per team (games appear twice, once from each team's perspective,
/// since the frontend renders a symmetric matrix).
///
/// Filters out games before the conference's cluster-derived play-start
/// date so the matrix matches what the Conference standings mode shows.
#[tauri::command]
fn list_head_to_head(conference: String) -> Result<Vec<H2HGame>, String> {
    if conference.is_empty() {
        return Err("conference is required for head-to-head".into());
    }
    let all_games = load_games()?;
    let registry = load_program_registry();

    // Reuse the same name-lookup shape as list_standings so classification
    // matches exactly: seed from games first, then registry so all D1
    // programs are addressable even if they have no schedule.
    let mut name_to_slug: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut name_to_conf: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for g in &all_games {
        if !g.program_name.is_empty() {
            let k = normalize_team_name(&g.program_name);
            name_to_slug.entry(k.clone()).or_insert_with(|| g.program_slug.clone());
            if !g.conference.is_empty() {
                name_to_conf.entry(k).or_insert_with(|| g.conference.clone());
            }
        }
    }
    for p in &registry {
        let k = normalize_team_name(&p.name);
        name_to_slug.entry(k.clone()).or_insert_with(|| p.slug.clone());
        name_to_conf.entry(k).or_insert_with(|| p.conference.clone());
    }

    // Compute the conference-play cluster start (identical logic to
    // list_standings) so we drop preseason rivalries between conference
    // members from the matrix.
    let is_same_conf = |g: &Game| -> bool {
        name_to_conf
            .get(&normalize_team_name(&g.opponent))
            .is_some_and(|opp_conf| opp_conf == &g.conference)
    };
    const CLUSTER_THRESHOLD: usize = 3;
    let mut same_conf_dates: Vec<String> = all_games
        .iter()
        .filter(|g| g.result.is_some() && g.conference == conference && is_same_conf(g))
        .map(|g| g.date.clone())
        .collect();
    same_conf_dates.sort();
    let mut conf_play_start: Option<String> = None;
    for i in 0..same_conf_dates.len() {
        let window_end = add_days_iso(&same_conf_dates[i], 7);
        let n = same_conf_dates[i..]
            .iter()
            .take_while(|d| **d <= window_end)
            .count();
        if n >= CLUSTER_THRESHOLD {
            conf_play_start = Some(same_conf_dates[i].clone());
            break;
        }
    }

    let mut rows: Vec<H2HGame> = Vec::new();
    for g in &all_games {
        if g.conference != conference {
            continue;
        }
        let Some(result) = &g.result else { continue };
        if !is_same_conf(g) {
            continue;
        }
        if let Some(start) = &conf_play_start {
            if g.date.as_str() < start.as_str() {
                continue;
            }
        }
        let opp_slug = match name_to_slug.get(&normalize_team_name(&g.opponent)) {
            Some(s) => s.clone(),
            None => continue,
        };
        rows.push(H2HGame {
            program_slug: g.program_slug.clone(),
            opponent_slug: opp_slug,
            date: g.date.clone(),
            home_away: g.home_away.clone(),
            outcome: result.outcome.clone(),
            team_score: result.team_score,
            opponent_score: result.opponent_score,
        });
    }
    Ok(rows)
}

/// One row in the conferences summary view.
#[derive(Debug, Clone, Serialize)]
pub struct ConferenceSummary {
    pub conference: String,
    /// How many programs the registry lists in this conference.
    pub team_count: usize,
    /// How many of those teams have any played games (are RPI-ranked).
    pub ranked_count: usize,
    /// Mean RPI rank across the ranked members. ``None`` when no member
    /// has a rank yet (early season / conference has no ingested data).
    pub avg_rpi_rank: Option<f64>,
}

/// Per-conference summary sorted by average RPI rank ascending
/// (best-ranked conference first). Powers the Conferences tab.
///
/// Uses the same registry + RPI rank map as ``list_standings`` so a
/// conference's ordering here is consistent with the RPI column in
/// its standings view.
#[tauri::command]
fn list_conference_summary() -> Result<Vec<ConferenceSummary>, String> {
    let all_games = load_games()?;
    let registry = load_program_registry();
    let registry_seed: Vec<(&str, &str)> = registry
        .iter()
        .map(|p| (p.name.as_str(), p.slug.as_str()))
        .collect();
    let rpi_full = standings::compute_rpi_full(&all_games, &registry_seed);
    let rpi_by_slug: std::collections::HashMap<String, f64> =
        rpi_full.iter().map(|(k, v)| (k.clone(), v.rpi)).collect();
    // Share the ranking helper with ``list_standings`` so the RPI column
    // and the "avg RPI rank" here can't drift apart.
    let rank_by_slug = standings::rank_by_slug(&rpi_by_slug);

    // Group by conference and compute mean rank across ranked members.
    let mut by_conf: std::collections::HashMap<String, (usize, Vec<u32>)> =
        std::collections::HashMap::new();
    for p in &registry {
        if p.conference.is_empty() {
            continue;
        }
        let entry = by_conf.entry(p.conference.clone()).or_default();
        entry.0 += 1;
        if let Some(rank) = rank_by_slug.get(&p.slug) {
            entry.1.push(*rank);
        }
    }

    let mut rows: Vec<ConferenceSummary> = by_conf
        .into_iter()
        .map(|(conference, (team_count, ranks))| {
            let ranked_count = ranks.len();
            let avg_rpi_rank = if ranks.is_empty() {
                None
            } else {
                Some(ranks.iter().map(|r| *r as f64).sum::<f64>() / ranks.len() as f64)
            };
            ConferenceSummary {
                conference,
                team_count,
                ranked_count,
                avg_rpi_rank,
            }
        })
        .collect();

    // Sort: conferences with an average rank first (best first),
    // unranked conferences at the end alphabetically.
    rows.sort_by(|a, b| match (a.avg_rpi_rank, b.avg_rpi_rank) {
        (Some(x), Some(y)) => x
            .partial_cmp(&y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.conference.cmp(&b.conference)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.conference.cmp(&b.conference),
    });
    Ok(rows)
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
    // sidearm strips it, the WMT API returns the plain digit) and on
    // leading zeros ("07" vs "7" for the same player) — normalise both
    // sides before joining. When the value is a pure integer we round-
    // trip through u32 to strip zero padding without touching non-numeric
    // jerseys (some sites use "GK" or "TR").
    fn norm(j: &str) -> String {
        let stripped = j.trim_start_matches('#').trim();
        if let Ok(n) = stripped.parse::<u32>() {
            n.to_string()
        } else {
            stripped.to_string()
        }
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

/// Where the aip Python pipeline lives — set at compile time to the
/// sibling repo directory. Overridable at runtime by
/// ``WSOCCER_STATS_AIP_DIR`` for people who install elsewhere.
fn aip_dir() -> PathBuf {
    if let Ok(v) = std::env::var("WSOCCER_STATS_AIP_DIR") {
        return PathBuf::from(v);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("athletics-ingest-platform")
}

#[derive(Debug, Clone, Serialize)]
pub struct RefreshResult {
    pub kind: String,
    pub conference: String,
    pub ok: bool,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum RefreshEvent {
    /// Sent once at the start with the total step count so the UI can size
    /// its progress bar without recomputing.
    Started { total: usize },
    /// Sent after each conference/kind subprocess completes.
    Step {
        index: usize,
        total: usize,
        result: RefreshResult,
    },
    /// Sent once at the end with the number of successful steps.
    Finished { ok: usize, total: usize },
}

/// Kick off a fresh ingest against live athletics sites.
///
/// Runs ``python -m athletics_ingest {schedule,stats}`` against the
/// sibling aip repo, scoping to ``conference`` when supplied (else all
/// Power 4 + WCC). Each subprocess writes NDJSON into ``output/``; the
/// existing list_standings / list_leaders / list_roster commands read
/// that directory on demand, so the UI just needs to re-invoke them
/// after this completes.
///
/// Blocks until every scheduled subprocess has been attempted. A full
/// unfiltered refresh takes ~2 minutes on a warm network (320 schedule
/// requests + 5 conference stats runs). Per-step progress is streamed
/// on the ``on_progress`` channel so the UI can render a live bar
/// rather than freezing.
///
/// When ``conference`` is set, the refresh is scoped to that single
/// conference (both its schedule and its stats). When it's None, we
/// run one schedule pass covering EVERY D1 program (the aip CLI
/// processes all 320 programs in a single subprocess when no
/// ``--conference`` flag is passed) and then one stats pass per
/// P4+WCC conference (stats are the more expensive scrape, and the
/// current UI only surfaces per-player stats for those five).
#[tauri::command]
async fn refresh_data(
    conference: Option<String>,
    on_progress: tauri::ipc::Channel<RefreshEvent>,
) -> Result<Vec<RefreshResult>, String> {
    let aip = aip_dir();
    if !aip.exists() {
        return Err(format!(
            "aip repo not found at {} — set WSOCCER_STATS_AIP_DIR",
            aip.display()
        ));
    }
    let output_dir = aip.join("output");

    // Build the (kind, conference_scope) work list. `""` means no
    // ``--conference`` filter (process every D1 program).
    //
    // The aip CLI uses subcommand names: ``ingest`` for rosters,
    // ``schedule`` for schedules, ``stats`` for per-player stats.
    let mut jobs: Vec<(&'static str, String)> = Vec::new();
    match conference.as_deref().filter(|c| !c.is_empty()) {
        Some(c) => {
            jobs.push(("ingest", c.to_string()));
            jobs.push(("schedule", c.to_string()));
            jobs.push(("stats", c.to_string()));
        }
        None => {
            // Every kind covers EVERY D1 program — with 20 workers
            // each pass runs in under a minute, and a user opening a
            // CAA team's roster expects to see the same stats we
            // surface for a P4/WCC team. Single subprocess per kind
            // is faster than 31 per-conference calls.
            jobs.push(("ingest", String::new()));
            jobs.push(("schedule", String::new()));
            jobs.push(("stats", String::new()));
        }
    }

    let total = jobs.len();
    let _ = on_progress.send(RefreshEvent::Started { total });

    let mut results = Vec::new();
    for (index, (kind, conf)) in jobs.into_iter().enumerate() {
        let output = tauri::async_runtime::spawn_blocking({
            let aip = aip.clone();
            let output_dir = output_dir.clone();
            let conf = conf.clone();
            move || {
                let mut cmd = std::process::Command::new("uv");
                cmd.current_dir(&aip).args([
                    "run",
                    "python",
                    "-m",
                    "athletics_ingest",
                    kind,
                ]);
                if !conf.is_empty() {
                    cmd.args(["--conference", &conf]);
                }
                cmd.args(["--output-dir"]).arg(&output_dir).output()
            }
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("spawn failed for {kind}/{conf}: {e}"))?;

        let stderr = String::from_utf8_lossy(&output.stderr);
        let summary = stderr
            .lines()
            .filter(|l| !l.trim().is_empty())
            .next_back()
            .unwrap_or("")
            .to_string();
        let result = RefreshResult {
            kind: kind.to_string(),
            // Preserve "all D1" as an empty string in the payload so
            // the UI can render "schedule (all)" rather than a bogus
            // "schedule ()" — surfaced as "all" below in the label.
            conference: if conf.is_empty() {
                "all".to_string()
            } else {
                conf.clone()
            },
            ok: output.status.success(),
            summary,
        };
        let _ = on_progress.send(RefreshEvent::Step {
            index: index + 1,
            total,
            result: result.clone(),
        });
        results.push(result);
    }
    let ok_count = results.iter().filter(|r| r.ok).count();
    let _ = on_progress.send(RefreshEvent::Finished {
        ok: ok_count,
        total,
    });
    Ok(results)
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

#[cfg(test)]
mod name_norm_tests {
    use super::normalize_team_name as n;

    #[test]
    fn strips_hash_number_prefix() {
        assert_eq!(n("#8 Tennessee"), "tennessee");
        assert_eq!(n("#19/14 TCU"), "tcu");
        assert_eq!(n("#T19 South Carolina"), "south carolina");
        assert_eq!(n("#RV Texas"), "texas");
    }

    #[test]
    fn strips_bare_rv_and_tie_marker() {
        assert_eq!(n("RV Texas"), "texas");
        assert_eq!(n("T3 Florida State"), "florida state");
    }

    #[test]
    fn strips_multiple_stacked_prefixes() {
        assert_eq!(n("#RV RV Texas"), "texas");
    }

    #[test]
    fn leaves_normal_names_untouched() {
        assert_eq!(n("Tennessee"), "tennessee");
        assert_eq!(n("Florida State"), "florida state");
        assert_eq!(n("Mississippi State"), "mississippi state");
    }

    #[test]
    fn collapses_st_abbreviation() {
        // Both the site's "Missouri St." and the registry's "Missouri State"
        // must normalize to the same lookup key.
        assert_eq!(n("Missouri St."), "missouri state");
        assert_eq!(n("Missouri State"), "missouri state");
        assert_eq!(n("Boise St"), "boise state");
        assert_eq!(n("#7 Fresno St."), "fresno state");
    }

    #[test]
    fn does_not_touch_st_in_other_positions() {
        // Words that happen to start with "st" aren't the abbreviation.
        assert_eq!(n("Stanford"), "stanford");
        assert_eq!(n("Stony Brook"), "stony brook");
        // Leading "St." means "Saint" (St. John's, St. Thomas) — leave it.
        assert_eq!(n("St. John's"), "st. john's");
        assert_eq!(n("St. Thomas"), "st. thomas");
        // "St" as the sole name is left alone (no leading word to modify).
        assert_eq!(n("St"), "st");
    }

    #[test]
    fn strips_parenthesized_rank() {
        assert_eq!(n("(#19) TCU"), "tcu");
        assert_eq!(n("(#4) Vanderbilt"), "vanderbilt");
        assert_eq!(n("(#7) Alabama"), "alabama");
        assert_eq!(n("(19) TCU"), "tcu");
        assert_eq!(n("(RV) Texas"), "texas");
        assert_eq!(n("(#T3) Florida State"), "florida state");
    }

    #[test]
    fn does_not_eat_geographic_qualifier() {
        // The parenthesized-rank stripper must not consume the ``(Ohio)``
        // qualifier on names like "Miami (Ohio)" — those aren't a ranking.
        assert_eq!(n("Miami (Ohio)"), "miami (ohio)");
    }

    #[test]
    fn add_days_iso_walks_month_boundary() {
        use super::add_days_iso;
        assert_eq!(add_days_iso("2026-08-08", 7), "2026-08-15");
        assert_eq!(add_days_iso("2026-08-28", 7), "2026-09-04");
        assert_eq!(add_days_iso("2026-09-10", 7), "2026-09-17");
        assert_eq!(add_days_iso("2026-12-30", 3), "2027-01-02");
        // Malformed input returns unchanged (caller falls back on this).
        assert_eq!(add_days_iso("not-a-date", 7), "not-a-date");
    }

    #[test]
    fn does_not_eat_the_only_word() {
        // A bare "RV" or "#8" with no following word is nonsense but
        // should not collapse to empty — leave it alone.
        assert_eq!(n("RV"), "rv");
        assert_eq!(n("#8"), "#8");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            list_standings,
            list_conferences,
            list_conference_summary,
            list_head_to_head,
            list_roster,
            list_schedule,
            list_leaders,
            refresh_data
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
