//! Interactive play: a human at a terminal against the engine.
//!
//! Command handling and engine replies are separate entry points (`execute`
//! and `play_engine_move`) so a game can be driven from tests without a TTY
//! and without searching after every command.

use std::io::{BufRead, Write};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use crate::eval::{self, MATE, MATE_IN_MAX_PLY};
use crate::movegen::generate_legal_moves;
use crate::position::Position;
use crate::san;
use crate::search::{Search, SearchLimits};
use crate::types::{Color, Move, PieceType, Square, square_of};

const DEFAULT_MOVETIME_MS: u64 = 1000;
const HASH_MB: usize = 64;

/// How a finished game ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Checkmate { winner: Color },
    Stalemate,
    FiftyMoveRule,
    ThreefoldRepetition,
    InsufficientMaterial,
    Resignation { winner: Color },
}

impl Outcome {
    /// A one-line result message written for the human's point of view.
    pub fn describe(self, human: Color) -> String {
        let side = |color: Color| {
            if color == human {
                "you win!"
            } else {
                "the engine wins."
            }
        };
        match self {
            Outcome::Checkmate { winner } => format!("Checkmate — {}", side(winner)),
            Outcome::Resignation { winner } => format!("Resignation — {}", side(winner)),
            Outcome::Stalemate => "Stalemate — draw.".into(),
            Outcome::FiftyMoveRule => "Draw by the fifty-move rule.".into(),
            Outcome::ThreefoldRepetition => "Draw by threefold repetition.".into(),
            Outcome::InsufficientMaterial => "Draw — insufficient material.".into(),
        }
    }
}

/// Whether the input loop should keep going.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flow {
    Continue,
    Quit,
}

pub struct Game {
    position: Position,
    search: Search,
    /// The colour the human is playing.
    human: Color,
    movetime: u64,
    depth: Option<u32>,
    /// Board orientation; follows the human's colour unless flipped by hand.
    flipped: bool,
    /// Position hashes reached so far, for threefold detection.
    seen: Vec<u64>,
    /// Every move played, in SAN, for display.
    history_san: Vec<String>,
    /// Plies played from the starting position, i.e. how much can be undone.
    plies: usize,
    last_move: Option<Move>,
    resigned: Option<Color>,
}

/// A move the engine chose, with the numbers behind it.
#[derive(Clone, Debug)]
pub struct EngineMove {
    pub san: String,
    pub uci: String,
    /// Score in centipawns from white's point of view.
    pub score_cp: i32,
    /// The same score written for a human: `+0.48` or `mate in 3 for white`.
    pub score: String,
    pub depth: u32,
    pub nodes: u64,
    pub millis: u128,
}

impl Game {
    pub fn new(human: Color) -> Game {
        crate::init();
        let mut search = Search::new(HASH_MB, Arc::new(AtomicBool::new(false)));
        // The engine reports its own one-line summary instead of UCI info.
        search.silent = true;
        let position = Position::startpos();
        Game {
            seen: vec![position.hash],
            position,
            search,
            human,
            movetime: DEFAULT_MOVETIME_MS,
            depth: None,
            flipped: human == Color::Black,
            history_san: Vec::new(),
            plies: 0,
            last_move: None,
            resigned: None,
        }
    }

    // --- I/O-free game operations, shared by the terminal and web front-ends.

    /// Plays a move written in SAN or UCI. Returns the move in SAN, or why it
    /// was refused.
    pub fn play_move(&mut self, text: &str) -> Result<String, String> {
        if self.outcome().is_some() {
            return Err("the game is over".into());
        }
        if self.is_engines_turn() {
            return Err("it is the engine's turn".into());
        }
        match san::parse(&mut self.position, text) {
            Some(mv) => Ok(self.apply(mv)),
            None => Err(format!("'{text}' is not a legal move here")),
        }
    }

