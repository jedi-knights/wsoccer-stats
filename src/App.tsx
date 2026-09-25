import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

type Standing = {
  program_slug: string;
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
  | { kind: "roster"; slug: string };

const CONFERENCE_LABELS: Record<string, string> = {
  acc: "ACC",
  sec: "SEC",
  big_ten: "Big Ten",
  big_12: "Big 12",
};

function labelFor(conf: string): string {
  return CONFERENCE_LABELS[conf] ?? conf;
}

function App() {
  const [view, setView] = useState<View>({ kind: "standings" });
  return view.kind === "standings" ? (
    <StandingsPage onOpenRoster={(slug) => setView({ kind: "roster", slug })} />
  ) : (
    <RosterPage slug={view.slug} onBack={() => setView({ kind: "standings" })} />
  );
}

function StandingsPage({ onOpenRoster }: { onOpenRoster: (slug: string) => void }) {
  const [conferences, setConferences] = useState<string[]>([]);
  const [selected, setSelected] = useState<string>(""); // "" = all
  const [standings, setStandings] = useState<Standing[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<string[]>("list_conferences")
      .then(setConferences)
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    setStandings(null);
    setError(null);
    invoke<Standing[]>("list_standings", {
      conference: selected || null,
    })
      .then(setStandings)
      .catch((e) => setError(String(e)));
  }, [selected]);

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
      {standings === null && !error && <p>Loading…</p>}
      {standings !== null && standings.length === 0 && !error && (
        <p>No games found for this selection.</p>
      )}
      {standings !== null && standings.length > 0 && (
        <table className="standings">
          <thead>
            <tr>
              <th>Program</th>
              <th>Conf</th>
              <th>GP</th>
              <th>W</th>
              <th>L</th>
              <th>T</th>
              <th>GF</th>
              <th>GA</th>
              <th>GD</th>
            </tr>
          </thead>
          <tbody>
            {standings.map((s) => (
              <tr key={s.program_slug}>
                <td>
                  <button
                    className="linklike"
                    onClick={() => onOpenRoster(s.program_slug)}
                    title={`Roster for ${s.program_slug}`}
                  >
                    {s.program_slug}
                  </button>
                </td>
                <td>{labelFor(s.conference)}</td>
                <td>{s.games_played}</td>
                <td>{s.wins}</td>
                <td>{s.losses}</td>
                <td>{s.ties}</td>
                <td>{s.goals_for}</td>
                <td>{s.goals_against}</td>
                <td>{s.goals_for - s.goals_against}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </main>
  );
}

function RosterPage({ slug, onBack }: { slug: string; onBack: () => void }) {
  const [players, setPlayers] = useState<Player[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<Player[]>("list_roster", { slug })
      .then(setPlayers)
      .catch((e) => setError(String(e)));
  }, [slug]);

  return (
    <main className="container">
      <header className="page-header">
        <div>
          <button className="linklike" onClick={onBack}>
            ← Standings
          </button>
          <h1>{slug}</h1>
          {players && players[0]?.conference && (
            <p className="subtitle">
              {labelFor(players[0].conference)} · {players.length} players
            </p>
          )}
        </div>
      </header>
      {error && (
        <p className="error" role="alert">
          Error: {error}
        </p>
      )}
      {players === null && !error && <p>Loading…</p>}
      {players !== null && players.length === 0 && !error && (
        <p>No roster data for this program.</p>
      )}
      {players !== null && players.length > 0 && (
        <table className="roster">
          <thead>
            <tr>
              <th>#</th>
              <th>Name</th>
              <th>Pos</th>
              <th>Class</th>
              <th>Ht</th>
              <th>Hometown</th>
            </tr>
          </thead>
          <tbody>
            {players.map((p, i) => (
              <tr key={`${p.jersey_number}-${p.name}-${i}`}>
                <td>{p.jersey_number}</td>
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
