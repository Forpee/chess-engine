import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import App from "./App";
import type { GameState } from "./api";

const START = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

function stateOf(overrides: Partial<GameState> = {}): GameState {
  return {
    fen: START,
    turn: "white",
    human: "white",
    check: false,
    over: false,
    result: "",
    engineToMove: false,
    lastMove: "",
    materialBalance: 0,
    movetime: 1000,
    message: "",
    engine: null,
    legal: ["e2e4", "e2e3", "g1f3"],
    history: [],
    ...overrides,
  };
}

/** Replaces fetch and records the commands the page sends. */
function mockEngine(responses: GameState[]) {
  const sent: string[] = [];
  let index = 0;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (_url: string, options: { body: string }) => {
      sent.push(options.body);
      const state = responses[Math.min(index, responses.length - 1)];
      index += 1;
      return { ok: true, json: async () => state } as Response;
    }),
  );
  return sent;
}

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("App", () => {
  it("asks for the state and draws the start position", async () => {
    const sent = mockEngine([stateOf()]);
    render(<App />);

    await waitFor(() => expect(screen.getByLabelText("chess board")).toBeTruthy());
    expect(sent[0]).toBe("state");

    const board = screen.getByLabelText("chess board");
    expect(board.querySelectorAll(".square")).toHaveLength(64);
    expect(board.querySelectorAll(".piece")).toHaveLength(32);
    // a8 is drawn first for white's orientation.
    expect(board.firstElementChild?.getAttribute("data-square")).toBe("a8");
  });

  it("flips the board when you play black", async () => {
    mockEngine([stateOf({ human: "black" })]);
    render(<App />);

    await waitFor(() => expect(screen.getByLabelText("chess board")).toBeTruthy());
    const board = screen.getByLabelText("chess board");
    expect(board.firstElementChild?.getAttribute("data-square")).toBe("h1");
  });

  it("sends a move when a piece is clicked to a legal square", async () => {
    const sent = mockEngine([stateOf(), stateOf({ history: ["e4"], engineToMove: false })]);
    render(<App />);
    await waitFor(() => expect(screen.getByLabelText("chess board")).toBeTruthy());

    const board = screen.getByLabelText("chess board");
    const square = (name: string) => board.querySelector<HTMLElement>(`[data-square="${name}"]`)!;

    // The board resolves pointer positions with elementFromPoint, which jsdom
    // does not implement (it has no layout), so point it at the test squares.
    const pointAt = (name: string) => {
      document.elementFromPoint = (() => square(name)) as typeof document.elementFromPoint;
    };

    pointAt("e2");
    square("e2").dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    await waitFor(() => expect(square("e4").className).toContain("target"));

    pointAt("e4");
    square("e4").dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));

    await waitFor(() => expect(sent).toContain("move e2e4"));
  });

  it("asks the engine to reply when it is its turn", async () => {
    const sent = mockEngine([
      stateOf({ turn: "black", engineToMove: true, history: ["e4"] }),
      stateOf({
        turn: "white",
        engineToMove: false,
        history: ["e4", "e5"],
        engine: {
          san: "e5",
          uci: "e7e5",
          score: "+0.27",
          scoreCp: 27,
          depth: 13,
          nodes: 1175917,
          millis: 240,
        },
      }),
    ]);
    render(<App />);

    await waitFor(() => expect(sent).toContain("engine"));
    await waitFor(() => expect(screen.getByText(/depth 13/)).toBeTruthy());
    expect(screen.getByText(/1,175,917 nodes/)).toBeTruthy();
  });

  it("shows the result when the game is over", async () => {
    mockEngine([stateOf({ over: true, result: "Checkmate — you win!" })]);
    render(<App />);
    await waitFor(() => expect(screen.getAllByText("Checkmate — you win!").length).toBe(2));
  });

  it("reports a dead engine instead of hanging", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => {
        throw new Error("connection refused");
      }),
    );
    render(<App />);
    await waitFor(() => expect(screen.getByText(/connection refused/)).toBeTruthy());
  });
});
