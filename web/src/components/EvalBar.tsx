import { whiteShare } from "../chess";

interface Props {
  /** Centipawns from white's point of view, or null before the first search. */
  scoreCp: number | null;
  flipped: boolean;
}

/** A thin bar beside the board: white's share of the evaluation. */
export default function EvalBar({ scoreCp, flipped }: Props) {
  const share = scoreCp === null ? 0.5 : whiteShare(scoreCp);
  const percent = Math.round(share * 100);
  return (
    <div
      className="evalbar"
      title={scoreCp === null ? "no evaluation yet" : `${(scoreCp / 100).toFixed(2)} for white`}
    >
      <div
        className="evalbar-fill"
        style={
          flipped
            ? { top: 0, bottom: "auto", height: `${100 - percent}%` }
            : { bottom: 0, top: "auto", height: `${percent}%` }
        }
      />
    </div>
  );
}