    /// Searches, plays the engine's move, and reports what it found.
    pub fn engine_reply(&mut self) -> Option<EngineMove> {
        if self.outcome().is_some() {
            return None;
        }
        let limits = match self.depth {
            Some(depth) => SearchLimits::fixed_depth(depth),
            None => SearchLimits::fixed_time(self.movetime),
        };
        let start = Instant::now();
        let result = self.search.think(&mut self.position, &limits);
        let millis = start.elapsed().as_millis();
        if result.best_move.is_none() {
            return None;
        }

        let white_pov = if self.position.side_to_move == Color::White {
            result.score
        } else {
            -result.score
        };
        let score = format_score(result.score, self.position.side_to_move);
        let uci = result.best_move.to_string();
        let san = self.apply(result.best_move);
        Some(EngineMove {
            san,
            uci,
            score_cp: white_pov,
            score,
            depth: result.depth,
            nodes: result.nodes,
            millis,
        })
    }

    /// What the engine would play, without playing it.
    pub fn suggestion(&mut self) -> Option<EngineMove> {
        if self.outcome().is_some() {
            return None;
        }
        let limits = SearchLimits::fixed_time(self.movetime.min(2000));
        let start = Instant::now();
        let result = self.search.think(&mut self.position, &limits);
        if result.best_move.is_none() {
            return None;
        }
        let white_pov = if self.position.side_to_move == Color::White {
            result.score
        } else {
            -result.score
        };
        Some(EngineMove {
            san: san::to_san(&mut self.position, result.best_move),
            uci: result.best_move.to_string(),
            score_cp: white_pov,
            score: format_score(result.score, self.position.side_to_move),
            depth: result.depth,
            nodes: result.nodes,
            millis: start.elapsed().as_millis(),
        })
    }

    /// Takes back the human's last move and the engine's reply. Returns how
    /// many half-moves were undone.
    pub fn take_back(&mut self) -> usize {
        if self.plies == 0 {
            return 0;
        }
        let count = if self.plies >= 2 && !self.is_engines_turn() {
            2
        } else {
            1
        };
        for _ in 0..count {
            self.position.unmake_move();
            self.seen.pop();
            self.history_san.pop();
            self.plies -= 1;
        }
        self.resigned = None;
        self.last_move = None;
        count
    }

    /// Replaces the board with a FEN position, keeping the current settings.
    pub fn set_position(&mut self, fen: &str) -> Result<(), String> {
        let position = Position::from_fen(fen)?;
        self.position = position;
        self.seen = vec![self.position.hash];
        self.history_san.clear();
        self.plies = 0;
        self.last_move = None;
        self.resigned = None;
        self.search.reset();
        Ok(())
    }

    pub fn resign(&mut self) {
        self.resigned = Some(self.human);
    }

    // --- Accessors used by the front-ends.

    pub fn fen(&self) -> String {
        self.position.to_fen()
    }

    pub fn side_to_move(&self) -> Color {
        self.position.side_to_move
    }

    pub fn human(&self) -> Color {
        self.human
    }

    pub fn in_check(&self) -> bool {
        self.position.in_check()
    }

    pub fn last_move(&self) -> Option<Move> {
        self.last_move
    }

    pub fn history_san(&self) -> &[String] {
        &self.history_san
    }

    /// Every legal move in UCI form, for a front-end to highlight.
    pub fn legal_uci(&self) -> Vec<String> {
        let mut probe = self.position.clone();
        generate_legal_moves(&mut probe)
            .iter()
            .map(|mv| mv.to_string())
            .collect()
    }

    /// Material difference in pawns, from white's point of view.
    pub fn material_balance(&self) -> i32 {
        material_balance(&self.position)
    }

    pub fn movetime(&self) -> u64 {
        self.movetime
    }

    pub fn set_movetime(&mut self, millis: u64) {
        self.movetime = millis.clamp(10, 60_000);
        self.depth = None;
    }

    /// Plays a whole game against stdin until the user quits or the game ends.
    pub fn run(&mut self) {
        println!("{}", banner(self.human));
        println!("{}", self.render());

        let stdin = std::io::stdin();
        let mut lines = stdin.lock().lines();
        loop {
            if self.is_engines_turn() && self.outcome().is_none() {
                self.play_engine_move();
                println!("{}", self.render());
            }
            if let Some(outcome) = self.outcome() {
                println!("{}", outcome.describe(self.human));
                return;
            }

            print!("your move> ");
            let _ = std::io::stdout().flush();
            let Some(Ok(line)) = lines.next() else { return };
            if self.execute(line.trim()) == Flow::Quit {
                return;
            }
        }
    }

