//! UCI protocol front-end.
//!
//! The search runs on its own thread so `stop` stays responsive during an
//! infinite or long search; the thread hands the search state (including the
//! transposition table) back when it finishes.

use std::io::{BufRead, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use crate::eval;
use crate::perft;
use crate::position::{Position, STARTPOS_FEN};
use crate::search::{Search, SearchLimits};
use crate::types::Color;

pub const ENGINE_NAME: &str = concat!("Rustic-", env!("CARGO_PKG_VERSION"));
pub const ENGINE_AUTHOR: &str = "chess-engine";

const DEFAULT_HASH_MB: usize = 64;
/// Search threads get a large stack; the search recurses up to MAX_PLY deep
/// with a move list and ordering scores in every frame.
const SEARCH_STACK_SIZE: usize = 16 * 1024 * 1024;

pub struct Uci {
    position: Position,
    /// Held here between searches, moved into the search thread during one.
    search: Option<Box<Search>>,
    handle: Option<JoinHandle<Box<Search>>>,
    stop: Arc<AtomicBool>,
    hash_mb: usize,
}

impl Uci {
    pub fn new() -> Uci {
        let stop = Arc::new(AtomicBool::new(false));
        Uci {
            position: Position::startpos(),
            search: Some(Box::new(Search::new(DEFAULT_HASH_MB, stop.clone()))),
            handle: None,
            stop,
            hash_mb: DEFAULT_HASH_MB,
        }
    }

    /// Reads commands until `quit` or end of input.
    pub fn run(&mut self) {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            if !self.execute(line.trim()) {
                break;
            }
        }
        self.stop_search();
    }

    /// Handles one command. Returns false when the engine should exit.
    pub fn execute(&mut self, line: &str) -> bool {
        let mut parts = line.split_whitespace();
        let Some(command) = parts.next() else {
            return true;
        };
        let args: Vec<&str> = parts.collect();

        match command {
            "uci" => {
                println!("id name {ENGINE_NAME}");
                println!("id author {ENGINE_AUTHOR}");
                println!("option name Hash type spin default {DEFAULT_HASH_MB} min 1 max 4096");
                println!("uciok");
            }
            "isready" => println!("readyok"),
            "ucinewgame" => {
                self.stop_search();
                if let Some(search) = self.search.as_mut() {
                    search.reset();
                }
                self.position = Position::startpos();
            }
            "setoption" => self.set_option(&args),
            "position" => self.set_position(&args),
            "go" => self.go(&args),
            "stop" => self.stop_search(),
            "ponderhit" => {}
            "quit" => return false,
            // Non-standard helpers, useful when driving the engine by hand.
            "d" | "display" => println!("{}", self.position),
            "eval" => println!("{}", eval::format_score(eval::evaluate(&self.position))),
            "perft" => self.perft(&args),
            "bench" => {
                let depth = args.first().and_then(|d| d.parse().ok()).unwrap_or(8);
                crate::search::bench(depth);
            }
            _ => println!("info string unknown command '{command}'"),
        }
        let _ = std::io::stdout().flush();
        true
    }

    fn set_option(&mut self, args: &[&str]) {
        // Format: setoption name <id> [value <x>]
        let name = args
            .iter()
            .position(|&a| a.eq_ignore_ascii_case("name"))
            .map(|i| {
                let end = args
                    .iter()
                    .position(|&a| a.eq_ignore_ascii_case("value"))
                    .unwrap_or(args.len());
                args[i + 1..end].join(" ")
            })
            .unwrap_or_default();
        let value = args
            .iter()
            .position(|&a| a.eq_ignore_ascii_case("value"))
            .map(|i| args[i + 1..].join(" "))
            .unwrap_or_default();

        match name.to_ascii_lowercase().as_str() {
            "hash" => {
                if let Ok(mb) = value.trim().parse::<usize>() {
                    self.stop_search();
                    self.hash_mb = mb.clamp(1, 4096);
                    if let Some(search) = self.search.as_mut() {
                        search.tt.resize(self.hash_mb);
                    }
                }
            }
            "clear hash" => {
                self.stop_search();
                if let Some(search) = self.search.as_mut() {
                    search.tt.clear();
                }
            }
            _ => println!("info string unknown option '{name}'"),
        }
    }

    fn set_position(&mut self, args: &[&str]) {
        self.stop_search();
        let moves_at = args
            .iter()
            .position(|&a| a == "moves")
            .unwrap_or(args.len());
        let (setup, moves) = args.split_at(moves_at);

        let position = match setup.first() {
            Some(&"startpos") | None => Position::from_fen(STARTPOS_FEN),
            Some(&"fen") => Position::from_fen(&setup[1..].join(" ")),
            Some(other) => Err(format!("unknown position type '{other}'")),
        };
        let mut position = match position {
            Ok(position) => position,
            Err(err) => {
                println!("info string invalid position: {err}");
                return;
            }
        };

        for text in moves.iter().skip(1) {
            match position.parse_uci_move(text) {
                Some(mv) => position.make_move(mv),
                None => {
                    println!("info string illegal move '{text}'");
                    return;
                }
            }
        }
        self.position = position;
    }

    fn go(&mut self, args: &[&str]) {
        self.stop_search();
        let mut limits = SearchLimits::default();
        let mut i = 0;
        while i < args.len() {
            let value = || args.get(i + 1).and_then(|v| v.parse::<u64>().ok());
            match args[i] {
                "depth" => {
                    if let Some(v) = value() {
                        limits.depth = v as u32;
                    }
                }
                "movetime" => limits.movetime = value(),
                "wtime" => limits.time[Color::White.index()] = value(),
                "btime" => limits.time[Color::Black.index()] = value(),
                "winc" => limits.increment[Color::White.index()] = value().unwrap_or(0),
                "binc" => limits.increment[Color::Black.index()] = value().unwrap_or(0),
                "movestogo" => limits.moves_to_go = value().map(|v| v as u32),
                "nodes" => limits.nodes = value(),
                "infinite" => limits.infinite = true,
                _ => {}
            }
            i += 1;
        }

        let Some(mut search) = self.search.take() else {
            println!("info string search already running");
            return;
        };
        self.stop.store(false, Ordering::Relaxed);
        let mut position = self.position.clone();

        let handle = std::thread::Builder::new()
            .stack_size(SEARCH_STACK_SIZE)
            .spawn(move || {
                let result = search.think(&mut position, &limits);
                match result.ponder_move {
                    Some(ponder) => println!("bestmove {} ponder {ponder}", result.best_move),
                    None => println!("bestmove {}", result.best_move),
                }
                let _ = std::io::stdout().flush();
                search
            })
            .expect("failed to spawn search thread");
        self.handle = Some(handle);
    }

    /// Signals the running search and reclaims its state. Safe to call when
    /// nothing is running.
    fn stop_search(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            match handle.join() {
                Ok(search) => self.search = Some(search),
                Err(_) => {
                    // The search thread panicked; rebuild rather than wedge.
                    println!("info string search thread failed, resetting");
                    self.search = Some(Box::new(Search::new(self.hash_mb, self.stop.clone())));
                }
            }
        }
        self.stop.store(false, Ordering::Relaxed);
    }

    fn perft(&mut self, args: &[&str]) {
        let depth = args.first().and_then(|d| d.parse().ok()).unwrap_or(5);
        let start = std::time::Instant::now();
        let (divide, total) = perft::perft_divide(&mut self.position, depth);
        for (mv, nodes) in divide {
            println!("{mv}: {nodes}");
        }
        let millis = start.elapsed().as_millis().max(1);
        println!(
            "\nnodes {total} time {millis}ms ({} nps)",
            total * 1000 / millis as u64
        );
    }
}

