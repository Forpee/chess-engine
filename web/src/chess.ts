/**
 * Pure display helpers. Deliberately contains no chess *rules*: legality,
 * promotion and game end all come from the engine, so these only ever
 * rearrange what the server already said.
 */

export const FILES = "abcdefgh";

export const GLYPHS: Record<string, string> = {
  k: "♚",
  q: "♛",
  r: "♜",
  b: "♝",
  n: "♞",
  p: "♟",
};

export type Board = Map<string, string>;

/** Square names in render order for the given orientation, a8-first. */
export function squareOrder(flipped: boolean): string[] {
  const ranks = flipped ? [1, 2, 3, 4, 5, 6, 7, 8] : [8, 7, 6, 5, 4, 3, 2, 1];
  const files = flipped ? [7, 6, 5, 4, 3, 2, 1, 0] : [0, 1, 2, 3, 4, 5, 6, 7];
  return ranks.flatMap((rank) => files.map((file) => FILES[file] + rank));
}

/** Expands the placement field of a FEN into a square -> piece letter map. */
export function boardFromFen(fen: string): Board {
  const board: Board = new Map();
  const rows = fen.split(" ")[0].split("/");
  rows.forEach((row, index) => {
    const rank = 8 - index;
    let file = 0;
    for (const character of row) {
      if (character >= "1" && character <= "8") {
        file += Number(character);
      } else {
        board.set(FILES[file] + rank, character);
        file += 1;
      }
    }
  });
  return board;
}

/**
 * Legal destinations from a square: target square -> the full UCI moves that
 * reach it. More than one move per target means a promotion choice.
 */
export function destinationsFrom(legal: string[], from: string): Map<string, string[]> {
  const targets = new Map<string, string[]>();
  for (const move of legal) {
    if (move.slice(0, 2) !== from) continue;
    const to = move.slice(2, 4);
    const existing = targets.get(to);
    if (existing) existing.push(move);
    else targets.set(to, [move]);
  }
  return targets;
}

export function canMoveFrom(legal: string[], square: string): boolean {
  return legal.some((move) => move.slice(0, 2) === square);
}

export function findKing(board: Board, color: "white" | "black"): string | null {
  const wanted = color === "white" ? "K" : "k";
  for (const [square, piece] of board) {
    if (piece === wanted) return square;
  }
  return null;
}

export function isWhitePiece(piece: string): boolean {
  return piece === piece.toUpperCase();
}

/** A dark square is one where file index + rank index is even (a1 is dark). */
export function isDarkSquare(square: string): boolean {
  const file = FILES.indexOf(square[0]);
  const rank = Number(square[1]) - 1;
  return (file + rank) % 2 === 0;
}

/**
 * Maps a centipawn score to white's share of the eval bar, flattening the
 * extremes so a decisive advantage doesn't peg the bar at a full 0 or 100.
 */
export function whiteShare(centipawns: number): number {
  return 1 / (1 + Math.exp(-centipawns / 320));
}

/** Groups SAN moves into numbered pairs for display. */
export function movePairs(history: string[]): Array<[number, string, string]> {
  const pairs: Array<[number, string, string]> = [];
  for (let i = 0; i < history.length; i += 2) {
    pairs.push([i / 2 + 1, history[i], history[i + 1] ?? ""]);
  }
  return pairs;
}