    /// Runs one user command: a move, or one of the helper commands.
    pub fn execute(&mut self, line: &str) -> Flow {
        let mut parts = line.split_whitespace();
        let Some(command) = parts.next() else {
            return Flow::Continue;
        };
        let rest: Vec<&str> = parts.collect();

        match command.to_ascii_lowercase().as_str() {
            "quit" | "exit" | "q" => return Flow::Quit,
            "help" | "?" => println!("{HELP}"),
            "board" | "d" => println!("{}", self.render()),
            "moves" => self.list_moves(),
            "fen" => {
                if rest.is_empty() {
                    println!("{}", self.position.to_fen());
                } else {
                    self.set_fen(&rest.join(" "));
                }
            }
            "eval" => {
                let score = eval::evaluate(&self.position);
                println!(
                    "static evaluation: {}",
                    format_score(score, self.position.side_to_move)
                );
            }
            "hint" => self.hint(),
            "undo" => self.undo(),
            "new" => {
                let color = rest
                    .first()
                    .and_then(|c| parse_color(c))
                    .unwrap_or(self.human);
                *self = Game::new(color);
                println!("{}", self.render());
            }
            "flip" => {
                self.flipped = !self.flipped;
                println!("{}", self.render());
            }
            "time" => match rest.first().and_then(|v| v.parse::<u64>().ok()) {
                Some(ms) => {
                    self.movetime = ms.max(10);
                    self.depth = None;
                    println!("engine will think for {} ms per move", self.movetime);
                }
                None => println!("usage: time <milliseconds>"),
            },
            "depth" => match rest.first().and_then(|v| v.parse::<u32>().ok()) {
                Some(d) => {
                    self.depth = Some(d.clamp(1, 100));
                    println!("engine will search to depth {}", self.depth.unwrap());
                }
                None => println!("usage: depth <plies>"),
            },
            "resign" => {
                self.resign();
                println!("You resign.");
            }
            _ => self.try_move(line),
        }
        Flow::Continue
    }

    fn try_move(&mut self, text: &str) {
        match self.play_move(text) {
            Ok(san) => println!("You play {san}"),
            Err(why) if why.starts_with('\'') => {
                println!("{why} — try 'moves' for the list")
            }
            Err(why) => println!("{why}"),
        }
    }

    /// Searches and plays the engine's reply, reporting it on stdout.
    pub fn play_engine_move(&mut self) {
        let Some(played) = self.engine_reply() else {
            return;
        };
        println!(
            "Engine plays {}   [{}, depth {}, {:.1}s, {} nodes]",
            played.san,
            played.score,
            played.depth,
            played.millis as f64 / 1000.0,
            played.nodes
        );
    }

    #[inline]
    pub fn is_engines_turn(&self) -> bool {
        self.position.side_to_move != self.human
    }

    /// The result of the game, or `None` if it is still going.
    pub fn outcome(&self) -> Option<Outcome> {
        if let Some(loser) = self.resigned {
            return Some(Outcome::Resignation {
                winner: loser.flip(),
            });
        }
        let mut probe = self.position.clone();
        if generate_legal_moves(&mut probe).is_empty() {
            return Some(if self.position.in_check() {
                Outcome::Checkmate {
                    winner: self.position.side_to_move.flip(),
                }
            } else {
                Outcome::Stalemate
            });
        }
        if self.position.halfmove_clock >= 100 {
            return Some(Outcome::FiftyMoveRule);
        }
        // The search treats one repetition as a draw; a real game needs three.
        if self
            .seen
            .iter()
            .filter(|&&hash| hash == self.position.hash)
            .count()
            >= 3
        {
            return Some(Outcome::ThreefoldRepetition);
        }
        if self.position.is_insufficient_material() {
            return Some(Outcome::InsufficientMaterial);
        }
        None
    }

