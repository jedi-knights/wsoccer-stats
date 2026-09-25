import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

type Standing = {
  program_slug: string;
  program_name: string;
  conference: string;
  wins: number;
  losses: number;
  ties: number;
  goals_for: number;
  goals_against: number;
  games_played: number;
};

type Player = {
  program_slug: string;
  program_name: string;
  conference: string;
  cms: string;
  name: string;
  jersey_number: string;
  position: string;
  class_year: string;
  height: string;
  hometown: string;
};

type View =
  | { kind: "standings" }
  | { kind: "roster"; slug: string; name: string };

const CONFERENCE_LABELS: Record<string, string> = {
  acc: "ACC",
  sec: "SEC",
  big_ten: "Big Ten",
  big_12: "Big 12",
  west_coast: "WCC",
};

function labelFor(conf: string): string {
  return CONFERENCE_LABELS[conf] ?? conf;
}

// ---- sortable table helpers ----------------------------------------------

type SortDir = "asc" | "desc";
type SortSpec<K extends string> = { key: K; dir: SortDir };

function useSortSpec<K extends string>(): [SortSpec<K> | null, (k: K) => void] {
  const [sort, setSort] = useState<SortSpec<K> | null>(null);
  const toggle = (key: K) => {
    setSort((prev) =>
      !prev || prev.key !== key
        ? { key, dir: "asc" }
        : { key, dir: prev.dir === "asc" ? "desc" : "asc" }
    );
  };
  return [sort, toggle];
}

function compareValues(a: unknown, b: unknown): number {
  if (typeof a === "number" && typeof b === "number") return a - b;
  if (a == null && b == null) return 0;
  if (a == null) return -1;
  if (b == null) return 1;
  return String(a).localeCompare(String(b), undefined, { numeric: true });
}

function applySort<T, K extends string>(
  rows: T[],
  sort: SortSpec<K> | null,
  getValue: (row: T, key: K) => unknown
): T[] {
  if (!sort) return rows;
  const copy = [...rows];
  copy.sort((a, b) => {
    const cmp = compareValues(getValue(a, sort.key), getValue(b, sort.key));
    return sort.dir === "asc" ? cmp : -cmp;
  });
  return copy;
}

function SortHeader<K extends string>({
  label,
  sortKey,
  sort,
  onToggle,
  align,
}: {
  label: string;
  sortKey: K;
  sort: SortSpec<K> | null;
  onToggle: (k: K) => void;
  align?: "left" | "right";
}) {
  const active = sort?.key === sortKey;
  return (
    <th
      className={`sortable${align === "right" ? " right" : ""}`}
      onClick={() => onToggle(sortKey)}
      title="Click to sort"
    >
      <span className="th-label">{label}</span>
      <span className="sort-arrow" aria-hidden="true">
        {active ? (sort!.dir === "asc" ? "▲" : "▼") : ""}
      </span>
    </th>
  );
}

// ---- app ----------------------------------------------------------------

function App() {
  const [view, setView] = useState<View>({ kind: "standings" });
  return view.kind === "standings" ? (
    <StandingsPage
      onOpenRoster={(slug, name) => setView({ kind: "roster", slug, name })}
    />
  ) : (
    <RosterPage
      slug={view.slug}
      name={view.name}
      onBack={() => setView({ kind: "standings" })}
    />
  );
}

// ---- standings ----------------------------------------------------------

type StandingSortKey =
  | "program_name"
  | "conference"
  | "games_played"
  | "wins"
  | "losses"
  | "ties"
  | "goals_for"
  | "goals_against"
  | "goal_differential";

