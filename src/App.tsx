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
  points: number;
};

type GameResult = {
  outcome: "W" | "L" | "T" | string;
  team_score: number;
  opponent_score: number;
};

type Game = {
  program_slug: string;
  program_name: string;
  conference: string;
  cms: string;
  date: string;
  opponent: string;
  home_away: string;
  result: GameResult | null;
};

type RosterEntry = {
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
  // Present when the program has a stats file and the player appears in it.
  games_played: number | null;
  games_started: number | null;
  minutes: number | null;
  goals: number | null;
  assists: number | null;
};

type View =
  | { kind: "standings"; conference: string }
  | { kind: "roster"; slug: string; name: string; fromConference: string };

const CONFERENCE_LABELS: Record<string, string> = {
  acc: "ACC",
  sec: "SEC",
  big_ten: "Big Ten",
  big_12: "Big 12",
  west_coast: "WCC",
};

const CONFERENCE_FULL_NAMES: Record<string, string> = {
  acc: "Atlantic Coast Conference",
  sec: "Southeastern Conference",
  big_ten: "Big Ten Conference",
  big_12: "Big 12 Conference",
  west_coast: "West Coast Conference",
};

function labelFor(conf: string): string {
  return CONFERENCE_LABELS[conf] ?? conf;
}

function fullNameFor(conf: string): string {
  return CONFERENCE_FULL_NAMES[conf] ?? labelFor(conf);
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
  const [view, setView] = useState<View>({ kind: "standings", conference: "" });
  return view.kind === "standings" ? (
    <StandingsPage
      initialConference={view.conference}
      onOpenRoster={(slug, name, fromConference) =>
        setView({ kind: "roster", slug, name, fromConference })
      }
    />
  ) : (
    <RosterPage
      slug={view.slug}
      name={view.name}
      fromConference={view.fromConference}
      onBack={() =>
        setView({ kind: "standings", conference: view.fromConference })
      }
    />
  );
}

// ---- standings ----------------------------------------------------------

type StandingSortKey =
  | "program_name"
  | "conference"
  | "points"
  | "games_played"
  | "wins"
  | "losses"
  | "ties"
  | "goals_for"
  | "goals_against"
  | "goal_differential";

