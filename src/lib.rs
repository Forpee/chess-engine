//! A UCI chess engine: bitboard board representation, magic-bitboard move
//! generation and an alpha-beta search with a transposition table.

pub mod attacks;
pub mod bitboard;
pub mod eval;
pub mod movegen;
pub mod perft;
pub mod play;
pub mod position;
pub mod san;
pub mod search;
pub mod server;
pub mod tt;
pub mod types;
pub mod uci;
pub mod zobrist;

pub use position::Position;
pub use types::{Color, Move, Piece, PieceType, Square};

/// Builds the attack and hash tables up front. Cheap, but not free, so callers
/// that care about first-search latency should call it at startup.
pub fn init() {
    attacks::init();
}
