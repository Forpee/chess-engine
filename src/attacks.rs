//! Precomputed attack tables.
//!
//! Leapers (pawn, knight, king) are simple lookups. Sliders use fancy magic
//! bitboards: the magics are searched for once at startup with a fixed-seed
//! PRNG, which keeps the source free of multi-kilobyte constant tables while
//! staying fully deterministic.

use std::sync::LazyLock;

use crate::bitboard::*;
use crate::types::{Color, Square, file_of, rank_of};

const ROOK_DIRS: [(i8, i8); 4] = [(0, 1), (0, -1), (1, 0), (-1, 0)];
const BISHOP_DIRS: [(i8, i8); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];

/// One square's magic hashing parameters into the shared attack table.
#[derive(Clone, Copy, Default)]
struct Magic {
    mask: Bitboard,
    magic: u64,
    shift: u32,
    offset: usize,
}

impl Magic {
    #[inline]
    fn index(&self, occupancy: Bitboard) -> usize {
        self.offset + (((occupancy & self.mask).wrapping_mul(self.magic)) >> self.shift) as usize
    }
}

struct Tables {
    pawn: [[Bitboard; 64]; 2],
    knight: [Bitboard; 64],
    king: [Bitboard; 64],
    rook: [Magic; 64],
    bishop: [Magic; 64],
    slider_attacks: Vec<Bitboard>,
    /// Squares strictly between two squares on a shared line, else empty.
    between: [[Bitboard; 64]; 64],
}

static TABLES: LazyLock<Tables> = LazyLock::new(Tables::new);

/// Walks each direction from `sq` until the board edge or a blocker (inclusive).
fn sliding_attacks(sq: Square, occupancy: Bitboard, dirs: &[(i8, i8); 4]) -> Bitboard {
    let mut attacks = EMPTY;
    for &(df, dr) in dirs {
        let mut f = file_of(sq) as i8;
        let mut r = rank_of(sq) as i8;
        loop {
            f += df;
            r += dr;
            if !(0..8).contains(&f) || !(0..8).contains(&r) {
                break;
            }
            let target = (r * 8 + f) as Square;
            attacks |= bb(target);
            if occupancy & bb(target) != 0 {
                break;
            }
        }
    }
    attacks
}

/// Relevant-occupancy mask: the attack set minus the edges, which can never
/// block anything behind them.
fn relevant_mask(sq: Square, dirs: &[(i8, i8); 4]) -> Bitboard {
    let edges = ((FILE_A | FILE_H) & !FILES[file_of(sq) as usize])
        | ((RANK_1 | RANK_8) & !RANKS[rank_of(sq) as usize]);
    sliding_attacks(sq, EMPTY, dirs) & !edges
}

/// Enumerates the subsets of `mask` (Carry-Rippler trick).
fn subsets(mask: Bitboard) -> Vec<Bitboard> {
    let mut result = Vec::with_capacity(1 << popcount(mask));
    let mut subset: Bitboard = 0;
    loop {
        result.push(subset);
        subset = subset.wrapping_sub(mask) & mask;
        if subset == 0 {
            break;
        }
    }
    result
}

/// xorshift64* — deterministic so magic generation is reproducible.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Sparse candidates hash far better than dense ones.
    fn sparse_u64(&mut self) -> u64 {
        self.next_u64() & self.next_u64() & self.next_u64()
    }
}

/// Finds a magic for one square and writes its attack entries into `table`.
fn build_magic(
    sq: Square,
    dirs: &[(i8, i8); 4],
    table: &mut Vec<Bitboard>,
    rng: &mut Rng,
) -> Magic {
    let mask = relevant_mask(sq, dirs);
    let bits = popcount(mask);
    let shift = 64 - bits;
    let size = 1usize << bits;
    let offset = table.len();

    let occupancies = subsets(mask);
    let references: Vec<Bitboard> = occupancies
        .iter()
        .map(|&occ| sliding_attacks(sq, occ, dirs))
        .collect();

    let mut used = vec![EMPTY; size];
    let mut epoch = vec![0u32; size];
    let mut current = 0u32;

    let magic = loop {
        let candidate = rng.sparse_u64();
        // Cheap filter: a usable magic must spread the top bits of the mask.
        if popcount(mask.wrapping_mul(candidate) & 0xFF00_0000_0000_0000) < 6 {
            continue;
        }
        current += 1;
        let mut ok = true;
        for (i, &occ) in occupancies.iter().enumerate() {
            let idx = ((occ.wrapping_mul(candidate)) >> shift) as usize;
            if epoch[idx] != current {
                epoch[idx] = current;
                used[idx] = references[i];
            } else if used[idx] != references[i] {
                ok = false;
                break;
            }
        }
        if ok {
            break candidate;
        }
    };

    table.resize(offset + size, EMPTY);
    for (i, &occ) in occupancies.iter().enumerate() {
        let idx = ((occ.wrapping_mul(magic)) >> shift) as usize;
        table[offset + idx] = references[i];
    }

    Magic {
        mask,
        magic,
        shift,
        offset,
    }
}