impl Default for Uci {
    fn default() -> Uci {
        Uci::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_command_sets_up_the_board() {
        let mut uci = Uci::new();
        uci.execute("position startpos moves e2e4 e7e5 g1f3");
        assert_eq!(
            uci.position.to_fen(),
            "rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2"
        );

        let fen = "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1";
        uci.execute(&format!("position fen {fen}"));
        assert_eq!(uci.position.to_fen(), fen);

        uci.execute("position fen 4k3/8/8/8/8/8/8/4K3 w - - 0 1 moves e1e2");
        assert_eq!(uci.position.to_fen(), "4k3/8/8/8/8/8/4K3/8 b - - 1 1");
    }

    #[test]
    fn illegal_input_leaves_the_position_alone() {
        let mut uci = Uci::new();
        uci.execute("position startpos moves e2e4 e2e4");
        assert_eq!(uci.position.to_fen(), STARTPOS_FEN);
        uci.execute("position fen total nonsense");
        assert_eq!(uci.position.to_fen(), STARTPOS_FEN);
    }

    #[test]
    fn go_produces_a_best_move_and_returns_state() {
        crate::init();
        let mut uci = Uci::new();
        uci.execute("position startpos");
        uci.execute("go depth 4");
        uci.stop_search();
        // The search state must come back so the next `go` can run.
        assert!(uci.search.is_some());
    }

    #[test]
    fn setoption_resizes_the_hash() {
        let mut uci = Uci::new();
        uci.execute("setoption name Hash value 8");
        assert_eq!(uci.hash_mb, 8);
        uci.execute("setoption name Hash value 99999");
        assert_eq!(uci.hash_mb, 4096);
    }

    #[test]
    fn quit_stops_the_loop() {
        let mut uci = Uci::new();
        assert!(uci.execute("uci"));
        assert!(uci.execute("isready"));
        assert!(!uci.execute("quit"));
    }
}
