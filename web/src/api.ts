/** The shape the Rust server returns from POST /api. */
export type Color = "white" | "black";

export interface EngineMove {
  san: string;
  uci: string;
  /** Human-readable score, e.g. "+0.24" or "mate in 3 for white". */
  score: string;
  /** Centipawns from white's point of view. */
  scoreCp: number;
  depth: number;
  nodes: number;
  millis: number;
}

export interface GameState {
  fen: string;
  turn: Color;
  human: Color;
  check: boolean;
  over: boolean;
  result: string;
  /** True when it is the engine's move and the game is still running. */
  engineToMove: boolean;
  lastMove: string;
  materialBalance: number;
  movetime: number;
  message: string;
  engine: EngineMove | null;
  /** Every legal move in UCI form — the only rules knowledge the UI gets. */
  legal: string[];
  history: string[];
}

/**
 * Sends one command to the engine and returns the new game state.
 * Commands: `state`, `move <san|uci>`, `engine`, `hint`, `undo`, `resign`,
 * `new <white|black>`, `time <ms>`.
 */
export async function command(text: string): Promise<GameState> {
  const response = await fetch("/api", { method: "POST", body: text });
  if (!response.ok) {
    throw new Error(`engine returned ${response.status}`);
  }
  return (await response.json()) as GameState;
}