    /// Makes a move and records it. Returns the move in SAN.
    fn apply(&mut self, mv: Move) -> String {
        let san = san::to_san(&mut self.position, mv);
        self.position.make_move(mv);
        self.seen.push(self.position.hash);
        self.history_san.push(san.clone());
        self.plies += 1;
        self.last_move = Some(mv);
        san
    }

    fn undo(&mut self) {
        match self.take_back() {
            0 => println!("nothing to undo"),
            count => {
                println!("took back {count} half-move(s)");
                println!("{}", self.render());
            }
        }
    }

    fn hint(&mut self) {
        match self.suggestion() {
            Some(best) => {
                println!(
                    "suggestion: {}   [{}, depth {}]",
                    best.san, best.score, best.depth
                )
            }
            None => println!("the game is over"),
        }
    }

    fn list_moves(&mut self) {
        let legal = generate_legal_moves(&mut self.position);
        let mut names: Vec<String> = legal
            .iter()
            .map(|&mv| san::to_san(&mut self.position, mv))
            .collect();
        names.sort();
        println!("{} legal moves:", names.len());
        for chunk in names.chunks(8) {
            println!("  {}", chunk.join("  "));
        }
    }

    fn set_fen(&mut self, fen: &str) {
        match self.set_position(fen) {
            Ok(()) => println!("{}", self.render()),
            Err(err) => println!("bad FEN: {err}"),
        }
    }

    /// The board, drawn from the side the human is playing.
    pub fn render(&self) -> String {
        let mut out = String::new();
        let ranks: Vec<u8> = if self.flipped {
            (0..8).collect()
        } else {
            (0..8).rev().collect()
        };
        let files: Vec<u8> = if self.flipped {
            (0..8).rev().collect()
        } else {
            (0..8).collect()
        };

        out.push_str("\n     +------------------------+\n");
        for &rank in &ranks {
            out.push_str(&format!("   {} |", rank + 1));
            for &file in &files {
                let sq = square_of(file, rank);
                let marker = match self.last_move {
                    // Bracket the square the last move came from and went to.
                    Some(mv) if mv.to() == sq => '[',
                    Some(mv) if mv.from() == sq => '(',
                    _ => ' ',
                };
                let symbol = match self.position.piece_at(sq) {
                    Some(piece) => piece.to_char(),
                    None => '.',
                };
                let close = match self.last_move {
                    Some(mv) if mv.to() == sq => ']',
                    Some(mv) if mv.from() == sq => ')',
                    _ => ' ',
                };
                out.push(marker);
                out.push(symbol);
                out.push(close);
            }
            out.push_str("|\n");
        }
        out.push_str("     +------------------------+\n      ");
        for &file in &files {
            out.push_str(&format!(" {} ", (b'a' + file) as char));
        }
        out.push('\n');

        let balance = material_balance(&self.position);
        out.push_str(&format!(
            "\n   move {}, {} to play{}{}\n",
            self.position.fullmove_number,
            if self.position.side_to_move == Color::White {
                "white"
            } else {
                "black"
            },
            if self.position.in_check() {
                " — check!"
            } else {
                ""
            },
            match balance {
                0 => String::new(),
                n if n > 0 => format!("   (white +{n})"),
                n => format!("   (black +{})", -n),
            }
        ));
        out
    }
}

/// Material difference in pawns, from white's point of view.
fn material_balance(pos: &Position) -> i32 {
    let mut balance = 0;
    for sq in 0..64u8 {
        if let Some(piece) = pos.piece_at(sq as Square) {
            let value = match piece.piece_type {
                PieceType::Pawn => 1,
                PieceType::Knight | PieceType::Bishop => 3,
                PieceType::Rook => 5,
                PieceType::Queen => 9,
                PieceType::King => 0,
            };
            balance += if piece.color == Color::White {
                value
            } else {
                -value
            };
        }
    }
    balance
}