impl Tables {
    fn new() -> Tables {
        let mut pawn = [[EMPTY; 64]; 2];
        let mut knight = [EMPTY; 64];
        let mut king = [EMPTY; 64];

        for sq in 0..64u8 {
            let b = bb(sq);
            pawn[Color::White.index()][sq as usize] = east(north(b)) | west(north(b));
            pawn[Color::Black.index()][sq as usize] = east(south(b)) | west(south(b));

            let l1 = west(b);
            let l2 = west(west(b));
            let r1 = east(b);
            let r2 = east(east(b));
            knight[sq as usize] =
                ((l1 | r1) << 16) | ((l1 | r1) >> 16) | ((l2 | r2) << 8) | ((l2 | r2) >> 8);

            let ring = b | east(b) | west(b);
            king[sq as usize] = (ring | north(ring) | south(ring)) & !b;
        }

        let mut slider_attacks = Vec::with_capacity(107_648);
        let mut rng = Rng(0x00A0_9C48_1B7E_5D31);
        let mut rook = [Magic::default(); 64];
        let mut bishop = [Magic::default(); 64];
        for sq in 0..64u8 {
            bishop[sq as usize] = build_magic(sq, &BISHOP_DIRS, &mut slider_attacks, &mut rng);
        }
        for sq in 0..64u8 {
            rook[sq as usize] = build_magic(sq, &ROOK_DIRS, &mut slider_attacks, &mut rng);
        }

        let mut between = [[EMPTY; 64]; 64];
        for a in 0..64u8 {
            for b in 0..64u8 {
                for dirs in [&ROOK_DIRS, &BISHOP_DIRS] {
                    if sliding_attacks(a, EMPTY, dirs) & bb(b) != 0 {
                        between[a as usize][b as usize] =
                            sliding_attacks(a, bb(b), dirs) & sliding_attacks(b, bb(a), dirs);
                    }
                }
            }
        }

        Tables {
            pawn,
            knight,
            king,
            rook,
            bishop,
            slider_attacks,
            between,
        }
    }
}

/// Forces table construction so timing-sensitive code doesn't pay for it later.
pub fn init() {
    LazyLock::force(&TABLES);
}

#[inline]
pub fn pawn_attacks(color: Color, sq: Square) -> Bitboard {
    TABLES.pawn[color.index()][sq as usize]
}

#[inline]
pub fn knight_attacks(sq: Square) -> Bitboard {
    TABLES.knight[sq as usize]
}

#[inline]
pub fn king_attacks(sq: Square) -> Bitboard {
    TABLES.king[sq as usize]
}

#[inline]
pub fn rook_attacks(sq: Square, occupancy: Bitboard) -> Bitboard {
    let magic = &TABLES.rook[sq as usize];
    TABLES.slider_attacks[magic.index(occupancy)]
}

#[inline]
pub fn bishop_attacks(sq: Square, occupancy: Bitboard) -> Bitboard {
    let magic = &TABLES.bishop[sq as usize];
    TABLES.slider_attacks[magic.index(occupancy)]
}

#[inline]
pub fn queen_attacks(sq: Square, occupancy: Bitboard) -> Bitboard {
    rook_attacks(sq, occupancy) | bishop_attacks(sq, occupancy)
}

/// Squares strictly between `a` and `b`, or empty if they don't share a line.
#[inline]
pub fn between(a: Square, b: Square) -> Bitboard {
    TABLES.between[a as usize][b as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::parse_square;

    fn sq(s: &str) -> Square {
        parse_square(s).unwrap()
    }

    #[test]
    fn knight_attacks_are_correct() {
        assert_eq!(popcount(knight_attacks(sq("a1"))), 2);
        assert_eq!(popcount(knight_attacks(sq("d4"))), 8);
        assert_eq!(popcount(knight_attacks(sq("h8"))), 2);
        assert_eq!(
            knight_attacks(sq("b1")),
            bb(sq("a3")) | bb(sq("c3")) | bb(sq("d2"))
        );
    }

    #[test]
    fn king_and_pawn_attacks_are_correct() {
        assert_eq!(popcount(king_attacks(sq("a1"))), 3);
        assert_eq!(popcount(king_attacks(sq("e4"))), 8);
        assert_eq!(pawn_attacks(Color::White, sq("a2")), bb(sq("b3")));
        assert_eq!(pawn_attacks(Color::Black, sq("h7")), bb(sq("g6")));
        assert_eq!(
            pawn_attacks(Color::White, sq("e4")),
            bb(sq("d5")) | bb(sq("f5"))
        );
    }

    #[test]
    fn sliders_stop_at_blockers() {
        // Rook on a1 with a blocker on a4: reaches a2..a4 and b1..h1.
        let occ = bb(sq("a4"));
        let attacks = rook_attacks(sq("a1"), occ);
        assert!(contains(attacks, sq("a4")));
        assert!(!contains(attacks, sq("a5")));
        assert!(contains(attacks, sq("h1")));
        assert_eq!(popcount(attacks), 3 + 7);

        let attacks = bishop_attacks(sq("c1"), bb(sq("e3")));
        assert!(contains(attacks, sq("e3")));
        assert!(!contains(attacks, sq("f4")));
    }

    #[test]
    fn magics_agree_with_reference_generation() {
        for sq in 0..64u8 {
            for occ in [EMPTY, 0x1234_5678_9abc_def0, FULL, RANK_4 | FILE_B] {
                assert_eq!(rook_attacks(sq, occ), sliding_attacks(sq, occ, &ROOK_DIRS));
                assert_eq!(
                    bishop_attacks(sq, occ),
                    sliding_attacks(sq, occ, &BISHOP_DIRS)
                );
            }
        }
    }

    #[test]
    fn between_covers_only_inner_squares() {
        assert_eq!(between(sq("a1"), sq("a4")), bb(sq("a2")) | bb(sq("a3")));
        assert_eq!(between(sq("c1"), sq("f4")), bb(sq("d2")) | bb(sq("e3")));
        assert_eq!(between(sq("a1"), sq("b1")), EMPTY);
        assert_eq!(between(sq("a1"), sq("b3")), EMPTY);
    }
}
