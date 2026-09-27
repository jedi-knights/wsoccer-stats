import type React from "react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
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
  rpi_rank: number;
  // Classical NCAA RPI in [0, 1] and its strength-of-schedule component
  // (opponent winning percentage with games vs this team excluded).
  // Populated only when the team has played games.
  rpi: number;
  sos: number;
  // False for zero-record stubs filled from the registry to represent a
  // conference member whose schedule wasn't ingested — the numeric
  // columns for such rows render as em-dashes so users can distinguish
  // "no data" from "played 0 games in this view".
  has_schedule_data: boolean;
};

type ConferenceEntry = {
  slug: string;
  label: string;
  full_name: string;
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

type H2HGame = {
  program_slug: string;
  opponent_slug: string;
  date: string;
  home_away: string;
  outcome: "W" | "L" | "T" | string;
  team_score: number;
  opponent_score: number;
};

type RefreshResult = {
  kind: string;
  conference: string;
  ok: boolean;
  summary: string;
};

type RefreshEvent =
  | { phase: "started"; total: number }
  | { phase: "step"; index: number; total: number; result: RefreshResult }
  | { phase: "finished"; ok: number; total: number };

type Tab = "conferences" | "standings" | "leaders" | "today";

type UpcomingMatch = {
  program_slug: string;
  program_name: string;
  conference: string;
  opponent_slug: string | null;
  opponent_name: string;
  opponent_conference: string;
  date: string;
  home_away: string;
  kickoff_time: string | null;
  broadcast_channel: string | null;
  broadcast_url: string | null;
  result: GameResult | null;
};

type ConferenceSummary = {
  conference: string;
  label: string;
  full_name: string;
  team_count: number;
  ranked_count: number;
  avg_rpi_rank: number | null;
};

type View =
  | { kind: "standings"; conference: string; tab: Tab }
  | { kind: "roster"; slug: string; name: string; fromConference: string; fromTab: Tab };

// Module-level toast emitter — App registers its `showToast` callback
// at mount so deep components (per-chart screenshot buttons) can pop a
// toast without prop-drilling.
let TOAST_EMITTER: ((msg: string) => void) | null = null;

function emitToast(msg: string): void {
  if (TOAST_EMITTER) TOAST_EMITTER(msg);
}

// Conference labels come from the backend (sourced from the aip
// registry's ``conferences.ndjson``). This module-level map is
// populated once at app startup so all callers see the same labels
// without threading them through props.
let CONFERENCE_LABEL_MAP: Record<string, { label: string; full_name: string }> = {};

function setConferenceLabels(entries: ConferenceEntry[]): void {
  const next: Record<string, { label: string; full_name: string }> = {};
  for (const e of entries) {
    next[e.slug] = { label: e.label, full_name: e.full_name };
  }
  CONFERENCE_LABEL_MAP = next;
}

function labelFor(conf: string): string {
  const hit = CONFERENCE_LABEL_MAP[conf];
  if (hit) return hit.label;
  // Fallback for a slug we haven't been told about (e.g., data older
  // than the conferences.ndjson change): title-case the underscores.
  return conf
    .split("_")
    .map((w) => (w.length === 0 ? w : w[0].toUpperCase() + w.slice(1)))
    .join(" ");
}

function fullNameFor(conf: string): string {
  const hit = CONFERENCE_LABEL_MAP[conf];
  return hit && hit.full_name ? hit.full_name : labelFor(conf);
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

async function captureScreenshotToClipboard(
  target?: HTMLElement | null,
): Promise<"ok" | "no-clipboard" | "denied" | "error"> {
  try {
    const el = target ?? document.body;
    const canvas = await html2canvas(el, {
      backgroundColor:
        getComputedStyle(document.documentElement)
          .getPropertyValue("--bg")
          .trim() || "#ffffff",
      // Ignore the on-screen chrome (global buttons, toasts, per-chart
      // copy buttons) so the screenshot is just the content.
      ignoreElements: (node) =>
        node.classList?.contains("top-actions") ||
        node.classList?.contains("toast") ||
        node.classList?.contains("chart-copy-btn"),
      scale: window.devicePixelRatio || 1,
      // For a scrollable target (h2h wrapper) capture the full un-clipped
      // content, not just the visible portion.
      width: Math.max(el.scrollWidth, el.clientWidth),
      height: Math.max(el.scrollHeight, el.clientHeight),
      windowWidth: Math.max(el.scrollWidth, document.documentElement.clientWidth),
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
    // Log unconditionally — the string return values are coarse
    // (`denied` / `error`), so the console message is where debugging
    // actually happens.
    console.error("screenshot failed:", e);
    if (String(e).includes("permission")) return "denied";
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

function RefreshButton({
  busy,
  onClick,
}: {
  busy: boolean;
  onClick: () => void;
}) {
  return (
    <button
      className="refresh-btn"
      disabled={busy}
      onClick={onClick}
      title="Refresh from live sites"
      aria-label="Refresh data from live sites"
    >
      <span className={busy ? "refresh-icon spinning" : "refresh-icon"}>
        {busy ? "⏳" : "🔄"}
      </span>
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
  useEffect(() => {
    TOAST_EMITTER = showToast;
    return () => {
      TOAST_EMITTER = null;
    };
  }, [showToast]);

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
    tab: "conferences",
  });
  const [dataRevision, setDataRevision] = useState(0);
  const [refreshing, setRefreshing] = useState(false);
  const [progress, setProgress] = useState<{
    done: number;
    total: number;
    label: string;
  } | null>(null);
  const refresh = useCallback(async () => {
    if (refreshing) return;
    setRefreshing(true);
    setProgress({ done: 0, total: 0, label: "Starting…" });
    try {
      const channel = new Channel<RefreshEvent>();
      channel.onmessage = (msg) => {
        if (msg.phase === "started") {
          setProgress({ done: 0, total: msg.total, label: "Starting…" });
        } else if (msg.phase === "step") {
          setProgress({
            done: msg.index,
            total: msg.total,
            label: `${msg.result.conference} · ${msg.result.kind}${
              msg.result.ok ? "" : " (failed)"
            }`,
          });
        } else if (msg.phase === "finished") {
          setProgress({
            done: msg.total,
            total: msg.total,
            label: `Refreshed ${msg.ok}/${msg.total} tasks`,
          });
        }
      };
      const results = await invoke<
        { kind: string; conference: string; ok: boolean; summary: string }[]
      >("refresh_data", {
        conference:
          view.kind === "standings"
            ? view.conference || null
            : view.fromConference || null,
        onProgress: channel,
      });
      const ok = results.filter((r) => r.ok).length;
      showToast(`Refreshed ${ok}/${results.length} tasks`);
      setDataRevision((n) => n + 1);
    } catch (e) {
      showToast(`Refresh failed: ${e}`);
    } finally {
      setRefreshing(false);
      window.setTimeout(() => setProgress(null), 1200);
    }
  }, [refreshing, showToast, view]);
  return (
    <>
      <div className="top-actions">
        <RefreshButton busy={refreshing} onClick={refresh} />
        <ScreenshotButton onDone={showToast} />
        <ThemeToggle theme={theme} onToggle={toggleTheme} />
      </div>
      {progress && (
        <div className="progress-banner" role="status" aria-live="polite">
          <div className="progress-label">
            <span>Refreshing…</span>
            <span>
              {progress.total > 0
                ? `${progress.done}/${progress.total} · ${progress.label}`
                : progress.label}
            </span>
          </div>
          <div className="progress-track">
            <div
              className="progress-fill"
              style={{
                width:
                  progress.total > 0
                    ? `${(progress.done / progress.total) * 100}%`
                    : "0%",
              }}
            />
          </div>
        </div>
      )}
      {toast && (
        <div className="toast" role="status" aria-live="polite">
          {toast}
        </div>
      )}
      {view.kind === "standings" ? (
        <BrowsePage
          key={`browse-${dataRevision}`}
          initialConference={view.conference}
          initialTab={view.tab}
          onOpenRoster={(slug, name, fromConference, fromTab) =>
            setView({ kind: "roster", slug, name, fromConference, fromTab })
          }
        />
      ) : (
        <RosterPage
          key={`roster-${view.slug}-${dataRevision}`}
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
  const [conferences, setConferences] = useState<ConferenceEntry[]>([]);
  const [selected, setSelected] = useState<string>(initialConference);
  const [tab, setTab] = useState<Tab>(initialTab);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<ConferenceEntry[]>("list_conferences")
      .then((entries) => {
        // Feed the module-level label map before any child component
        // renders — labelFor/fullNameFor read from it synchronously.
        setConferenceLabels(entries);
        setConferences(entries);
      })
      .catch((e) => setError(String(e)));
  }, []);

  return (
    <main className="container">
      <header className="page-header">
        <div className="tabs" role="tablist">
          <button
            role="tab"
            aria-selected={tab === "conferences"}
            className={`tab ${tab === "conferences" ? "tab-active" : ""}`}
            onClick={() => setTab("conferences")}
          >
            Conferences
          </button>
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
          <TodayTabButton
            active={tab === "today"}
            onClick={() => setTab("today")}
          />
        </div>
        {tab !== "conferences" && tab !== "today" && (
          <label className="filter" title="Conference filter">
            Conference:{" "}
            <select
              value={selected}
              onChange={(e) => setSelected(e.target.value)}
            >
              <option value="">All ({conferences.length})</option>
              {conferences.map((c) => (
                <option key={c.slug} value={c.slug}>
                  {c.label}
                </option>
              ))}
            </select>
          </label>
        )}
      </header>
      {error && (
        <p className="error" role="alert">
          Error: {error}
        </p>
      )}
      {tab === "conferences" ? (
        <ConferencesPage
          onOpenConference={(conf) => {
            setSelected(conf);
            setTab("standings");
          }}
        />
      ) : tab === "today" ? (
        <TodayPage
          onOpenRoster={(slug, name) => onOpenRoster(slug, name, selected, tab)}
        />
      ) : tab === "standings" ? (
        <StandingsPage
          conference={selected}
          onOpenRoster={(slug, name) => onOpenRoster(slug, name, selected, tab)}
          onSelectConference={setSelected}
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

// ---- today --------------------------------------------------------------

/// Local-tz `YYYY-MM-DD`. The Swedish locale happens to produce the
/// ISO format natively, which saves us doing tz math in the frontend
/// or shipping a date library to the backend just for one string.
function localTodayIso(d: Date = new Date()): string {
  return d.toLocaleDateString("sv-SE");
}

/// Return a `Date` that refreshes every `pollMs` — components that
/// depend on it (the live-match badge, the LIVE row tint) re-render
/// as the clock advances, so at 5 AM a 7 PM kickoff correctly reads
/// as not-live and flips at kickoff without a page reload.
function useNow(pollMs: number): Date {
  const [now, setNow] = useState<Date>(() => new Date());
  useEffect(() => {
    const id = window.setInterval(() => setNow(new Date()), pollMs);
    return () => window.clearInterval(id);
  }, [pollMs]);
  return now;
}

/// Parse a kickoff-time string like "7 p.m." / "7:30 PM" / "1 PM ET"
/// into minutes-since-local-midnight. Returns null when unparseable
/// (e.g. "TBA", "TBD"). Ignores any trailing timezone label — we
/// assume the source publishes local kickoff, which is the standard
/// convention.
function parseKickoffMinutes(s: string): number | null {
  const m = s.match(/^\s*(\d{1,2})(?::(\d{2}))?\s*(a\.?m\.?|p\.?m\.?)/i);
  if (!m) return null;
  let hour = parseInt(m[1], 10);
  const minute = m[2] ? parseInt(m[2], 10) : 0;
  const isPm = m[3][0].toLowerCase() === "p";
  if (isPm && hour !== 12) hour += 12;
  if (!isPm && hour === 12) hour = 0;
  if (hour < 0 || hour > 23 || minute < 0 || minute > 59) return null;
  return hour * 60 + minute;
}

/// Match window: kickoff → kickoff + 150 min (regulation 90 + halftime
/// + stoppage + a bit of buffer for late-arriving results). Games with
/// no kickoff time are never considered live to avoid the "everything
/// scheduled today looks live at 5 AM" false positive.
const MATCH_WINDOW_MINUTES = 150;

function isMatchLive(m: UpcomingMatch, now: Date): boolean {
  if (m.result) return false;
  if (m.date !== localTodayIso(now)) return false;
  if (!m.kickoff_time) return false;
  const kickoff = parseKickoffMinutes(m.kickoff_time);
  if (kickoff === null) return false;
  const nowMinutes = now.getHours() * 60 + now.getMinutes();
  return nowMinutes >= kickoff && nowMinutes <= kickoff + MATCH_WINDOW_MINUTES;
}

/// Poll the backend for today+tomorrow's matches. The `now` returned
/// here refreshes every `pollMs` so callers can pass it into
/// `isMatchLive` and get a fresh evaluation on every tick, even
/// between backend refetches (which happen at the same cadence).
function useUpcomingMatches(pollMs: number): {
  matches: UpcomingMatch[] | null;
  now: Date;
  today: string;
  refresh: () => void;
} {
  const [matches, setMatches] = useState<UpcomingMatch[] | null>(null);
  const now = useNow(pollMs);
  const today = localTodayIso(now);
  const [tick, setTick] = useState(0);
  const refresh = useCallback(() => setTick((n) => n + 1), []);

  useEffect(() => {
    invoke<UpcomingMatch[]>("list_todays_matches", { today: localTodayIso() })
      .then(setMatches)
      .catch(() => setMatches([]));
  }, [tick]);

  useEffect(() => {
    const id = window.setInterval(refresh, pollMs);
    return () => window.clearInterval(id);
  }, [pollMs, refresh]);

  return { matches, now, today, refresh };
}

function TodayTabButton({
  active,
  onClick,
}: {
  active: boolean;
  onClick: () => void;
}) {
  // Poll every 60 s while the user is on other tabs — cheap enough for
  // a background timer, fast enough that a kickoff is picked up within
  // a minute of the match starting.
  const { matches, now } = useUpcomingMatches(60_000);
  const liveCount = matches
    ? matches.filter((m) => isMatchLive(m, now)).length
    : 0;
  return (
    <button
      role="tab"
      aria-selected={active}
      className={`tab ${active ? "tab-active" : ""}`}
      onClick={onClick}
    >
      Today
      {liveCount > 0 && (
        <span
          className="live-dot"
          title={`${liveCount} match${liveCount === 1 ? "" : "es"} in progress`}
          aria-label={`${liveCount} live match${liveCount === 1 ? "" : "es"}`}
        />
      )}
    </button>
  );
}

function TodayPage({
  onOpenRoster,
}: {
  onOpenRoster: (slug: string, name: string) => void;
}) {
  const { matches, today, now } = useUpcomingMatches(30_000);
  const tomorrow = useMemo(() => {
    const d = new Date();
    d.setDate(d.getDate() + 1);
    return d.toLocaleDateString("sv-SE");
  }, []);

  // Deduplicate two-known-team matchups: prefer the home team's row so
  // each unique game appears once.
  const deduped = useMemo(() => {
    if (!matches) return matches;
    // Key by sorted pair of slugs when both known. Prefer the row
    // where the OWNER is the home team; if neither is home (both away
    // shouldn't happen), keep the first encountered.
    const byPair = new Map<string, UpcomingMatch>();
    const singles: UpcomingMatch[] = [];
    for (const m of matches) {
      if (!m.opponent_slug) {
        singles.push(m);
        continue;
      }
      const key = [m.program_slug, m.opponent_slug].sort().join("|") + "|" + m.date;
      const existing = byPair.get(key);
      const isHome = m.home_away === "home";
      if (!existing || (isHome && existing.home_away !== "home")) {
        byPair.set(key, m);
      }
    }
    return [...byPair.values(), ...singles];
  }, [matches]);

  if (matches === null) return <main className="container"><p>Loading…</p></main>;
  if (!deduped || deduped.length === 0) {
    return (
      <main className="container">
        <h2 className="section-heading">Today &amp; Tomorrow</h2>
        <p>No games scheduled between {today} and {tomorrow}.</p>
      </main>
    );
  }

  const dates = Array.from(new Set(deduped.map((m) => m.date))).sort();
  const label = (d: string) => (d === today ? "Today" : d === tomorrow ? "Tomorrow" : d);

  return (
    <main className="container">
      {dates.map((d) => (
        <section key={d} className="today-section">
          <h2 className="section-heading">
            {label(d)}
            <span className="today-date-suffix"> · {d}</span>
          </h2>
          <table className="today-matches">
            <thead>
              <tr>
                <th>Kickoff</th>
                <th>Matchup</th>
                <th>Conf</th>
                <th>Broadcast</th>
                <th>Status</th>
              </tr>
            </thead>
            <tbody>
              {deduped
                .filter((m) => m.date === d)
                .map((m) => {
                  const live = isMatchLive(m, now);
                  const venue =
                    m.home_away === "away"
                      ? "@"
                      : m.home_away === "neutral"
                        ? "vs (n)"
                        : "vs";
                  const status = m.result
                    ? `${m.result.outcome} ${m.result.team_score}-${m.result.opponent_score}`
                    : live
                      ? "LIVE"
                      : m.kickoff_time || "TBD";
                  return (
                    <tr
                      key={`${m.program_slug}-${m.opponent_slug ?? m.opponent_name}-${m.date}`}
                      className={live ? "today-row-live" : undefined}
                    >
                      <td>{m.kickoff_time || "—"}</td>
                      <td>
                        <button
                          className="linklike"
                          onClick={() =>
                            onOpenRoster(
                              m.program_slug,
                              m.program_name || m.program_slug,
                            )
                          }
                        >
                          {m.program_name || m.program_slug}
                        </button>
                        {" "}
                        <span className="today-venue">{venue}</span>{" "}
                        {m.opponent_slug ? (
                          <button
                            className="linklike"
                            onClick={() =>
                              onOpenRoster(m.opponent_slug!, m.opponent_name)
                            }
                          >
                            {m.opponent_name}
                          </button>
                        ) : (
                          <span>{m.opponent_name}</span>
                        )}
                      </td>
                      <td>
                        {m.conference && labelFor(m.conference)}
                        {m.opponent_conference &&
                          m.opponent_conference !== m.conference &&
                          ` / ${labelFor(m.opponent_conference)}`}
                      </td>
                      <td>
                        {m.broadcast_url ? (
                          <a
                            className="external-link"
                            href={m.broadcast_url}
                            target="_blank"
                            rel="noopener noreferrer"
                          >
                            {m.broadcast_channel || "Watch"} ↗
                          </a>
                        ) : (
                          m.broadcast_channel || "—"
                        )}
                      </td>
                      <td>
                        {live ? (
                          <span className="live-badge">● LIVE</span>
                        ) : (
                          status
                        )}
                      </td>
                    </tr>
                  );
                })}
            </tbody>
          </table>
        </section>
      ))}
    </main>
  );
}

// ---- conferences --------------------------------------------------------

type ConferenceSortKey = "conference" | "team_count" | "avg_rpi_rank";

function ConferencesPage({
  onOpenConference,
}: {
  onOpenConference: (conference: string) => void;
}) {
  const [rows, setRows] = useState<ConferenceSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sort, toggleSort] = useSortSpec<ConferenceSortKey>();

  useEffect(() => {
    invoke<ConferenceSummary[]>("list_conference_summary")
      .then((entries) => {
        // Seed the module-level label map from this response too so any
        // sibling component that renders BEFORE list_conferences resolves
        // still gets correct acronyms.
        setConferenceLabels(
          entries.map((e) => ({
            slug: e.conference,
            label: e.label,
            full_name: e.full_name,
          })),
        );
        setRows(entries);
      })
      .catch((e) => setError(String(e)));
  }, []);

  const sortedRows = useMemo(() => {
    if (!rows) return rows;
    return applySort(rows, sort, (row, key) => {
      switch (key) {
        case "conference":
          return row.label;
        case "avg_rpi_rank":
          // Unranked conferences sort last regardless of direction.
          if (row.avg_rpi_rank === null) {
            return sort?.dir === "desc" ? -Infinity : Infinity;
          }
          return row.avg_rpi_rank;
        default:
          return row[key];
      }
    });
  }, [rows, sort]);

  if (error) {
    return (
      <p className="error" role="alert">
        Error: {error}
      </p>
    );
  }
  if (sortedRows === null) return <p>Loading…</p>;
  if (sortedRows.length === 0) return <p>No conference data available.</p>;

  return (
    <table className="standings">
      <thead>
        <tr>
          <SortHeader
            label="Conference"
            sortKey="conference"
            sort={sort}
            onToggle={toggleSort}
          />
          <SortHeader
            label="Teams"
            sortKey="team_count"
            sort={sort}
            onToggle={toggleSort}
          />
          <SortHeader
            label="Avg RPI Rank"
            sortKey="avg_rpi_rank"
            sort={sort}
            onToggle={toggleSort}
          />
        </tr>
      </thead>
      <tbody>
        {sortedRows.map((r) => (
          <tr key={r.conference}>
            <td>
              <button
                className="linklike"
                onClick={() => onOpenConference(r.conference)}
                title={`Open ${r.full_name || r.label} standings`}
              >
                {r.label}
              </button>
            </td>
            <td>{r.team_count}</td>
            <td
              title={
                r.avg_rpi_rank === null
                  ? "No RPI-ranked teams yet"
                  : `Mean rank across ${r.ranked_count} ranked team${r.ranked_count === 1 ? "" : "s"}`
              }
            >
              {r.avg_rpi_rank === null ? "—" : r.avg_rpi_rank.toFixed(1)}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

// ---- standings ----------------------------------------------------------

type StandingSortKey =
  | "program_name"
  | "conference"
  | "rpi_rank"
  | "points"
  | "games_played"
  | "wins"
  | "losses"
  | "ties"
  | "goals_for"
  | "goals_against"
  | "goal_differential";

type StandingsMode = "all" | "conference" | "non_conference";

const STANDINGS_MODES: { key: StandingsMode; label: string }[] = [
  { key: "all", label: "Overall" },
  { key: "conference", label: "Conference" },
  { key: "non_conference", label: "Non-conference" },
];

/// Small clipboard-copy button rendered in the top-right of a chart
/// frame. Takes a ref to the element to capture (usually the frame's
/// outer div; for scrollable content pass the inner un-clipped node).
/// Toasts on success/failure via the module-level emitter.
function ChartCopyButton({ target }: { target: React.RefObject<HTMLElement> }) {
  const [busy, setBusy] = useState(false);
  return (
    <button
      className="chart-copy-btn"
      disabled={busy}
      title="Copy this chart to clipboard"
      aria-label="Copy chart to clipboard"
      onClick={async () => {
        setBusy(true);
        const result = await captureScreenshotToClipboard(target.current);
        setBusy(false);
        emitToast(
          result === "ok"
            ? "Chart copied to clipboard"
            : result === "no-clipboard"
              ? "Clipboard API unavailable in this webview"
              : result === "denied"
                ? "Clipboard permission denied"
                : "Chart copy failed",
        );
      }}
    >
      {busy ? "⏳" : "📸"}
    </button>
  );
}

/// Shared frame + title + copy-button chrome for the standings charts.
/// The copy button captures whatever ref is passed (defaults to the
/// frame's outer div). Children render below the title bar.
function ChartFrame({
  title,
  captureRef,
  children,
}: {
  title: string;
  captureRef?: React.RefObject<HTMLElement>;
  children: React.ReactNode;
}) {
  const localRef = useRef<HTMLDivElement>(null);
  const target = captureRef ?? (localRef as React.RefObject<HTMLElement>);
  return (
    <div className="chart-block" ref={localRef}>
      <div className="chart-header">
        <div className="chart-title">{title}</div>
        <ChartCopyButton target={target} />
      </div>
      {children}
    </div>
  );
}

/// Decide which side of a dot to place its label so the text stays
/// inside the plot bounds. Returns:
/// - `dx`: +1 for right-of-dot, -1 for left-of-dot (caller multiplies
///   by `r + gap` to get the actual x offset)
/// - `dy`: vertical offset added to the dot's cy (positive = down)
/// - `textAnchor`: matching SVG `text-anchor` value so the text
///   grows away from the dot in the right direction
///
/// Right-side flip when the dot sits in the right 35% of the plot;
/// vertical offset shifts labels downward when the dot is at the very
/// top so the ascender doesn't get clipped by the plot top border.
function labelAnchorFor(
  cx: number,
  cy: number,
  pad: { top: number; left: number; right: number; bottom: number },
  plotW: number,
  plotH: number,
): { dx: 1 | -1; dy: number; textAnchor: "start" | "end" } {
  const rightThreshold = pad.left + plotW * 0.65;
  const topThreshold = pad.top + plotH * 0.1;
  const flipLeft = cx >= rightThreshold;
  const belowDot = cy <= topThreshold;
  return {
    dx: flipLeft ? -1 : 1,
    dy: belowDot ? 14 : 4,
    textAnchor: flipLeft ? "end" : "start",
  };
}

/// GF-vs-GA scatter plot, one dot per team that has played at least one
/// game. GF grows to the right, GA grows DOWN — so the top-right corner
/// is high-scoring + defensively solid (best), bottom-left is
/// low-scoring + leaky (worst). A dashed diagonal marks GD = 0 so
/// teams above the line have positive GD and teams below have negative.
/// Clicking a dot opens the roster for that team.
function GoalsScatter({
  rows,
  onOpenRoster,
  selectedSlugs,
}: {
  rows: Standing[];
  onOpenRoster: (slug: string, name: string) => void;
  selectedSlugs: Set<string>;
}) {
  // Skip stubs and unplayed teams — they'd all cluster at the origin.
  const points = rows.filter(
    (r) => r.has_schedule_data && r.games_played > 0,
  );
  if (points.length < 2) return null;

  // Width scales via viewBox — the container is styled to match the
  // standings table's width so the chart lines up visually with it.
  const width = 960;
  const height = 320;
  const pad = { top: 16, right: 24, bottom: 40, left: 44 };
  const plotW = width - pad.left - pad.right;
  const plotH = height - pad.top - pad.bottom;

  const maxGF = Math.max(5, ...points.map((p) => p.goals_for));
  const maxGA = Math.max(5, ...points.map((p) => p.goals_against));
  const axisMax = Math.ceil(Math.max(maxGF, maxGA) / 5) * 5;

  const xFor = (gf: number) => pad.left + (gf / axisMax) * plotW;
  const yFor = (ga: number) => pad.top + (ga / axisMax) * plotH;

  const ticks: number[] = [];
  const step = axisMax >= 30 ? 10 : 5;
  for (let v = 0; v <= axisMax; v += step) ticks.push(v);

  return (
    <ChartFrame title="Goals For vs. Goals Against">
      <svg
        width="100%"
        viewBox={`0 0 ${width} ${height}`}
        preserveAspectRatio="xMidYMid meet"
        role="img"
        aria-label="Goals for vs. goals against"
      >
        {/* Diagonal reference line at GF = GA (goal differential zero). */}
        <line
          x1={xFor(0)}
          y1={yFor(0)}
          x2={xFor(axisMax)}
          y2={yFor(axisMax)}
          stroke="var(--form-outline, #999)"
          strokeDasharray="4 4"
          strokeWidth={1}
          opacity={0.5}
        />

        {/* Tick lines + labels */}
        {ticks.map((v) => (
          <g key={`tx-${v}`}>
            <line
              x1={xFor(v)}
              y1={pad.top}
              x2={xFor(v)}
              y2={pad.top + plotH}
              stroke="var(--border)"
              strokeWidth={1}
            />
            <text
              x={xFor(v)}
              y={pad.top + plotH + 16}
              textAnchor="middle"
              fontSize={11}
              fill="var(--text-muted)"
            >
              {v}
            </text>
          </g>
        ))}
        {ticks.map((v) => (
          <g key={`ty-${v}`}>
            <line
              x1={pad.left}
              y1={yFor(v)}
              x2={pad.left + plotW}
              y2={yFor(v)}
              stroke="var(--border)"
              strokeWidth={1}
            />
            <text
              x={pad.left - 6}
              y={yFor(v) + 4}
              textAnchor="end"
              fontSize={11}
              fill="var(--text-muted)"
            >
              {v}
            </text>
          </g>
        ))}

        {/* Axis titles */}
        <text
          x={pad.left + plotW / 2}
          y={height - 6}
          textAnchor="middle"
          fontSize={12}
          fill="var(--heading)"
        >
          Goals For
        </text>
        <text
          transform={`translate(12 ${pad.top + plotH / 2}) rotate(-90)`}
          textAnchor="middle"
          fontSize={12}
          fill="var(--heading)"
        >
          Goals Against
        </text>

        {/* Team dots. Radius shrinks when the point cloud is dense so
            a 300-dot "All" view stays readable. Starred teams draw on
            top (rendered last) with a bigger radius, a dark ring, and
            an inline label — the whole point of starring is that these
            dots stand out when the chart is shared. */}
        {(() => {
          const baseR = points.length > 100 ? 3 : points.length > 40 ? 4 : 5;
          const selR = baseR + 4;
          const unstarred = points.filter((p) => !selectedSlugs.has(p.program_slug));
          const starred = points.filter((p) => selectedSlugs.has(p.program_slug));
          const dotFor = (p: Standing, selected: boolean) => {
            const gd = p.goals_for - p.goals_against;
            const fill =
              gd > 0
                ? "var(--form-win)"
                : gd < 0
                  ? "var(--form-loss)"
                  : "var(--form-tie)";
            const cx = xFor(p.goals_for);
            const cy = yFor(p.goals_against);
            const r = selected ? selR : baseR;
            const anchor = labelAnchorFor(cx, cy, pad, plotW, plotH);
            return (
              <g key={p.program_slug}>
                <circle
                  cx={cx}
                  cy={cy}
                  r={r}
                  fill={fill}
                  opacity={selected ? 1 : 0.85}
                  stroke={selected ? "var(--text)" : "none"}
                  strokeWidth={selected ? 2 : 0}
                  style={{ cursor: "pointer" }}
                  onClick={() =>
                    onOpenRoster(p.program_slug, p.program_name || p.program_slug)
                  }
                >
                  <title>
                    {`${p.program_name || p.program_slug} · GF ${p.goals_for} · GA ${p.goals_against} · GD ${gd >= 0 ? "+" : ""}${gd}`}
                  </title>
                </circle>
                {selected && (
                  <text
                    x={cx + anchor.dx * (r + 4)}
                    y={cy + anchor.dy}
                    textAnchor={anchor.textAnchor}
                    fontSize={12}
                    fontWeight={600}
                    fill="var(--text)"
                    style={{ pointerEvents: "none" }}
                  >
                    {p.program_name || p.program_slug}
                  </text>
                )}
              </g>
            );
          };
          return (
            <>
              {unstarred.map((p) => dotFor(p, false))}
              {starred.map((p) => dotFor(p, true))}
            </>
          );
        })()}
      </svg>
    </ChartFrame>
  );
}

/// SoS-vs-RPI scatter — one dot per played team. X = Strength of
/// Schedule (OWP component of RPI, so higher = tougher schedule).
/// Y = RPI (top of the plot). A dashed y = x line is a rough sanity
/// indicator: teams sitting well ABOVE that line achieved a stronger
/// RPI than their SoS alone would predict (dominant against weak
/// schedules OR big wins against tough ones). Click a dot to open
/// the roster.
function SosScatter({
  rows,
  onOpenRoster,
  selectedSlugs,
}: {
  rows: Standing[];
  onOpenRoster: (slug: string, name: string) => void;
  selectedSlugs: Set<string>;
}) {
  const points = rows.filter(
    (r) => r.has_schedule_data && r.games_played > 0 && r.rpi > 0,
  );
  if (points.length < 2) return null;

  const width = 960;
  const height = 320;
  const pad = { top: 16, right: 24, bottom: 40, left: 52 };
  const plotW = width - pad.left - pad.right;
  const plotH = height - pad.top - pad.bottom;

  // Both axes are in [0, 1]; give a bit of headroom so dots don't
  // touch the edges. Use a shared window derived from the data.
  const allX = points.map((p) => p.sos);
  const allY = points.map((p) => p.rpi);
  const minX = Math.max(0, Math.min(...allX) - 0.05);
  const maxX = Math.min(1, Math.max(...allX) + 0.05);
  const minY = Math.max(0, Math.min(...allY) - 0.05);
  const maxY = Math.min(1, Math.max(...allY) + 0.05);

  const xFor = (v: number) =>
    pad.left + ((v - minX) / (maxX - minX)) * plotW;
  const yFor = (v: number) =>
    pad.top + (1 - (v - minY) / (maxY - minY)) * plotH;

  const tickVals = (lo: number, hi: number) => {
    const step = 0.05;
    const first = Math.ceil(lo / step) * step;
    const out: number[] = [];
    for (let v = first; v <= hi + 1e-9; v += step) out.push(Math.round(v * 100) / 100);
    return out;
  };
  const xTicks = tickVals(minX, maxX);
  const yTicks = tickVals(minY, maxY);

  return (
    <ChartFrame title="Strength of Schedule vs. RPI">
      <svg
        width="100%"
        viewBox={`0 0 ${width} ${height}`}
        preserveAspectRatio="xMidYMid meet"
        role="img"
        aria-label="Strength of schedule vs RPI scatter"
      >
        {/* y = x reference line (only visible within the shared window) */}
        {(() => {
          const from = Math.max(minX, minY);
          const to = Math.min(maxX, maxY);
          if (to <= from) return null;
          return (
            <line
              x1={xFor(from)}
              y1={yFor(from)}
              x2={xFor(to)}
              y2={yFor(to)}
              stroke="var(--form-outline, #999)"
              strokeDasharray="4 4"
              strokeWidth={1}
              opacity={0.5}
            />
          );
        })()}

        {xTicks.map((v) => (
          <g key={`sx-${v}`}>
            <line
              x1={xFor(v)}
              y1={pad.top}
              x2={xFor(v)}
              y2={pad.top + plotH}
              stroke="var(--border)"
              strokeWidth={1}
            />
            <text
              x={xFor(v)}
              y={pad.top + plotH + 16}
              textAnchor="middle"
              fontSize={11}
              fill="var(--text-muted)"
            >
              {v.toFixed(2)}
            </text>
          </g>
        ))}
        {yTicks.map((v) => (
          <g key={`sy-${v}`}>
            <line
              x1={pad.left}
              y1={yFor(v)}
              x2={pad.left + plotW}
              y2={yFor(v)}
              stroke="var(--border)"
              strokeWidth={1}
            />
            <text
              x={pad.left - 6}
              y={yFor(v) + 4}
              textAnchor="end"
              fontSize={11}
              fill="var(--text-muted)"
            >
              {v.toFixed(2)}
            </text>
          </g>
        ))}

        <text
          x={pad.left + plotW / 2}
          y={height - 6}
          textAnchor="middle"
          fontSize={12}
          fill="var(--heading)"
        >
          Strength of Schedule (OWP)
        </text>
        <text
          transform={`translate(14 ${pad.top + plotH / 2}) rotate(-90)`}
          textAnchor="middle"
          fontSize={12}
          fill="var(--heading)"
        >
          RPI
        </text>

        {(() => {
          const baseR = points.length > 100 ? 3 : points.length > 40 ? 4 : 5;
          const selR = baseR + 4;
          const unstarred = points.filter((p) => !selectedSlugs.has(p.program_slug));
          const starred = points.filter((p) => selectedSlugs.has(p.program_slug));
          const dotFor = (p: Standing, selected: boolean) => {
            const cx = xFor(p.sos);
            const cy = yFor(p.rpi);
            const r = selected ? selR : baseR;
            const anchor = labelAnchorFor(cx, cy, pad, plotW, plotH);
            return (
              <g key={p.program_slug}>
                <circle
                  cx={cx}
                  cy={cy}
                  r={r}
                  fill="var(--link)"
                  opacity={selected ? 1 : 0.75}
                  stroke={selected ? "var(--text)" : "none"}
                  strokeWidth={selected ? 2 : 0}
                  style={{ cursor: "pointer" }}
                  onClick={() =>
                    onOpenRoster(p.program_slug, p.program_name || p.program_slug)
                  }
                >
                  <title>
                    {`${p.program_name || p.program_slug} · RPI ${p.rpi.toFixed(3)} · SoS ${p.sos.toFixed(3)} · rank ${p.rpi_rank || "—"}`}
                  </title>
                </circle>
                {selected && (
                  <text
                    x={cx + anchor.dx * (r + 4)}
                    y={cy + anchor.dy}
                    textAnchor={anchor.textAnchor}
                    fontSize={12}
                    fontWeight={600}
                    fill="var(--text)"
                    style={{ pointerEvents: "none" }}
                  >
                    {p.program_name || p.program_slug}
                  </text>
                )}
              </g>
            );
          };
          return (
            <>
              {unstarred.map((p) => dotFor(p, false))}
              {starred.map((p) => dotFor(p, true))}
            </>
          );
        })()}
      </svg>
    </ChartFrame>
  );
}

/// Square W-L-T grid of every conference member vs every other. Cell
/// (row, col) shows the result of row-team's game against col-team from
/// the row team's perspective. Empty cells = teams haven't played yet;
/// diagonal = self. Uses the current standings sort order so users can
/// re-order the matrix by clicking the standings headers.
function HeadToHeadMatrix({
  conference,
  teams,
}: {
  conference: string;
  teams: Standing[];
}) {
  const [games, setGames] = useState<H2HGame[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  // All hooks must run in the same order on every render — declare
  // ref up here alongside state/effect, BEFORE any conditional returns.
  const tableRef = useRef<HTMLTableElement>(null);

  useEffect(() => {
    if (!conference) {
      setGames(null);
      return;
    }
    setGames(null);
    setError(null);
    invoke<H2HGame[]>("list_head_to_head", { conference })
      .then(setGames)
      .catch((e) => setError(String(e)));
  }, [conference]);

  // Only meaningful when a conference is selected and we can see enough
  // members to fill a grid.
  if (!conference) return null;
  const rowTeams = teams.filter((t) => t.has_schedule_data);
  if (rowTeams.length < 2) return null;
  if (error) {
    return (
      <p className="error" role="alert">
        Head-to-head error: {error}
      </p>
    );
  }
  if (games === null) return null;

  const cellMap = new Map<string, H2HGame>();
  for (const g of games) {
    cellMap.set(`${g.program_slug}|${g.opponent_slug}`, g);
  }

  return (
    <ChartFrame
      title="Head-to-Head"
      captureRef={tableRef as React.RefObject<HTMLElement>}
    >
      <div className="h2h-wrapper">
        <table className="h2h-matrix" ref={tableRef}>
        <thead>
          <tr>
            <th className="h2h-corner" />
            {rowTeams.map((t) => (
              <th
                key={t.program_slug}
                className="h2h-col-header"
                title={t.program_name || t.program_slug}
              >
                <span>{t.program_name || t.program_slug}</span>
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rowTeams.map((row) => (
            <tr key={row.program_slug}>
              <th scope="row" className="h2h-row-header">
                {row.program_name || row.program_slug}
              </th>
              {rowTeams.map((col) => {
                if (row.program_slug === col.program_slug) {
                  return (
                    <td key={col.program_slug} className="h2h-self">
                      ·
                    </td>
                  );
                }
                const g = cellMap.get(
                  `${row.program_slug}|${col.program_slug}`,
                );
                if (!g) {
                  return (
                    <td
                      key={col.program_slug}
                      className="h2h-cell h2h-empty"
                      title="Not played"
                    />
                  );
                }
                const cls = `h2h-cell h2h-${g.outcome.toLowerCase()}`;
                const venue =
                  g.home_away === "away"
                    ? "at"
                    : g.home_away === "neutral"
                      ? "vs (n)"
                      : "vs";
                const title = `${row.program_name || row.program_slug} ${venue} ${col.program_name || col.program_slug} · ${g.date} · ${g.outcome} ${g.team_score}-${g.opponent_score}`;
                return (
                  <td key={col.program_slug} className={cls} title={title}>
                    {g.team_score}-{g.opponent_score}
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
      </div>
    </ChartFrame>
  );
}

function StandingsPage({
  conference,
  onOpenRoster,
  onSelectConference,
}: {
  conference: string;
  onOpenRoster: (slug: string, name: string) => void;
  onSelectConference: (conference: string) => void;
}) {
  const [standings, setStandings] = useState<Standing[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sort, toggleSort] = useSortSpec<StandingSortKey>();
  const [query, setQuery] = useState<string>("");
  const [mode, setMode] = useState<StandingsMode>("conference");
  // Teams the user has "starred" in the standings — their dots on the
  // scatters render larger, ringed, and labelled so the current view
  // can be shared with a specific team called out. Reset on conference
  // change so switching conferences starts with a clean selection.
  const [selectedSlugs, setSelectedSlugs] = useState<Set<string>>(new Set());
  useEffect(() => {
    setSelectedSlugs(new Set());
  }, [conference]);
  const toggleSelected = useCallback((slug: string) => {
    setSelectedSlugs((prev) => {
      const next = new Set(prev);
      if (next.has(slug)) next.delete(slug);
      else next.add(slug);
      return next;
    });
  }, []);

  useEffect(() => {
    setStandings(null);
    setError(null);
    invoke<Standing[]>("list_standings", {
      conference: conference || null,
      mode,
    })
      .then(setStandings)
      .catch((e) => setError(String(e)));
  }, [conference, mode]);

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
        case "rpi_rank":
          // Rank 0 means "unranked" — always sort those last regardless
          // of direction, mirroring the roster's null-handling pattern.
          if (row.rpi_rank === 0) {
            return sort?.dir === "desc" ? -Infinity : Infinity;
          }
          return row.rpi_rank;
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
        <div className="segmented" role="tablist" aria-label="Games included">
          {STANDINGS_MODES.map((m) => (
            <button
              key={m.key}
              role="tab"
              aria-selected={mode === m.key}
              className={`segment ${mode === m.key ? "segment-active" : ""}`}
              onClick={() => setMode(m.key)}
            >
              {m.label}
            </button>
          ))}
        </div>
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
              <th
                className="star-col"
                title="Star a team to highlight it on the charts"
                aria-label="Highlight column"
              >
                ★
              </th>
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
                label="RPI"
                sortKey="rpi_rank"
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
            {sortedRows.map((s) => {
              // Zero-record stubs (Oklahoma, Notre Dame, etc.) get
              // em-dashes so users can distinguish "no data ingested"
              // from "played 0 in this view". The Conf column and
              // program link remain interactive on stub rows so users
              // can still navigate.
              const dash = "—";
              const dashOr = <T,>(v: T) => (s.has_schedule_data ? String(v) : dash);
              const isSelected = selectedSlugs.has(s.program_slug);
              // Stub rows have no data point on the scatters, so
              // starring them is meaningless — disable that column.
              const canStar = s.has_schedule_data && s.games_played > 0;
              return (
                <tr
                  key={s.program_slug}
                  className={isSelected ? "row-selected" : undefined}
                >
                  <td className="star-col">
                    <button
                      className={`star-btn${isSelected ? " star-btn-on" : ""}`}
                      onClick={() => toggleSelected(s.program_slug)}
                      disabled={!canStar}
                      title={
                        !canStar
                          ? "No played games to highlight"
                          : isSelected
                            ? "Un-highlight on charts"
                            : "Highlight on charts"
                      }
                      aria-pressed={isSelected}
                      aria-label={`${isSelected ? "Un-star" : "Star"} ${s.program_name || s.program_slug}`}
                    >
                      {isSelected ? "★" : "☆"}
                    </button>
                  </td>
                  <td>
                    <button
                      className="linklike"
                      onClick={() =>
                        onOpenRoster(s.program_slug, s.program_name || s.program_slug)
                      }
                      title={
                        s.has_schedule_data
                          ? `Roster for ${s.program_name || s.program_slug}`
                          : `Roster for ${s.program_name || s.program_slug} (no schedule ingested)`
                      }
                    >
                      {s.program_name || s.program_slug}
                    </button>
                  </td>
                  <td>
                    <button
                      className="linklike"
                      onClick={() => onSelectConference(s.conference)}
                      title={`Filter to ${fullNameFor(s.conference)}`}
                    >
                      {labelFor(s.conference)}
                    </button>
                  </td>
                  <td title="RPI rank across every loaded program (1 = best)">
                    {s.rpi_rank === 0 ? dash : s.rpi_rank}
                  </td>
                  <td>{dashOr(s.points)}</td>
                  <td>{dashOr(s.games_played)}</td>
                  <td>
                    {s.has_schedule_data
                      ? `${s.wins}-${s.losses}-${s.ties}`
                      : dash}
                  </td>
                  <td>{dashOr(s.goals_for)}</td>
                  <td>{dashOr(s.goals_against)}</td>
                  <td>{dashOr(s.goals_for - s.goals_against)}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
      {sortedRows !== null && sortedRows.length > 0 && (
        <div className="standings-charts">
          <GoalsScatter
            rows={sortedRows}
            onOpenRoster={onOpenRoster}
            selectedSlugs={selectedSlugs}
          />
          <SosScatter
            rows={sortedRows}
            onOpenRoster={onOpenRoster}
            selectedSlugs={selectedSlugs}
          />
          <HeadToHeadMatrix conference={conference} teams={sortedRows} />
        </div>
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

// ---- form timeline ------------------------------------------------------

/// Horizontal strip of one dot per game in chronological order. Played
/// games are filled in W/L/T colors; unplayed games are outlined so the
/// user sees how much of the season is ahead. Tooltip on each dot shows
/// the date, opponent, and result.
function FormTimeline({ games }: { games: Game[] }) {
  // Games arrive sorted by date from list_schedule.
  const RADIUS = 7;
  const GAP = 10;
  const STROKE = 1.5;
  const HEIGHT = RADIUS * 2 + STROKE * 2 + 4;
  const step = RADIUS * 2 + GAP;
  const width = games.length * step - GAP + STROKE * 2;

  const fillFor = (g: Game): string => {
    if (!g.result) return "transparent";
    if (g.result.outcome === "W") return "var(--form-win, #2d8f3d)";
    if (g.result.outcome === "L") return "var(--form-loss, #c53838)";
    if (g.result.outcome === "T") return "var(--form-tie, #808080)";
    return "transparent";
  };

  return (
    <div className="form-timeline" aria-label="Season form">
      <svg
        width={width}
        height={HEIGHT}
        viewBox={`0 0 ${width} ${HEIGHT}`}
        role="img"
      >
        {games.map((g, i) => {
          const cx = STROKE + RADIUS + i * step;
          const cy = HEIGHT / 2;
          const played = g.result !== null;
          const title = g.result
            ? `${g.date} · ${g.home_away === "away" ? "at" : "vs"} ${g.opponent} · ${g.result.outcome} ${g.result.team_score}-${g.result.opponent_score}`
            : `${g.date} · ${g.home_away === "away" ? "at" : "vs"} ${g.opponent} · upcoming`;
          return (
            <circle
              key={`${g.date}-${g.opponent}-${i}`}
              cx={cx}
              cy={cy}
              r={RADIUS}
              fill={fillFor(g)}
              stroke="var(--form-outline, currentColor)"
              strokeWidth={played ? 0 : STROKE}
              opacity={played ? 1 : 0.35}
            >
              <title>{title}</title>
            </circle>
          );
        })}
      </svg>
    </div>
  );
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
        <FormTimeline games={games} />
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
