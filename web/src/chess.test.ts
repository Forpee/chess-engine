import { describe, expect, it } from "vitest";
import {
  boardFromFen,
  canMoveFrom,
  destinationsFrom,
  findKing,
  isDarkSquare,
  movePairs,
  squareOrder,
  whiteShare,
} from "./chess";

const START = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

describe("boardFromFen", () => {
  it("places every piece of the start position", () => {
    const board = boardFromFen(START);
    expect(board.size).toBe(32);
    expect(board.get("a1")).toBe("R");
    expect(board.get("e1")).toBe("K");
    expect(board.get("e8")).toBe("k");
    expect(board.get("h7")).toBe("p");
    expect(board.get("e4")).toBeUndefined();
  });

  it("handles empty-square runs and a partly empty board", () => {
    const board = boardFromFen("8/8/8/4p3/8/8/8/K6k w - - 0 1");
    expect(board.size).toBe(3);
    expect(board.get("e5")).toBe("p");
    expect(board.get("a1")).toBe("K");
    expect(board.get("h1")).toBe("k");
  });
});

describe("squareOrder", () => {
  it("runs a8 to h1 for white", () => {
    const order = squareOrder(false);
    expect(order[0]).toBe("a8");
    expect(order[7]).toBe("h8");
    expect(order[63]).toBe("h1");
    expect(order).toHaveLength(64);
  });

  it("is exactly reversed when flipped", () => {
    expect(squareOrder(true)).toEqual([...squareOrder(false)].reverse());
  });
});

describe("destinationsFrom", () => {
  const legal = ["e2e4", "e2e3", "g1f3", "e7e8q", "e7e8r", "e7e8b", "e7e8n"];

  it("groups moves by target square", () => {
    const targets = destinationsFrom(legal, "e2");
    expect([...targets.keys()].sort()).toEqual(["e3", "e4"]);
    expect(targets.get("e4")).toEqual(["e2e4"]);
  });

  it("keeps every promotion choice for one target", () => {
    expect(destinationsFrom(legal, "e7").get("e8")).toHaveLength(4);
  });

  it("returns nothing for a square with no moves", () => {
    expect(destinationsFrom(legal, "h8").size).toBe(0);
  });

  it("reports which squares can be picked up", () => {
    expect(canMoveFrom(legal, "e2")).toBe(true);
    expect(canMoveFrom(legal, "a1")).toBe(false);
  });
});

describe("board colours", () => {
  it("puts a light square in each player's right-hand corner", () => {
    expect(isDarkSquare("a1")).toBe(true);
    expect(isDarkSquare("h1")).toBe(false);
    expect(isDarkSquare("a8")).toBe(false);
    expect(isDarkSquare("h8")).toBe(true);
    // The white king starts on a dark square; the file alternates from there.
    expect(isDarkSquare("e1")).toBe(true);
    expect(isDarkSquare("e4")).toBe(false);
  });
});

describe("findKing", () => {
  it("finds each king and distinguishes colour", () => {
    const board = boardFromFen(START);
    expect(findKing(board, "white")).toBe("e1");
    expect(findKing(board, "black")).toBe("e8");
  });
});

describe("whiteShare", () => {
  it("is even at zero and monotonic", () => {
    expect(whiteShare(0)).toBeCloseTo(0.5);
    expect(whiteShare(300)).toBeGreaterThan(0.6);
    expect(whiteShare(-300)).toBeLessThan(0.4);
    expect(whiteShare(5000)).toBeGreaterThan(0.99);
    // Symmetric around zero.
    expect(whiteShare(200) + whiteShare(-200)).toBeCloseTo(1);
  });
});

describe("movePairs", () => {
  it("numbers moves and leaves a trailing white move unpaired", () => {
    expect(movePairs(["e4", "e5", "Nf3"])).toEqual([
      [1, "e4", "e5"],
      [2, "Nf3", ""],
    ]);
    expect(movePairs([])).toEqual([]);
  });
});
