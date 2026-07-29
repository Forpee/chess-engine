import { useEffect, useRef, useState } from "react";
import Board from "./components/Board";
import EvalBar from "./components/EvalBar";
import Panel from "./components/Panel";
import { useGame } from "./useGame";

export default function App() {
  const { state, busy, thinking, error, run } = useGame();
  const [flipped, setFlipped] = useState(false);
  // Last score seen, so the eval bar holds its value between searches.
  const lastScore = useRef<number | null>(null);
  // Orientation follows your colour, but only when the colour actually
  // changes — otherwise every refresh would undo a manual flip.
  const lastHuman = useRef<string | null>(null);

  useEffect(() => {
    if (state && state.human !== lastHuman.current) {
      lastHuman.current = state.human;
      setFlipped(state.human === "black");
    }
  }, [state]);

  if (!state) {
    return (
      <main>
        <p className="subtle">{error ?? "Connecting to the engine…"}</p>
      </main>
    );
  }

  if (state.engine) lastScore.current = state.engine.scoreCp;

  return (
    <main>
      <section className="board-area">
        <EvalBar scoreCp={lastScore.current} flipped={flipped} />
        <Board
          state={state}
          flipped={flipped}
          interactive={!busy && !thinking && !state.over}
          onMove={(uci) => run(`move ${uci}`)}
        />
      </section>

      <Panel
        state={state}
        thinking={thinking}
        busy={busy}
        error={error}
        onCommand={run}
        onFlip={() => setFlipped((value) => !value)}
        onNewGame={(color) => {
          lastScore.current = null;
          void run(`new ${color}`);
        }}
      />
    </main>
  );
}
