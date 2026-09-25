import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

type Standing = {
  program_slug: string;
  wins: number;
  losses: number;
  ties: number;
  goals_for: number;
  goals_against: number;
  games_played: number;
};

function App() {
  const [standings, setStandings] = useState<Standing[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<Standing[]>("list_standings")
      .then(setStandings)
      .catch((e) => setError(String(e)));
  }, []);

  return (
    <main className="container">
      <h1>Standings</h1>
      {error && (
        <p className="error" role="alert">
          Error: {error}
        </p>
      )}
      {standings === null && !error && <p>Loading…</p>}
      {standings !== null && standings.length === 0 && !error && (
        <p>
          No games found. Set <code>WSOCCER_STATS_DATA_DIR</code> to a directory
          of schedule <code>.ndjson</code> files, or run the athletics-ingest
          pipeline into the sibling repo's <code>output/schedules/</code>.
        </p>
      )}
      {standings !== null && standings.length > 0 && (
        <table className="standings">
          <thead>
            <tr>
              <th>Program</th>
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
                <td>{s.program_slug}</td>
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

export default App;