function StandingsPage({
  initialConference,
  onOpenRoster,
}: {
  initialConference: string;
  onOpenRoster: (slug: string, name: string, fromConference: string) => void;
}) {
  const [conferences, setConferences] = useState<string[]>([]);
  const [selected, setSelected] = useState<string>(initialConference); // "" = all
  const [standings, setStandings] = useState<Standing[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sort, toggleSort] = useSortSpec<StandingSortKey>();
  const [query, setQuery] = useState<string>("");

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
    const needle = query.trim().toLowerCase();
    const filtered = needle
      ? standings.filter((s) =>
          (s.program_name || s.program_slug).toLowerCase().includes(needle)
        )
      : standings;
    return applySort(filtered, sort, (row, key) => {
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
  }, [standings, sort, query]);

  return (
    <main className="container">
      <header className="page-header">
        <h1>Standings</h1>
        <div className="filter-group">
          <label className="filter">
            <input
              className="search"
              type="search"
              placeholder="Search teams…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </label>
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
        </div>
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
                label="PTS"
                sortKey="points"
                sort={sort}
                onToggle={toggleSort}
                align="right"
              />
              <SortHeader
                label="GP"
                sortKey="games_played"
                sort={sort}
                onToggle={toggleSort}
                align="right"
              />
              <SortHeader
                label="Record"
                sortKey="wins"
                sort={sort}
                onToggle={toggleSort}
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
                      onOpenRoster(
                        s.program_slug,
                        s.program_name || s.program_slug,
                        selected
                      )
                    }
                    title={`Roster for ${s.program_name || s.program_slug}`}
                  >
                    {s.program_name || s.program_slug}
                  </button>
                </td>
                <td title={fullNameFor(s.conference)}>{labelFor(s.conference)}</td>
                <td className="right">{s.points}</td>
                <td className="right">{s.games_played}</td>
                <td>{`${s.wins}-${s.losses}-${s.ties}`}</td>
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
  | "hometown"
  | "games_played"
  | "games_started"
  | "minutes"
  | "goals"
  | "assists";

function fmtStat(n: number | null): string {
  return n === null ? "—" : String(n);
}

function RosterPage({
  slug,
  name,
  fromConference,
  onBack,
}: {
  slug: string;
  name: string;
  fromConference: string;
  onBack: () => void;
}) {
  const [players, setPlayers] = useState<RosterEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sort, toggleSort] = useSortSpec<RosterSortKey>();

  const [games, setGames] = useState<Game[] | null>(null);
  const [scheduleError, setScheduleError] = useState<string | null>(null);

  useEffect(() => {
    invoke<RosterEntry[]>("list_roster", { slug })
      .then(setPlayers)
      .catch((e) => setError(String(e)));
    invoke<Game[]>("list_schedule", { slug })
      .then(setGames)
      .catch((e) => setScheduleError(String(e)));
  }, [slug]);

  const displayName = players?.[0]?.program_name || name;
  const conf = players?.[0]?.conference;
  const hasStats = !!players?.some((p) => p.games_played !== null);

  const sortedRows = useMemo(() => {
    if (!players) return players;
    return applySort(players, sort, (row, key) => {
      if (key === "jersey_number") {
        const n = parseInt(row.jersey_number, 10);
        return Number.isFinite(n) ? n : row.jersey_number;
      }
      // Numeric stat columns: nulls sort last regardless of direction.
      // Send Infinity so null > any real number, then invert if desc.
      const v = row[key];
      if (v === null) return sort?.dir === "desc" ? -Infinity : Infinity;
      return v as number | string;
    });
  }, [players, sort]);

  return (
    <main className="container">
      <header className="page-header">
        <div>
          <button className="linklike" onClick={onBack}>
            ← {fromConference ? `${labelFor(fromConference)} Standings` : "Standings"}
          </button>
          <h1>{displayName}</h1>
          <p className="subtitle">
            {conf || fromConference ? labelFor(conf || fromConference) : " "}
            {sortedRows
              ? ` · ${sortedRows.length} player${sortedRows.length === 1 ? "" : "s"}`
              : sortedRows === null && !error
                ? " · loading…"
                : ""}
            {sortedRows &&
              !hasStats &&
              sortedRows.length > 0 &&
              " · stats unavailable for this program"}
          </p>
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
              <SortHeader label="Name" sortKey="name" sort={sort} onToggle={toggleSort} />
              <SortHeader label="Pos" sortKey="position" sort={sort} onToggle={toggleSort} />
              <SortHeader
                label="Class"
                sortKey="class_year"
                sort={sort}
                onToggle={toggleSort}
              />
              <SortHeader label="Ht" sortKey="height" sort={sort} onToggle={toggleSort} />
              <SortHeader
                label="Hometown"
                sortKey="hometown"
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
                label="GS"
                sortKey="games_started"
                sort={sort}
                onToggle={toggleSort}
                align="right"
              />
              <SortHeader
                label="MIN"
                sortKey="minutes"
                sort={sort}
                onToggle={toggleSort}
                align="right"
              />
              <SortHeader
                label="G"
                sortKey="goals"
                sort={sort}
                onToggle={toggleSort}
                align="right"
              />
              <SortHeader
                label="A"
                sortKey="assists"
                sort={sort}
                onToggle={toggleSort}
                align="right"
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
                <td className="right">{fmtStat(p.games_played)}</td>
                <td className="right">{fmtStat(p.games_started)}</td>
                <td className="right">{fmtStat(p.minutes)}</td>
                <td className="right">{fmtStat(p.goals)}</td>
                <td className="right">{fmtStat(p.assists)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <h2 className="section-heading">Schedule</h2>
      {scheduleError && (
        <p className="error" role="alert">
          Error: {scheduleError}
        </p>
      )}
      {games === null && !scheduleError && <p>Loading schedule…</p>}
      {games !== null && games.length === 0 && !scheduleError && (
        <p>No schedule data for this program.</p>
      )}
      {games !== null && games.length > 0 && (
        <table className="schedule">
          <thead>
            <tr>
              <th>Date</th>
              <th>Home/Away</th>
              <th>Opponent</th>
              <th>Result</th>
            </tr>
          </thead>
          <tbody>
            {games.map((g, i) => (
              <tr key={`${g.date}-${g.opponent}-${i}`}>
                <td>{g.date}</td>
                <td>{g.home_away === "away" ? "at" : g.home_away === "neutral" ? "vs (n)" : "vs"}</td>
                <td>{g.opponent}</td>
                <td>
                  {g.result
                    ? `${g.result.outcome} ${g.result.team_score}-${g.result.opponent_score}`
                    : "—"}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </main>
  );
}

export default App;
