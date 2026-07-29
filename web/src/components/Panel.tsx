import { useEffect, useState } from "react";
import type { GameState } from "../api";
import MoveList from "./MoveList";

interface Props {
  state: GameState;
  thinking: boolean;
  busy: boolean;
  error: string | null;
  onCommand: (text: string) => void;
  onFlip: () => void;
  onNewGame: (color: "white" | "black") => void;
}

export default function Panel({
  state,
  thinking,
  busy,
  error,
  onCommand,
  onFlip,
  onNewGame,
}: Props) {
  // Local copy so dragging the slider stays smooth; committed on release.
  const [movetime, setMovetime] = useState(state.movetime);
  useEffect(() => setMovetime(state.movetime), [state.movetime]);

  const status = state.over
    ? state.result
    : thinking
      ? "Engine is thinking…"
      : state.check
        ? "Your move — you are in check"
        : "Your move";

  const engine = state.engine;
  const detail = engine
    ? `${engine.san} · ${engine.score} · depth ${engine.depth} · ` +
      `${(engine.millis / 1000).toFixed(1)}s · ${engine.nodes.toLocaleString()} nodes`
    : state.message;

  return (
    <aside className="panel">
      <header>
        <h1>chess&#8209;engine</h1>
        <p className="subtle">You are {state.human}</p>
      </header>

      <div className="status">{status}</div>
      <div className="engine-line subtle">{error ?? detail}</div>

      <div className="controls">
        <button type="button" onClick={() => onNewGame("white")} disabled={busy}>
          New · white
        </button>
        <button type="button" onClick={() => onNewGame("black")} disabled={busy}>
          New · black
        </button>
        <button type="button" onClick={() => onCommand("undo")} disabled={busy || thinking}>
          Undo
        </button>
        <button type="button" onClick={() => onCommand("hint")} disabled={busy || state.over}>
          Hint
        </button>
        <button type="button" onClick={onFlip}>
          Flip
        </button>
        <button
          type="button"
          className="danger"
          onClick={() => onCommand("resign")}
          disabled={busy || state.over}
        >
          Resign
        </button>
      </div>

      <label className="slider">
        <span>
          Engine time <output>{(movetime / 1000).toFixed(1)}s</output>
        </span>
        <input
          type="range"
          min={100}
          max={5000}
          step={100}
          value={movetime}
          onChange={(event) => setMovetime(Number(event.target.value))}
          onPointerUp={() => onCommand(`time ${movetime}`)}
          onKeyUp={() => onCommand(`time ${movetime}`)}
        />
      </label>

      <MoveList history={state.history} />
      <code className="fen">{state.fen}</code>
    </aside>
  );
}
