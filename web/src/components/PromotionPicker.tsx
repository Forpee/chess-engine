import { FILES, GLYPHS } from "../chess";
import type { Color } from "../api";

interface Props {
  /** The square being promoted on, e.g. "e8". */
  square: string;
  /** The candidate moves, which differ only in their promotion suffix. */
  moves: string[];
  flipped: boolean;
  color: Color;
  onPick: (uci: string) => void;
  onCancel: () => void;
}

const ORDER = ["q", "r", "b", "n"];

export default function PromotionPicker({
  square,
  moves,
  flipped,
  color,
  onPick,
  onCancel,
}: Props) {
  const file = FILES.indexOf(square[0]);
  const rank = Number(square[1]) - 1;
  const column = flipped ? 7 - file : file;
  const promotingOnTopEdge = flipped ? rank === 0 : rank === 7;

  const choices = ORDER.map((piece) => moves.find((move) => move.endsWith(piece))).filter(
    (move): move is string => Boolean(move),
  );

  return (
    <>
      {/* Clicking anywhere else cancels the choice. */}
      <div className="promotion-backdrop" onClick={onCancel} />
      <div
        className="promotion"
        style={{
          left: `${(column / 8) * 100}%`,
          [promotingOnTopEdge ? "top" : "bottom"]: 0,
        }}
      >
        {choices.map((move) => (
          <button
            key={move}
            type="button"
            className={`piece ${color}`}
            onClick={() => onPick(move)}
            aria-label={`promote to ${move.slice(4)}`}
          >
            {GLYPHS[move.slice(4) || "q"]}
          </button>
        ))}
      </div>
    </>
  );
}