/// Formats a search score from white's point of view, in pawns.
fn format_score(score: i32, side_to_move: Color) -> String {
    let white_score = if side_to_move == Color::White {
        score
    } else {
        -score
    };
    if white_score.abs() >= MATE_IN_MAX_PLY {
        let moves = (MATE - white_score.abs() + 1) / 2;
        let side = if white_score > 0 { "white" } else { "black" };
        return format!("mate in {moves} for {side}");
    }
    format!("{:+.2}", white_score as f64 / 100.0)
}

fn parse_color(text: &str) -> Option<Color> {
    match text.to_ascii_lowercase().as_str() {
        "w" | "white" => Some(Color::White),
        "b" | "black" => Some(Color::Black),
        _ => None,
    }
}

/// Parses the command-line arguments after `play`: an optional colour and an
/// optional per-move thinking time in milliseconds.
pub fn start(args: &[String]) {
    let mut human = Color::White;
    let mut movetime = DEFAULT_MOVETIME_MS;
    for arg in args {
        if let Some(color) = parse_color(arg) {
            human = color;
        } else if let Ok(ms) = arg.parse::<u64>() {
            movetime = ms.max(10);
        }
    }
    let mut game = Game::new(human);
    game.movetime = movetime;
    game.run();
}

fn banner(human: Color) -> String {
    format!(
        "\nYou are {}. Enter moves as e4, Nf3, exd5, O-O or e2e4.\n\
         Type 'help' for commands.",
        if human == Color::White {
            "white"
        } else {
            "black"
        }
    )
}

