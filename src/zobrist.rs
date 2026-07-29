//! Zobrist hashing keys, generated once from a fixed seed.

use std::sync::LazyLock;

use crate::types::{Color, Piece, Square};

pub struct Keys {
    pub pieces: [[u64; 64]; 12],
    pub castling: [u64; 16],
    pub en_passant: [u64; 8],
    pub side_to_move: u64,
}

static KEYS: LazyLock<Keys> = LazyLock::new(|| {
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move || {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    };

    let mut pieces = [[0u64; 64]; 12];
    for piece in pieces.iter_mut() {
        for sq in piece.iter_mut() {
            *sq = next();
        }
    }
    let mut castling = [0u64; 16];
    for key in castling.iter_mut() {
        *key = next();
    }
    let mut en_passant = [0u64; 8];
    for key in en_passant.iter_mut() {
        *key = next();
    }
    Keys {
        pieces,
        castling,
        en_passant,
        side_to_move: next(),
    }
});

#[inline]
pub fn piece(p: Piece, sq: Square) -> u64 {
    KEYS.pieces[p.index()][sq as usize]
}

#[inline]
pub fn castling(rights: u8) -> u64 {
    KEYS.castling[rights as usize]
}

/// Keyed by file only; the rank is implied by the side to move.
#[inline]
pub fn en_passant(file: u8) -> u64 {
    KEYS.en_passant[file as usize]
}

/// Mixed in when it is black's turn.
#[inline]
pub fn side_to_move() -> u64 {
    KEYS.side_to_move
}

#[inline]
pub fn side_key(color: Color) -> u64 {
    match color {
        Color::White => 0,
        Color::Black => side_to_move(),
    }
}
