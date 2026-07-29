# chess-engine

A UCI chess engine in Rust, with no dependencies outside the standard library.

## Running

```sh
cargo build --release
./target/release/chess-engine serve    # play in a browser at localhost:8080
./target/release/chess-engine play     # play a game at the terminal
./target/release/chess-engine          # UCI mode, reads commands on stdin
./target/release/chess-engine bench 10 # fixed-depth node-count benchmark
```

## Playing in a browser

```sh
./target/release/chess-engine serve              # you are white, 1s per move
./target/release/chess-engine serve 8090 black 3000
```

Then open the printed URL. Drag or click pieces to move, with legal
destinations shown, a promotion picker, an evaluation bar, a move list, undo,
hints and adjustable thinking time.

The engine runs natively in the same process, so the browser adds no search
overhead — the page is only a view. It binds to loopback and holds one game per
process: a local app, not a service.

The UI is React + TypeScript (see [web/](web/)), built by Vite into static files
that the binary embeds. **Node is only needed to change the UI** — the build
output is committed, so `cargo build` alone produces a working binary.

## Playing at the terminal

```sh
./target/release/chess-engine play            # you are white, 1s per move
./target/release/chess-engine play black 3000 # play black, 3s per move
```

Moves are entered in ordinary notation — `e4`, `Nf3`, `exd5`, `O-O`, `e8=Q` —
or in UCI's long form (`e2e4`) if you prefer. Check and mate marks are
optional, `0-0` works for castling, and lowercase is accepted when it isn't
ambiguous. The board is drawn from your side, with the last move bracketed.

| Command | |
| --- | --- |
| `moves` | list every legal move |
| `hint` | ask the engine what it would play |
| `undo` | take back your move and the engine's reply |
| `board` / `flip` | redraw / swap orientation |
| `eval` | static evaluation, no search |
| `fen [<fen>]` | show the position, or set one up |
| `time <ms>` / `depth <n>` | how hard the engine thinks (default 1s/move) |
| `new [w\|b]` | new game, optionally switching colour |
| `resign`, `quit` | |

Checkmate, stalemate, threefold repetition, the fifty-move rule and
insufficient material are all detected and end the game.

Point any UCI GUI (Cute Chess, Arena, BanksiaGUI, En Croissant) at the release
binary. A quick session by hand:

```
uci
position startpos moves e2e4 e7e5
go movetime 1000
```

Beyond the protocol, the engine also accepts `d` (print the board), `eval`
(static evaluation), `perft <depth>` and `bench <depth>`.

Supported UCI commands: `uci`, `isready`, `ucinewgame`, `setoption name Hash`,
`position [startpos | fen ...] [moves ...]`, `go` (`depth`, `movetime`,
`wtime`/`btime`/`winc`/`binc`/`movestogo`, `nodes`, `infinite`), `stop`, `quit`.

## Design

| Module | Role |
| --- | --- |
| [types.rs](src/types.rs) | Colours, pieces, squares, 16-bit packed moves |
| [bitboard.rs](src/bitboard.rs) | `u64` board sets and shift helpers |
| [attacks.rs](src/attacks.rs) | Leaper tables and magic-bitboard sliders |
| [position.rs](src/position.rs) | Board state, FEN, make/unmake, Zobrist hashing |
| [movegen.rs](src/movegen.rs) | Pseudo-legal generation, filtered on make |
| [eval.rs](src/eval.rs) | Tapered evaluation |
| [search.rs](src/search.rs) | Iterative deepening alpha-beta |
| [tt.rs](src/tt.rs) | Transposition table |
| [uci.rs](src/uci.rs) | Protocol front-end |
| [san.rs](src/san.rs) | Standard algebraic notation, both directions |
| [play.rs](src/play.rs) | Game state and rules shared by both front-ends |
| [server.rs](src/server.rs) | Minimal HTTP server for the browser UI |
| [web/](web/) | React + TypeScript front-end |

**Board.** Bitboards per piece type and colour, plus a mailbox for square
lookups. Sliding attacks use fancy magic bitboards; the magics are searched for
at startup with a fixed-seed PRNG, so there are no multi-kilobyte constant
tables in the source but generation is still deterministic.

**Move generation** is pseudo-legal. Legality is decided by `try_make_move`,
which makes the move and rejects it if the king is left attacked — simpler than
tracking pins and check masks during generation, at a modest cost per move.

**Search** is principal variation search with:

- a transposition table with depth-preferred, aging replacement,
- quiescence search over captures and promotions, with delta pruning,
- null-move pruning and reverse futility pruning,
- late move reductions,
- check extensions and mate-distance pruning,
- move ordering by TT move → MVV-LVA captures → killers → history,
- aspiration windows around the previous iteration's score.

**Evaluation** is tapered between midgame and endgame using PeSTO's material
values and piece-square tables, plus passed/doubled/isolated pawns, mobility,
the bishop pair, rooks on open files, and a king pawn-shelter term.

## Tests

```sh
cargo test --release                 # unit + integration tests (~3s)
cargo test --release -- --ignored    # deep perft, ~480M nodes (~15s)
cd web && npm test                   # front-end tests
```

Move generation is verified with perft against published node counts —
including the standard tricky positions for en passant discovered check,
castling rights, and promotions — up to 119,060,324 nodes from the start
position. `perft_is_colour_symmetric` additionally checks each position against
its own colour-flipped mirror.

Search correctness is checked by tests that solve tactics, play a full game
against itself, and confirm forced mates against a brute-force mate solver that
shares no code with the search.
