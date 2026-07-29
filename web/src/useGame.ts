import { useCallback, useEffect, useRef, useState } from "react";
import { command, type GameState } from "./api";

/**
 * Owns the conversation with the engine.
 *
 * The human's move and the engine's reply are two separate requests: the first
 * returns immediately so the board can paint, and `engineToMove` in the reply
 * triggers the second. Without that split the board would sit frozen for the
 * whole search.
 */
export function useGame() {
  const [state, setState] = useState<GameState | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // A ref as well as state, so the auto-reply effect can't fire twice.
  const inFlight = useRef(false);

  const run = useCallback(async (text: string) => {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    try {
      setState(await command(text));
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    void run("state");
  }, [run]);

  useEffect(() => {
    if (state?.engineToMove && !state.over && !busy) {
      void run("engine");
    }
  }, [state, busy, run]);

  const thinking = Boolean(state?.engineToMove) && !state?.over;
  return { state, busy, thinking, error, run };
}