function StandingsPage({
  onOpenRoster,
}: {
  onOpenRoster: (slug: string, name: string) => void;
}) {
  const [conferences, setConferences] = useState<string[]>([]);
  const [selected, setSelected] = useState<string>(""); // "" = all
  const [standings, setStandings] = useState<Standing[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sort, toggleSort] = useSortSpec<StandingSortKey>();

  useEffect(() => {
    invoke<string[]>("list_conferences")
      .then(setConferences)
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    setStandings(null);
    setError(null);
    invoke<Standing[]>("list_standings", { conference: selected || null })
      .then(setStandings)
      .catch((e) => setError(String(e)));
  }, [selected]);

  const sortedRows = useMemo(() => {
    if (!standings) return standings;
    return applySort(standings, sort, (row, key) => {
      switch (key) {
        case "goal_differential":
          return row.goals_for - row.goals_against;
        case "conference":
          return labelFor(row.conference);
        case "program_name":
          return row.program_name || row.program_slug;
        default:
          return row[key];
      }
    });
  }, [standings, sort]);

  return (
    <main className="container">
      <header className="page-header">
        <h1>Standings</h1>
        <label className="filter">
          Conference:{" "}
          <select value={selected} onChange={(e) => setSelected(e.target.value)}>
            <option value="">All ({conferences.length})</option>
            {conferences.map((c) => (
              <option key={c} value={c}>
                {labelFor(c)}
              </option>
            ))}
          </select>
        </label>
      </header>
      {error && (
        <p className="error" role="alert">
          Error: {error}
        </p>
      )}
      {sortedRows === null && !error && <p>Loading…</p>}
      {sortedRows !== null && sortedRows.length === 0 && !error && (
        <p>No games found for this selection.</p>
      )}
      {sortedRows !== null && sortedRows.length > 0 && (
        <table className="standings">
          <thead>
            <tr>
              <SortHeader
                label="Program"
                sortKey="program_name"
                sort={sort}
                onToggle={toggleSort}
              />
              <SortHeader
                label="Conf"
                sortKey="conference"
                sort={sort}
                onToggle={toggleSort}
              />
              <SortHeader
                label="GP"
                sortKey="games_played"
                sort={sort}
                onToggle={toggleSort}
                align="right"
              />
              <SortHeader
                label="W"
                sortKey="wins"
                sort={sort}
                onToggle={toggleSort}
                align="right"
              />
              <SortHeader
                label="L"
                sortKey="losses"
                sort={sort}
                onToggle={toggleSort}
                align="right"
              />
              <SortHeader
                label="T"
                sortKey="ties"
                sort={sort}
                onToggle={toggleSort}
                align="right"
              />
              <SortHeader
                label="GF"
                sortKey="goals_for"
                sort={sort}
                onToggle={toggleSort}
                align="right"
              />
              <SortHeader
                label="GA"
                sortKey="goals_against"
                sort={sort}
                onToggle={toggleSort}
                align="right"
              />
              <SortHeader
                label="GD"
                sortKey="goal_differential"
                sort={sort}
                onToggle={toggleSort}
                align="right"
              />
            </tr>
          </thead>
          <tbody>
            {sortedRows.map((s) => (
              <tr key={s.program_slug}>
                <td>
                  <button
                    className="linklike"
                    onClick={() =>
                      onOpenRoster(s.program_slug, s.program_name || s.program_slug)
                    }
                    title={`Roster for ${s.program_name || s.program_slug}`}
                  >
                    {s.program_name || s.program_slug}
                  </button>
                </td>
                <td>{labelFor(s.conference)}</td>
                <td className="right">{s.games_played}</td>
                <td className="right">{s.wins}</td>
                <td className="right">{s.losses}</td>
                <td className="right">{s.ties}</td>
                <td className="right">{s.goals_for}</td>
                <td className="right">{s.goals_against}</td>
                <td className="right">{s.goals_for - s.goals_against}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </main>
  );
}

// ---- roster -------------------------------------------------------------

type RosterSortKey =
  | "jersey_number"
  | "name"
  | "position"
  | "class_year"
  | "height"
  | "hometown";

function RosterPage({
  slug,
  name,
  onBack,
}: {
  slug: string;
  name: string;
  onBack: () => void;
}) {
  const [players, setPlayers] = useState<Player[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sort, toggleSort] = useSortSpec<RosterSortKey>();

  useEffect(() => {
    invoke<Player[]>("list_roster", { slug })
      .then(setPlayers)
      .catch((e) => setError(String(e)));
  }, [slug]);

  const displayName = players?.[0]?.program_name || name;
  const conf = players?.[0]?.conference;

  const sortedRows = useMemo(() => {
    if (!players) return players;
    return applySort(players, sort, (row, key) => {
      if (key === "jersey_number") {
        const n = parseInt(row.jersey_number, 10);
        return Number.isFinite(n) ? n : row.jersey_number;
      }
      return row[key];
    });
  }, [players, sort]);

  return (
    <main className="container">
      <header className="page-header">
        <div>
          <button className="linklike" onClick={onBack}>
            ← Standings
          </button>
          <h1>{displayName}</h1>
          {sortedRows && conf && (
            <p className="subtitle">
              {labelFor(conf)} · {sortedRows.length} players
            </p>
          )}
        </div>
      </header>
      {error && (
        <p className="error" role="alert">
          Error: {error}
        </p>
      )}
      {sortedRows === null && !error && <p>Loading…</p>}
      {sortedRows !== null && sortedRows.length === 0 && !error && (
        <p>No roster data for this program.</p>
      )}
      {sortedRows !== null && sortedRows.length > 0 && (
        <table className="roster">
          <thead>
            <tr>
              <SortHeader
                label="#"
                sortKey="jersey_number"
                sort={sort}
                onToggle={toggleSort}
                align="right"
              />
              <SortHeader
                label="Name"
                sortKey="name"
                sort={sort}
                onToggle={toggleSort}
              />
              <SortHeader
                label="Pos"
                sortKey="position"
                sort={sort}
                onToggle={toggleSort}
              />
              <SortHeader
                label="Class"
                sortKey="class_year"
                sort={sort}
                onToggle={toggleSort}
              />
              <SortHeader
                label="Ht"
                sortKey="height"
                sort={sort}
                onToggle={toggleSort}
              />
              <SortHeader
                label="Hometown"
                sortKey="hometown"
                sort={sort}
                onToggle={toggleSort}
              />
            </tr>
          </thead>
          <tbody>
            {sortedRows.map((p, i) => (
              <tr key={`${p.jersey_number}-${p.name}-${i}`}>
                <td className="right">{p.jersey_number}</td>
                <td>{p.name}</td>
                <td>{p.position}</td>
                <td>{p.class_year}</td>
                <td>{p.height}</td>
                <td>{p.hometown}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </main>
  );
}

export default App;
