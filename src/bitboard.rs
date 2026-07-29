//! Bitboard helpers. A `Bitboard` is a u64 where bit `n` is square `n`.

use crate::types::Square;

pub type Bitboard = u64;

pub const EMPTY: Bitboard = 0;
pub const FULL: Bitboard = !0;

pub const FILE_A: Bitboard = 0x0101_0101_0101_0101;
pub const FILE_B: Bitboard = FILE_A << 1;
pub const FILE_G: Bitboard = FILE_A << 6;
pub const FILE_H: Bitboard = FILE_A << 7;

pub const RANK_1: Bitboard = 0xff;
pub const RANK_2: Bitboard = RANK_1 << 8;
pub const RANK_4: Bitboard = RANK_1 << 24;
pub const RANK_5: Bitboard = RANK_1 << 32;
pub const RANK_7: Bitboard = RANK_1 << 48;
pub const RANK_8: Bitboard = RANK_1 << 56;

pub const FILES: [Bitboard; 8] = [
    FILE_A,
    FILE_A << 1,
    FILE_A << 2,
    FILE_A << 3,
    FILE_A << 4,
    FILE_A << 5,
    FILE_A << 6,
    FILE_A << 7,
];

pub const RANKS: [Bitboard; 8] = [
    RANK_1,
    RANK_1 << 8,
    RANK_1 << 16,
    RANK_1 << 24,
    RANK_1 << 32,
    RANK_1 << 40,
    RANK_1 << 48,
    RANK_1 << 56,
];

#[inline]
pub const fn bb(sq: Square) -> Bitboard {
    1u64 << sq
}

#[inline]
pub const fn contains(b: Bitboard, sq: Square) -> bool {
    b & bb(sq) != 0
}

#[inline]
pub const fn lsb(b: Bitboard) -> Square {
    b.trailing_zeros() as Square
}

#[inline]
pub const fn popcount(b: Bitboard) -> u32 {
    b.count_ones()
}

/// Removes and returns the least significant set bit's square.
#[inline]
pub fn pop_lsb(b: &mut Bitboard) -> Square {
    let sq = lsb(*b);
    *b &= *b - 1;
    sq
}

#[inline]
pub const fn north(b: Bitboard) -> Bitboard {
    b << 8
}

#[inline]
pub const fn south(b: Bitboard) -> Bitboard {
    b >> 8
}

#[inline]
pub const fn east(b: Bitboard) -> Bitboard {
    (b & !FILE_H) << 1
}

#[inline]
pub const fn west(b: Bitboard) -> Bitboard {
    (b & !FILE_A) >> 1
}

/// Iterates the set squares of a bitboard, low to high.
pub struct BitIter(pub Bitboard);

impl Iterator for BitIter {
    type Item = Square;

    #[inline]
    fn next(&mut self) -> Option<Square> {
        if self.0 == 0 {
            None
        } else {
            Some(pop_lsb(&mut self.0))
        }
    }
}

#[inline]
pub fn iter_bits(b: Bitboard) -> BitIter {
    BitIter(b)
}

/// Renders a bitboard as an 8x8 grid, rank 8 at the top. Debugging aid.
pub fn to_string(b: Bitboard) -> String {
    let mut s = String::new();
    for rank in (0..8).rev() {
        for file in 0..8 {
            s.push(if contains(b, rank * 8 + file) {
                'X'
            } else {
                '.'
            });
            s.push(' ');
        }
        s.push('\n');
    }
    s
}
