import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import html2canvas from "html2canvas";
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
  roster_url: string;
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

type Tab = "standings" | "leaders";

type View =
  | { kind: "standings"; conference: string; tab: Tab }
  | { kind: "roster"; slug: string; name: string; fromConference: string; fromTab: Tab };

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
      className={`sortable${align === "right" ? " right" : ""}${active ? " sort-active" : ""}`}
      onClick={() => onToggle(sortKey)}
      title="Click to sort"
    >
      <span className="th-label">{label}</span>
      <span className="sort-arrow" aria-hidden="true">
        {active ? (sort!.dir === "asc" ? "▲" : "▼") : "↕"}
      </span>
    </th>
  );
}

// ---- app ----------------------------------------------------------------

type Theme = "light" | "dark";

function initialTheme(): Theme {
  try {
    const stored = localStorage.getItem("theme");
    if (stored === "light" || stored === "dark") return stored;
  } catch {
    // ignore inaccessible localStorage
  }
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}

async function captureScreenshotToClipboard(): Promise<"ok" | "no-clipboard" | "denied" | "error"> {
  try {
    const canvas = await html2canvas(document.body, {
      backgroundColor:
        getComputedStyle(document.documentElement).getPropertyValue("--bg").trim() || "#ffffff",
      // Ignore the on-screen buttons so the screenshot isn't cluttered.
      ignoreElements: (el) =>
        el.classList?.contains("top-actions") || el.classList?.contains("toast"),
      scale: window.devicePixelRatio || 1,
    });
    const blob: Blob | null = await new Promise((resolve) =>
      canvas.toBlob(resolve, "image/png")
    );
    if (!blob) return "error";
    if (typeof ClipboardItem === "undefined" || !navigator.clipboard?.write) {
      return "no-clipboard";
    }
    await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
    return "ok";
  } catch (e) {
    if (String(e).includes("permission")) return "denied";
    console.error("screenshot failed:", e);
    return "error";
  }
}

function ScreenshotButton({ onDone }: { onDone: (status: string) => void }) {
  const [busy, setBusy] = useState(false);
  return (
    <button
      className="screenshot-btn"
      disabled={busy}
      onClick={async () => {
        setBusy(true);
        const result = await captureScreenshotToClipboard();
        setBusy(false);
        onDone(
          result === "ok"
            ? "Screenshot copied to clipboard"
            : result === "no-clipboard"
              ? "Clipboard API unavailable in this webview"
              : result === "denied"
                ? "Clipboard permission denied"
                : "Screenshot failed"
        );
      }}
      title="Copy screenshot to clipboard (⌘/Ctrl+Shift+C)"
      aria-label="Copy screenshot to clipboard"
    >
      📸
    </button>
  );
}