const HELP: &str = "\
  <move>        play a move: e4, Nf3, exd5, O-O, e8=Q, or e2e4
  moves         list every legal move
  hint          ask the engine what it would play
  undo          take back your last move (and the engine's reply)
  board / flip  redraw the board / swap the orientation
  eval          static evaluation of this position, no search
  fen [<fen>]   show the current FEN, or set up a position
  time <ms>     engine thinking time per move (default 1000)
  depth <n>     search a fixed depth instead of a fixed time
  new [w|b]     start a new game, optionally choosing your colour
  resign        concede the game
  quit          leave";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::STARTPOS_FEN;

    fn game_at(fen: &str, human: Color) -> Game {
        let mut game = Game::new(human);
        game.execute(&format!("fen {fen}"));
        game
    }

    #[test]
    fn accepts_moves_in_san_and_uci() {
        let mut game = Game::new(Color::White);
        game.execute("e4");
        assert_eq!(
            game.position.to_fen(),
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1"
        );
        assert_eq!(game.plies, 1);
        assert!(game.is_engines_turn());

        let mut game = Game::new(Color::White);
        game.execute("g1f3");
        assert_eq!(game.plies, 1);
    }

    #[test]
    fn rejects_illegal_moves_without_changing_the_board() {
        let mut game = Game::new(Color::White);
        for text in ["e5", "Nf6", "xyzzy", "e2e5"] {
            game.execute(text);
        }
        assert_eq!(game.position.to_fen(), STARTPOS_FEN);
        assert_eq!(game.plies, 0);
    }

    #[test]
    fn will_not_move_out_of_turn() {
        let mut game = Game::new(Color::Black);
        game.execute("e4"); // White is the engine's; the human plays black.
        assert_eq!(game.position.to_fen(), STARTPOS_FEN);
        assert!(game.is_engines_turn());
    }

    #[test]
    fn engine_replies_with_a_legal_move() {
        let mut game = Game::new(Color::White);
        game.execute("e4");
        game.play_engine_move();
        assert_eq!(game.plies, 2);
        assert!(!game.is_engines_turn());
        // The position must still be reachable and consistent.
        let rebuilt = Position::from_fen(&game.position.to_fen()).unwrap();
        assert_eq!(rebuilt.hash, game.position.hash);
    }

    #[test]
    fn undo_takes_back_a_full_move() {
        let mut game = Game::new(Color::White);
        game.execute("e4");
        game.play_engine_move();
        game.execute("undo");
        assert_eq!(game.plies, 0);
        assert_eq!(game.position.to_fen(), STARTPOS_FEN);
        assert!(!game.is_engines_turn());
        game.execute("undo"); // Nothing left; must not panic.
        assert_eq!(game.plies, 0);
    }

    #[test]
    fn detects_how_the_game_ended() {
        // Fool's mate, with black to move and mated.
        let game = game_at(
            "rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3",
            Color::White,
        );
        assert_eq!(
            game.outcome(),
            Some(Outcome::Checkmate {
                winner: Color::Black
            })
        );

        let game = game_at("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1", Color::White);
        assert_eq!(game.outcome(), Some(Outcome::Stalemate));

        let game = game_at("4k3/8/8/8/8/8/8/4K3 w - - 0 1", Color::White);
        assert_eq!(game.outcome(), Some(Outcome::InsufficientMaterial));

        let mut game = game_at("4k3/8/8/8/8/8/4P3/4K3 w - - 99 60", Color::White);
        game.execute("Kd1"); // A quiet move takes the clock to 100.
        assert_eq!(game.outcome(), Some(Outcome::FiftyMoveRule));

        let mut game = Game::new(Color::White);
        assert_eq!(game.outcome(), None);
        game.execute("resign");
        assert_eq!(
            game.outcome(),
            Some(Outcome::Resignation {
                winner: Color::Black
            })
        );
    }

    #[test]
    fn detects_threefold_repetition_but_not_twofold() {
        // Drives both sides directly: `execute` deliberately refuses to move
        // for the engine, which `will_not_move_out_of_turn` covers.
        fn play(game: &mut Game, text: &str) {
            let mv = san::parse(&mut game.position, text).expect("legal move");
            game.apply(mv);
        }

        let mut game = Game::new(Color::White);
        for text in ["Nf3", "Nf6", "Ng1", "Ng8"] {
            play(&mut game, text);
        }
        assert_eq!(game.outcome(), None, "twofold is not yet a draw");
        for text in ["Nf3", "Nf6", "Ng1", "Ng8"] {
            play(&mut game, text);
        }
        assert_eq!(game.outcome(), Some(Outcome::ThreefoldRepetition));
    }

    #[test]
    fn commands_do_not_disturb_the_position() {
        let mut game = Game::new(Color::White);
        game.execute("e4");
        let fen = game.position.to_fen();
        for command in [
            "board", "moves", "eval", "fen", "flip", "help", "time 50", "depth 4",
        ] {
            game.execute(command);
        }
        assert_eq!(game.position.to_fen(), fen);
        assert_eq!(game.depth, Some(4));
        assert_eq!(game.movetime, 50);
    }

    #[test]
    fn quit_stops_the_loop() {
        let mut game = Game::new(Color::White);
        assert_eq!(game.execute("board"), Flow::Continue);
        assert_eq!(game.execute("quit"), Flow::Quit);
    }

    #[test]
    fn new_game_can_switch_colours() {
        let mut game = Game::new(Color::White);
        game.execute("e4");
        game.execute("new black");
        assert_eq!(game.human, Color::Black);
        assert_eq!(game.plies, 0);
        assert!(game.is_engines_turn());
        assert!(game.flipped);
    }

    #[test]
    fn engine_finds_mate_when_offered_one() {
        let mut game = game_at("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1", Color::Black);
        game.play_engine_move();
        assert_eq!(
            game.outcome(),
            Some(Outcome::Checkmate {
                winner: Color::White
            })
        );
    }

    #[test]
    fn board_renders_both_orientations() {
        let game = Game::new(Color::White);
        let white_view = game.render();
        // The rank nearest the reader is white's back rank.
        let lines: Vec<&str> = white_view.lines().filter(|l| l.contains('|')).collect();
        assert!(lines[0].contains('r'), "black pieces on top: {}", lines[0]);
        assert!(
            lines[7].contains('R'),
            "white pieces at the bottom: {}",
            lines[7]
        );

        let black_view = Game::new(Color::Black).render();
        let lines: Vec<&str> = black_view.lines().filter(|l| l.contains('|')).collect();
        assert!(lines[0].contains('R'), "flipped for black: {}", lines[0]);
    }
}
