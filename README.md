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