function ThemeToggle({
  theme,
  onToggle,
}: {
  theme: Theme;
  onToggle: () => void;
}) {
  return (
    <button
      className="theme-toggle"
      onClick={onToggle}
      title={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"}
      aria-label="Toggle color theme"
    >
      {theme === "dark" ? "☀️" : "🌙"}
    </button>
  );
}

function App() {
  const [theme, setTheme] = useState<Theme>(initialTheme);
  useEffect(() => {
    document.documentElement.setAttribute("data-theme", theme);
    try {
      localStorage.setItem("theme", theme);
    } catch {
      // ignore
    }
  }, [theme]);
  const toggleTheme = () =>
    setTheme((t) => (t === "dark" ? "light" : "dark"));

  const [toast, setToast] = useState<string | null>(null);
  const showToast = useCallback((msg: string) => {
    setToast(msg);
    window.setTimeout(() => setToast(null), 2500);
  }, []);

  // ⌘/Ctrl + Shift + C — copy a screenshot of the whole app.
  useEffect(() => {
    const onKey = async (e: KeyboardEvent) => {
      if (
        (e.metaKey || e.ctrlKey) &&
        e.shiftKey &&
        (e.key === "C" || e.key === "c")
      ) {
        e.preventDefault();
        const result = await captureScreenshotToClipboard();
        showToast(
          result === "ok"
            ? "Screenshot copied to clipboard"
            : result === "no-clipboard"
              ? "Clipboard API unavailable in this webview"
              : result === "denied"
                ? "Clipboard permission denied"
                : "Screenshot failed"
        );
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [showToast]);

  const [view, setView] = useState<View>({
    kind: "standings",
    conference: "",
    tab: "standings",
  });
  return (
    <>
      <div className="top-actions">
        <ScreenshotButton onDone={showToast} />
        <ThemeToggle theme={theme} onToggle={toggleTheme} />
      </div>
      {toast && (
        <div className="toast" role="status" aria-live="polite">
          {toast}
        </div>
      )}
      {view.kind === "standings" ? (
        <BrowsePage
          initialConference={view.conference}
          initialTab={view.tab}
          onOpenRoster={(slug, name, fromConference, fromTab) =>
            setView({ kind: "roster", slug, name, fromConference, fromTab })
          }
        />
      ) : (
        <RosterPage
          slug={view.slug}
          name={view.name}
          fromConference={view.fromConference}
          fromTab={view.fromTab}
          onBack={() =>
            setView({
              kind: "standings",
              conference: view.fromConference,
              tab: view.fromTab,
            })
          }
        />
      )}
    </>
  );
}

// ---- browse (standings + leaders tabs) ---------------------------------

function BrowsePage({
  initialConference,
  initialTab,
  onOpenRoster,
}: {
  initialConference: string;
  initialTab: Tab;
  onOpenRoster: (
    slug: string,
    name: string,
    fromConference: string,
    fromTab: Tab
  ) => void;
}) {
  const [conferences, setConferences] = useState<string[]>([]);
  const [selected, setSelected] = useState<string>(initialConference);
  const [tab, setTab] = useState<Tab>(initialTab);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<string[]>("list_conferences")
      .then(setConferences)
      .catch((e) => setError(String(e)));
  }, []);

  return (
    <main className="container">
      <header className="page-header">
        <div className="tabs" role="tablist">
          <button
            role="tab"
            aria-selected={tab === "standings"}
            className={`tab ${tab === "standings" ? "tab-active" : ""}`}
            onClick={() => setTab("standings")}
          >
            Standings
          </button>
          <button
            role="tab"
            aria-selected={tab === "leaders"}
            className={`tab ${tab === "leaders" ? "tab-active" : ""}`}
            onClick={() => setTab("leaders")}
          >
            Leaders
          </button>
        </div>
        <label className="filter" title="Conference filter">
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
      {tab === "standings" ? (
        <StandingsPage
          conference={selected}
          onOpenRoster={(slug, name) => onOpenRoster(slug, name, selected, tab)}
        />
      ) : (
        <LeadersPage
          conference={selected}
          onOpenRoster={(slug, name) => onOpenRoster(slug, name, selected, tab)}
        />
      )}
    </main>
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
  conference,
  onOpenRoster,
}: {
  conference: string;
  onOpenRoster: (slug: string, name: string) => void;
}) {
  const [standings, setStandings] = useState<Standing[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sort, toggleSort] = useSortSpec<StandingSortKey>();
  const [query, setQuery] = useState<string>("");

  useEffect(() => {
    setStandings(null);
    setError(null);
    invoke<Standing[]>("list_standings", { conference: conference || null })
      .then(setStandings)
      .catch((e) => setError(String(e)));
  }, [conference]);

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
    <>
      <div className="toolbar">
        <input
          className="search"
          type="search"
          placeholder="Search teams…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      </div>
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
              />
              <SortHeader
                label="GP"
                sortKey="games_played"
                sort={sort}
                onToggle={toggleSort}
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
              />
              <SortHeader
                label="GA"
                sortKey="goals_against"
                sort={sort}
                onToggle={toggleSort}
              />
              <SortHeader
                label="GD"
                sortKey="goal_differential"
                sort={sort}
                onToggle={toggleSort}
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
                <td title={fullNameFor(s.conference)}>{labelFor(s.conference)}</td>
                <td>{s.points}</td>
                <td>{s.games_played}</td>
                <td>{`${s.wins}-${s.losses}-${s.ties}`}</td>
                <td>{s.goals_for}</td>
                <td>{s.goals_against}</td>
                <td>{s.goals_for - s.goals_against}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </>
  );
}

// ---- leaders ------------------------------------------------------------

type LeaderCategory =
  | "goals"
  | "assists"
  | "points"
  | "minutes"
  | "games_played"
  | "games_started";

type LeaderRow = {
  program_slug: string;
  program_name: string;
  conference: string;
  name: string;
  jersey_number: string;
  games_played: number;
  games_started: number;
  minutes: number;
  goals: number;
  assists: number;
};

const LEADER_CATEGORIES: { key: LeaderCategory; label: string }[] = [
  { key: "goals", label: "Goals" },
  { key: "assists", label: "Assists" },
  { key: "points", label: "Points (G×2 + A)" },
  { key: "minutes", label: "Minutes" },
  { key: "games_played", label: "Games Played" },
  { key: "games_started", label: "Games Started" },
];

function pickValue(row: LeaderRow, cat: LeaderCategory): number {
  switch (cat) {
    case "goals":
      return row.goals;
    case "assists":
      return row.assists;
    case "points":
      return row.goals * 2 + row.assists;
    case "minutes":
      return row.minutes;
    case "games_played":
      return row.games_played;
    case "games_started":
      return row.games_started;
  }
}

function LeadersPage({
  conference,
  onOpenRoster,
}: {
  conference: string;
  onOpenRoster: (slug: string, name: string) => void;
}) {
  const [category, setCategory] = useState<LeaderCategory>("goals");
  const [rows, setRows] = useState<LeaderRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setRows(null);
    setError(null);
    invoke<LeaderRow[]>("list_leaders", {
      category,
      conference: conference || null,
      limit: 25,
    })
      .then(setRows)
      .catch((e) => setError(String(e)));
  }, [category, conference]);

  return (
    <>
      <div className="toolbar">
        <label className="filter">
          Category:{" "}
          <select
            value={category}
            onChange={(e) => setCategory(e.target.value as LeaderCategory)}
          >
            {LEADER_CATEGORIES.map((c) => (
              <option key={c.key} value={c.key}>
                {c.label}
              </option>
            ))}
          </select>
        </label>
      </div>
      {error && (
        <p className="error" role="alert">
          Error: {error}
        </p>
      )}
      {rows === null && !error && <p>Loading…</p>}
      {rows !== null && rows.length === 0 && !error && (
        <p>No stats available for this selection.</p>
      )}
      {rows !== null && rows.length > 0 && (
        <table className="leaders">
          <thead>
            <tr>
              <th>Rank</th>
              <th>Player</th>
              <th>Team</th>
              <th>Conf</th>
              <th>
                {LEADER_CATEGORIES.find((c) => c.key === category)?.label}
              </th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r, i) => (
              <tr key={`${r.program_slug}-${r.jersey_number}-${r.name}`}>
                <td>{i + 1}</td>
                <td>
                  #{r.jersey_number} {r.name}
                </td>
                <td>
                  <button
                    className="linklike"
                    onClick={() => onOpenRoster(r.program_slug, r.program_name || r.program_slug)}
                  >
                    {r.program_name || r.program_slug}
                  </button>
                </td>
                <td title={fullNameFor(r.conference)}>{labelFor(r.conference)}</td>
                <td>{pickValue(r, category)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </>
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
  fromTab,
  onBack,
}: {
  slug: string;
  name: string;
  fromConference: string;
  fromTab: Tab;
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
            ← {fromConference ? `${labelFor(fromConference)} ` : ""}
            {fromTab === "leaders" ? "Leaders" : "Standings"}
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
            {players?.[0]?.roster_url && (
              <>
                {" · "}
                <a
                  className="external-link"
                  href={players[0].roster_url}
                  target="_blank"
                  rel="noopener noreferrer"
                  title="Open the program's official athletics page"
                >
                  Official site ↗
                </a>
              </>
            )}
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
              />
              <SortHeader
                label="GS"
                sortKey="games_started"
                sort={sort}
                onToggle={toggleSort}
              />
              <SortHeader
                label="MIN"
                sortKey="minutes"
                sort={sort}
                onToggle={toggleSort}
              />
              <SortHeader
                label="G"
                sortKey="goals"
                sort={sort}
                onToggle={toggleSort}
              />
              <SortHeader
                label="A"
                sortKey="assists"
                sort={sort}
                onToggle={toggleSort}
              />
            </tr>
          </thead>
          <tbody>
            {sortedRows.map((p, i) => (
              <tr key={`${p.jersey_number}-${p.name}-${i}`}>
                <td>{p.jersey_number}</td>
                <td>{p.name}</td>
                <td>{p.position}</td>
                <td>{p.class_year}</td>
                <td>{p.height}</td>
                <td>{p.hometown}</td>
                <td>{fmtStat(p.games_played)}</td>
                <td>{fmtStat(p.games_started)}</td>
                <td>{fmtStat(p.minutes)}</td>
                <td>{fmtStat(p.goals)}</td>
                <td>{fmtStat(p.assists)}</td>
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
