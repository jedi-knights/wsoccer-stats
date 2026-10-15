# wsoccer-stats

Desktop viewer for NCAA Division I women's soccer statistics: standings, RPI
rankings computed on demand, and per-team drill-down into schedule and
opponents.

Solo local-use tool. Offline-capable once data is ingested.

## Status

Fresh scaffold. No data ingest, no RPI engine, no UI wired up yet.

## Stack

- Tauri v2 (Rust backend, WebView frontend)
- React + TypeScript + Vite
- Data source: TBD — see `athletics-ingest-platform` (sibling repo) for the
  roster ingest work; results/schedule/RPI ingest is not yet built.

## Development

Prerequisites: Rust, Node (LTS), Xcode Command Line Tools on macOS.
See `~/.claude/rules/tauri-setup.md` for the full per-OS list.

```
npm install
npm run tauri dev
```

## Layout

```
src/            React frontend
src-tauri/      Rust backend, tauri.conf.json, capabilities/
```

## RPI methodology

The RPI column follows the NCAA Division I women's soccer formula:

```
RPI = 0.25 · AWP + 0.50 · OWP + 0.25 · OOWP
```

- **AWP** — Adjusted Winning Percentage. Each result is weighted by game
  location before summing:

  | Result | Home | Away | Neutral |
  |---|---|---|---|
  | Win | ×0.6 | ×1.4 | ×1.0 |
  | Loss | ×1.4 | ×0.6 | ×1.0 |
  | Tie | half-win + half-loss, each at the location's weight |

  So a road win counts as 1.4 wins, a home loss counts as 1.4 losses, a
  home tie counts as 0.3 wins + 0.7 losses, and so on. The adjusted
  record is then `AWP = adj_W / (adj_W + adj_L)`.

- **OWP** — mean AWP of each DI opponent, with games against the team
  itself removed (so a team's own result doesn't inflate its opponents'
  rating). This is the Strength of Schedule component.

- **OOWP** — mean AWP of each DI opponent's own opponents.

**DI-only scope.** Games against non-DI opponents (anything not in the
`athletics-ingest-platform` registry) are excluded entirely — not just
from OWP/OOWP but from the team's own AWP record. This matches NCAA
scope: RPI counts only DI-vs-DI results.

Implementation lives in `src-tauri/src/standings.rs::compute_rpi_full`.
