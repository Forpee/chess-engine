import { useCallback, useMemo, useRef, useState } from "react";
import type { GameState } from "../api";
import {
  boardFromFen,
  canMoveFrom,
  destinationsFrom,
  findKing,
  FILES,
  GLYPHS,
  isDarkSquare,
  isWhitePiece,
  squareOrder,
} from "../chess";
import PromotionPicker from "./PromotionPicker";

interface Props {
  state: GameState;
  flipped: boolean;
  /** False while the engine thinks or after the game ends. */
  interactive: boolean;
  onMove: (uci: string) => void;
}

interface Drag {
  from: string;
  x: number;
  y: number;
  piece: string;
}

export default function Board({ state, flipped, interactive, onMove }: Props) {
  const [selected, setSelected] = useState<string | null>(null);
  const [promotion, setPromotion] = useState<{ to: string; moves: string[] } | null>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const boardRef = useRef<HTMLDivElement>(null);

  const board = useMemo(() => boardFromFen(state.fen), [state.fen]);
  const order = useMemo(() => squareOrder(flipped), [flipped]);
  const targets = useMemo(
    () => (selected ? destinationsFrom(state.legal, selected) : new Map<string, string[]>()),
    [selected, state.legal],
  );
  const checkedKing = state.check ? findKing(board, state.turn) : null;

  /** Plays from -> to, or opens the promotion picker when the piece matters. */
  const attempt = useCallback(
    (from: string, to: string) => {
      const moves = destinationsFrom(state.legal, from).get(to);
      if (!moves) return;
      setSelected(null);
      if (moves.length === 1) onMove(moves[0]);
      else setPromotion({ to, moves });
    },
    [state.legal, onMove],
  );

  const squareAt = (x: number, y: number): string | null => {
    const element = document.elementFromPoint(x, y);
    return element?.closest<HTMLElement>("[data-square]")?.dataset.square ?? null;
  };

  const onPointerDown = (event: React.PointerEvent) => {
    if (!interactive || promotion) return;
    const square = squareAt(event.clientX, event.clientY);
    if (!square) return;

    if (selected && selected !== square && targets.has(square)) {
      attempt(selected, square);
      return;
    }
    if (!canMoveFrom(state.legal, square)) {
      setSelected(null);
      return;
    }
    event.preventDefault();
    setSelected(square);
    const piece = board.get(square);
    if (piece) setDrag({ from: square, x: event.clientX, y: event.clientY, piece });
  };

  const onPointerMove = (event: React.PointerEvent) => {
    if (drag) setDrag({ ...drag, x: event.clientX, y: event.clientY });
  };

  const onPointerUp = (event: React.PointerEvent) => {
    if (!drag) return;
    const { from } = drag;
    setDrag(null);
    const to = squareAt(event.clientX, event.clientY);
    // Releasing on the origin keeps the piece selected for a second click.
    if (to && to !== from) attempt(from, to);
  };

  return (
    <div className="board-wrap">
      <div
        ref={boardRef}
        className="board"
        aria-label="chess board"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
      >
        {order.map((square, index) => {
          const piece = board.get(square);
          const isTarget = targets.has(square);
          const classes = ["square"];
          if (isDarkSquare(square)) classes.push("dark");
          if (state.lastMove.slice(0, 2) === square || state.lastMove.slice(2, 4) === square) {
            classes.push("last");
          }
          if (selected === square) classes.push("selected");
          if (checkedKing === square) classes.push("check");
          if (isTarget) classes.push(piece ? "target capture" : "target");

          return (
            <div
              key={square}
              className={classes.join(" ")}
              data-square={square}
              // Coordinates only on the outer edge, following the orientation.
              data-rank={index % 8 === 0 ? square[1] : undefined}
              data-file={index >= 56 ? square[0] : undefined}
            >
              {piece && (
                <span
                  className={`piece ${isWhitePiece(piece) ? "white" : "black"}`}
                  style={drag?.from === square ? { visibility: "hidden" } : undefined}
                >
                  {GLYPHS[piece.toLowerCase()]}
                </span>
              )}
            </div>
          );
        })}
      </div>

      {drag && (
        <span
          className={`piece dragging ${isWhitePiece(drag.piece) ? "white" : "black"}`}
          style={{ left: drag.x, top: drag.y }}
        >
          {GLYPHS[drag.piece.toLowerCase()]}
        </span>
      )}

      {promotion && (
        <PromotionPicker
          square={promotion.to}
          moves={promotion.moves}
          flipped={flipped}
          color={state.turn}
          onPick={(uci) => {
            setPromotion(null);
            onMove(uci);
          }}
          onCancel={() => setPromotion(null)}
        />
      )}

      {state.over && (
        <div className="overlay">
          <span>{state.result}</span>
        </div>
      )}
    </div>
  );
}

export { FILES };
