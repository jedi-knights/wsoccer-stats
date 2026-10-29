# wsoccer-stats — TODO

## Pending ideas (parked, pick any order)

- [ ] **Staleness warning on the Updated badge** — colour the badge muted today, warning past ~12h, so "I should refresh" is obvious at a glance. Complements `<LastRefreshedBadge>` added in PR #41.
- [ ] **Filter / dismiss noise in the Issues panel** — permanent-noise entries (e.g. `harvard` "unknown CMS", programs that have never been scrapeable) crowd the real signal. Add a dismiss button that persists via `localStorage`, or auto-hide errors unchanged across runs. Panel added in PR #42.
- [ ] **Deep links / URL state** — encode `?view=standings&conf=west_coast` in the webview URL so a specific view is shareable and restorable from a cold start. Pairs with the dropdown-filter persistence in PR #37.
- [ ] **Game detail / box score view** — click a schedule row and see goals/cards/lineups. Big lift; needs new box-score parsers in `athletics-ingest-platform` (per-CMS).
- [ ] **Scheduled background refresh** — slow (hourly) auto-refresh independent of the Today view, so the app's always-fresh even if it hasn't been opened in a day. Mirrors the Today-view live polling pattern from PRs #33 / #40.
- [ ] **CSV export** of standings / schedule / roster — one-shot "download as CSV" from the per-table copy buttons already in place.
- [ ] **`--since` filter in `athletics-ingest-platform` CLI** — the biggest refresh perf win left (cross-repo). Cuts an all-DI refresh from ~15 s to seconds by only touching programs with games in the last few days.

---

## Historical scaffolding notes (keep for context)

Fresh Tauri scaffold. Nothing wired to real data yet. Work in the order below
unless there's a specific reason not to — data has to exist before a viewer is
worth building.

## Load-bearing decision (do this first)

- [ ] **Decide where results/schedule data comes from.** Three options:
  - Extend `athletics-ingest-platform` with a schedule/results parser
    (fits its "registry + CMS-agnostic parsers" shape; leverages existing
    Sidearm/WMT work).
  - Consume an existing NCAA feed (verify one is usable and licensable
    before assuming it exists).
  - Manual entry for a small slice (spike-only; not a real answer).
  - **Decision:** ___

## Data layer (before any UI work)

- [ ] Model the domain: `Team`, `Game`, `Result`, `Season`. One place.
- [ ] Schedule/results parser for one CMS (Sidearm covers most D1 sites).
      Fixture-driven, same pattern as `athletics-ingest-platform/parsers/wmt.py`.
- [ ] Storage format for parsed results (SQLite? NDJSON? JSON per team?).
      Pick something the Rust backend can read cheaply on demand.
- [ ] RPI engine as a pure function over the results table.
      Testable in isolation. NCAA women's soccer RPI formula:
      - 25% team WP (win %), 50% opponents' WP, 25% opponents' opponents' WP
      - Confirm exact weights against the current NCAA methodology before locking.

## Tauri backend (`src-tauri/`)

- [ ] Define command surface. Minimum viable:
  - `list_standings(conference?: string) -> Vec<StandingRow>`
  - `get_team(slug: string) -> TeamDetail`
  - `get_team_schedule(slug: string) -> Vec<Game>`
  - `compute_rpi(season: string) -> Vec<RpiRow>`
- [ ] Every command returns `Result<T, CommandError>` — no `.unwrap()`.
- [ ] Add the corresponding capability entries in `capabilities/default.json`
      when each command is added — do not defer.
- [ ] Tighten `core:default` to the specific `core:*` permissions actually used
      once the command surface is stable.

## Frontend (`src/`)

- [ ] Router setup (React Router or TanStack Router) — routes: `/standings`,
      `/team/:slug`, `/rpi`.
- [ ] Table library: TanStack Table v8 for standings + schedule.
- [ ] Standings page — sortable columns, conference filter.
- [ ] Team detail page — schedule, opponent list, RPI trajectory.
- [ ] RPI page — full ranked table, filterable by conference.

## Housekeeping

- [ ] Set up CI (build check for Rust + typecheck for TS + `npm run tauri build`
      dry run on macOS runner). Coverage-HTML step uses `genhtml`, not
      ReportGenerator (see `~/.claude/rules/coverage-report-tooling.md`).
- [ ] Decide GitHub remote — private or public? `gh repo create` when ready.
- [ ] Icon + app metadata (currently the default Tauri logo).
- [ ] Signing/notarization plan before first distributable build (only if
      shipping to anyone other than yourself).

## Deliberately deferred

- Mobile targets (iOS/Android) — desktop-only per scope.
- Multi-season history — get current season working first.
- Player-level stats — scope is team-level standings/RPI/schedule.
- Auto-update — solo local use, not needed.
