//! Static evaluation: tapered material + piece-square tables (PeSTO values)
//! plus pawn structure, mobility, rook placement and a king shelter term.
//!
//! Scores are centipawns from the side to move's point of view.

use std::sync::LazyLock;

use crate::attacks;
use crate::bitboard::*;
use crate::position::Position;
use crate::types::*;

pub const MATE: i32 = 30_000;
/// Any |score| above this encodes a forced mate at some distance.
pub const MATE_IN_MAX_PLY: i32 = MATE - 1000;
pub const INFINITY: i32 = 32_000;

/// Midgame/endgame material values, indexed by `PieceType`.
const MG_MATERIAL: [i32; 6] = [82, 337, 365, 477, 1025, 0];
const EG_MATERIAL: [i32; 6] = [94, 281, 297, 512, 936, 0];

/// Game phase weight per piece; 24 is a full opening board.
const PHASE_WEIGHT: [i32; 6] = [0, 1, 1, 2, 4, 0];
const TOTAL_PHASE: i32 = 24;

/// Value used for move ordering and delta pruning.
#[inline]
pub fn piece_value(pt: PieceType) -> i32 {
    MG_MATERIAL[pt.index()]
}

// Piece-square tables, written rank 8 first so they read like a board.
#[rustfmt::skip]
const MG_TABLES: [[i32; 64]; 6] = [
    // Pawn
    [
          0,   0,   0,   0,   0,   0,   0,   0,
         98, 134,  61,  95,  68, 126,  34, -11,
         -6,   7,  26,  31,  65,  56,  25, -20,
        -14,  13,   6,  21,  23,  12,  17, -23,
        -27,  -2,  -5,  12,  17,   6,  10, -25,
        -26,  -4,  -4, -10,   3,   3,  33, -12,
        -35,  -1, -20, -23, -15,  24,  38, -22,
          0,   0,   0,   0,   0,   0,   0,   0,
    ],
    // Knight
    [
       -167, -89, -34, -49,  61, -97, -15, -107,
        -73, -41,  72,  36,  23,  62,   7,  -17,
        -47,  60,  37,  65,  84, 129,  73,   44,
         -9,  17,  19,  53,  37,  69,  18,   22,
        -13,   4,  16,  13,  28,  19,  21,   -8,
        -23,  -9,  12,  10,  19,  17,  25,  -16,
        -29, -53, -12,  -3,  -1,  18, -14,  -19,
       -105, -21, -58, -33, -17, -28, -19,  -23,
    ],
    // Bishop
    [
        -29,   4, -82, -37, -25, -42,   7,  -8,
        -26,  16, -18, -13,  30,  59,  18, -47,
        -16,  37,  43,  40,  35,  50,  37,  -2,
         -4,   5,  19,  50,  37,  37,   7,  -2,
         -6,  13,  13,  26,  34,  12,  10,   4,
          0,  15,  15,  15,  14,  27,  18,  10,
          4,  15,  16,   0,   7,  21,  33,   1,
        -33,  -3, -14, -21, -13, -12, -39, -21,
    ],
    // Rook
    [
         32,  42,  32,  51,  63,   9,  31,  43,
         27,  32,  58,  62,  80,  67,  26,  44,
         -5,  19,  26,  36,  17,  45,  61,  16,
        -24, -11,   7,  26,  24,  35,  -8, -20,
        -36, -26, -12,  -1,   9,  -7,   6, -23,
        -45, -25, -16, -17,   3,   0,  -5, -33,
        -44, -16, -20,  -9,  -1,  11,  -6, -71,
        -19, -13,   1,  17,  16,   7, -37, -26,
    ],
    // Queen
    [
        -28,   0,  29,  12,  59,  44,  43,  45,
        -24, -39,  -5,   1, -16,  57,  28,  54,
        -13, -17,   7,   8,  29,  56,  47,  57,
        -27, -27, -16, -16,  -1,  17,  -2,   1,
         -9, -26,  -9, -10,  -2,  -4,   3,  -3,
        -14,   2, -11,  -2,  -5,   2,  14,   5,
        -35,  -8,  11,   2,   8,  15,  -3,   1,
         -1, -18,  -9,  10, -15, -25, -31, -50,
    ],
    // King
    [
        -65,  23,  16, -15, -56, -34,   2,  13,
         29,  -1, -20,  -7,  -8,  -4, -38, -29,
         -9,  24,   2, -16, -20,   6,  22, -22,
        -17, -20, -12, -27, -30, -25, -14, -36,
        -49,  -1, -27, -39, -46, -44, -33, -51,
        -14, -14, -22, -46, -44, -30, -15, -27,
          1,   7,  -8, -64, -43, -16,   9,   8,
        -15,  36,  12, -54,   8, -28,  24,  14,
    ],
];

#[rustfmt::skip]
const EG_TABLES: [[i32; 64]; 6] = [
    // Pawn
    [
          0,   0,   0,   0,   0,   0,   0,   0,
        178, 173, 158, 134, 147, 132, 165, 187,
         94, 100,  85,  67,  56,  53,  82,  84,
         32,  24,  13,   5,  -2,   4,  17,  17,
         13,   9,  -3,  -7,  -7,  -8,   3,  -1,
          4,   7,  -6,   1,   0,  -5,  -1,  -8,
         13,   8,   8,  10,  13,   0,   2,  -7,
          0,   0,   0,   0,   0,   0,   0,   0,
    ],
    // Knight
    [
        -58, -38, -13, -28, -31, -27, -63, -99,
        -25,  -8, -25,  -2,  -9, -25, -24, -52,
        -24, -20,  10,   9,  -1,  -9, -19, -41,
        -17,   3,  22,  22,  22,  11,   8, -18,
        -18,  -6,  16,  25,  16,  17,   4, -18,
        -23,  -3,  -1,  15,  10,  -3, -20, -22,
        -42, -20, -10,  -5,  -2, -20, -23, -44,
        -29, -51, -23, -15, -22, -18, -50, -64,
    ],
    // Bishop
    [
        -14, -21, -11,  -8,  -7,  -9, -17, -24,
         -8,  -4,   7, -12,  -3, -13,  -4, -14,
          2,  -8,   0,  -1,  -2,   6,   0,   4,
         -3,   9,  12,   9,  14,  10,   3,   2,
         -6,   3,  13,  19,   7,  10,  -3,  -9,
        -12,  -3,   8,  10,  13,   3,  -7, -15,
        -14, -18,  -7,  -1,   4,  -9, -15, -27,
        -23,  -9, -23,  -5,  -9, -16,  -5, -17,
    ],
    // Rook
    [
         13,  10,  18,  15,  12,  12,   8,   5,
         11,  13,  13,  11,  -3,   3,   8,   3,
          7,   7,   7,   5,   4,  -3,  -5,  -3,
          4,   3,  13,   1,   2,   1,  -1,   2,
          3,   5,   8,   4,  -5,  -6,  -8, -11,
         -4,   0,  -5,  -1,  -7, -12,  -8, -16,
         -6,  -6,   0,   2,  -9,  -9, -11,  -3,
         -9,   2,   3,  -1,  -5, -13,   4, -20,
    ],
    // Queen
    [
         -9,  22,  22,  27,  27,  19,  10,  20,
        -17,  20,  32,  41,  58,  25,  30,   0,
        -20,   6,   9,  49,  47,  35,  19,   9,
          3,  22,  24,  45,  57,  40,  57,  36,
        -18,  28,  19,  47,  31,  34,  39,  23,
        -16, -27,  15,   6,   9,  17,  10,   5,
        -22, -23, -30, -16, -16, -23, -36, -32,
        -33, -28, -22, -43,  -5, -32, -20, -41,
    ],
    // King
    [
        -74, -35, -18, -18, -11,  15,   4, -17,
        -12,  17,  14,  17,  17,  38,  23,  11,
         10,  17,  23,  15,  20,  45,  44,  13,
         -8,  22,  24,  27,  26,  33,  26,   3,
        -18,  -4,  21,  24,  27,  23,   9, -11,
        -19,  -3,  11,  21,  23,  16,   7,  -9,
        -27, -11,   4,  13,  14,   4,  -5, -17,
        -53, -34, -21, -11, -28, -14, -24, -43,
    ],
];

struct PawnMasks {
    /// Squares that must be free of enemy pawns for a pawn to be passed.
    passed: [[Bitboard; 64]; 2],
    /// The files either side of a given file.
    adjacent_files: [Bitboard; 8],
}

static PAWN_MASKS: LazyLock<PawnMasks> = LazyLock::new(|| {
    let mut adjacent_files = [EMPTY; 8];
    for file in 0..8usize {
        if file > 0 {
            adjacent_files[file] |= FILES[file - 1];
        }
        if file < 7 {
            adjacent_files[file] |= FILES[file + 1];
        }
    }

    let mut passed = [[EMPTY; 64]; 2];
    for sq in 0..64u8 {
        let file = file_of(sq) as usize;
        let rank = rank_of(sq) as usize;
        let span = FILES[file] | adjacent_files[file];
        let ahead_white = RANKS[rank + 1..].iter().fold(EMPTY, |acc, &r| acc | r);
        let ahead_black = RANKS[..rank].iter().fold(EMPTY, |acc, &r| acc | r);
        passed[Color::White.index()][sq as usize] = span & ahead_white;
        passed[Color::Black.index()][sq as usize] = span & ahead_black;
    }
    PawnMasks {
        passed,
        adjacent_files,
    }
});

const PASSED_PAWN_MG: [i32; 8] = [0, 5, 10, 20, 35, 60, 100, 0];
const PASSED_PAWN_EG: [i32; 8] = [0, 15, 25, 40, 65, 105, 165, 0];
const DOUBLED_PAWN: (i32, i32) = (-10, -22);
const ISOLATED_PAWN: (i32, i32) = (-14, -12);
const BISHOP_PAIR: (i32, i32) = (30, 50);
const ROOK_OPEN_FILE: (i32, i32) = (28, 12);
const ROOK_SEMI_OPEN_FILE: (i32, i32) = (12, 6);
/// Mobility is scored per attacked square, offset so an average piece is neutral.
const MOBILITY_MG: [i32; 6] = [0, 4, 4, 3, 1, 0];
const MOBILITY_EG: [i32; 6] = [0, 4, 5, 4, 3, 0];
const MOBILITY_OFFSET: [i32; 6] = [0, 4, 6, 6, 12, 0];
const KING_SHELTER: i32 = 9;
const TEMPO: i32 = 12;

/// Running midgame/endgame score pair, always from white's perspective.
#[derive(Default, Clone, Copy)]
struct Score {
    mg: i32,
    eg: i32,
}

impl Score {
    #[inline]
    fn add(&mut self, color: Color, mg: i32, eg: i32) {
        let sign = if color == Color::White { 1 } else { -1 };
        self.mg += sign * mg;
        self.eg += sign * eg;
    }
}

pub fn evaluate(pos: &Position) -> i32 {
    let mut score = Score::default();
    let mut phase = 0;

    for color in [Color::White, Color::Black] {
        phase += evaluate_side(pos, color, &mut score);
    }

    let phase = phase.min(TOTAL_PHASE);
    let tapered = (score.mg * phase + score.eg * (TOTAL_PHASE - phase)) / TOTAL_PHASE;
    let perspective = if pos.side_to_move == Color::White {
        1
    } else {
        -1
    };
    tapered * perspective + TEMPO
}

/// Accumulates one side's terms into `score` and returns its phase weight.
fn evaluate_side(pos: &Position, color: Color, score: &mut Score) -> i32 {
    let mut phase = 0;
    let occupied = pos.occupied();
    let own = pos.color_bb(color);
    let own_pawns = pos.pieces(color, PieceType::Pawn);
    let enemy_pawns = pos.pieces(color.flip(), PieceType::Pawn);
    // Squares defended by enemy pawns are not useful mobility.
    let enemy_pawn_attacks = pawn_attack_span(enemy_pawns, color.flip());
    let masks = &*PAWN_MASKS;

    for pt in PIECE_TYPES {
        let pieces = pos.pieces(color, pt);
        phase += popcount(pieces) as i32 * PHASE_WEIGHT[pt.index()];

        for sq in iter_bits(pieces) {
            // Tables are written from white's view, so mirror for black.
            let table_sq = if color == Color::White {
                flip_square(sq)
            } else {
                sq
            } as usize;
            score.add(
                color,
                MG_MATERIAL[pt.index()] + MG_TABLES[pt.index()][table_sq],
                EG_MATERIAL[pt.index()] + EG_TABLES[pt.index()][table_sq],
            );

            match pt {
                PieceType::Pawn => {
                    let file = file_of(sq) as usize;
                    let relative_rank = match color {
                        Color::White => rank_of(sq),
                        Color::Black => 7 - rank_of(sq),
                    } as usize;

                    if masks.passed[color.index()][sq as usize] & enemy_pawns == 0 {
                        score.add(
                            color,
                            PASSED_PAWN_MG[relative_rank],
                            PASSED_PAWN_EG[relative_rank],
                        );
                    }
                    if own_pawns & masks.adjacent_files[file] == 0 {
                        score.add(color, ISOLATED_PAWN.0, ISOLATED_PAWN.1);
                    }
                    if popcount(own_pawns & FILES[file]) > 1 {
                        // Counted once per pawn on the file, halving the penalty each.
                        score.add(color, DOUBLED_PAWN.0 / 2, DOUBLED_PAWN.1 / 2);
                    }
                }
                PieceType::Rook => {
                    let file = FILES[file_of(sq) as usize];
                    if own_pawns & file == 0 {
                        if enemy_pawns & file == 0 {
                            score.add(color, ROOK_OPEN_FILE.0, ROOK_OPEN_FILE.1);
                        } else {
                            score.add(color, ROOK_SEMI_OPEN_FILE.0, ROOK_SEMI_OPEN_FILE.1);
                        }
                    }
                }
                PieceType::King => {
                    // Friendly pawns sheltering the king, mostly a midgame concern.
                    let shelter = attacks::king_attacks(sq) & own_pawns;
                    score.add(color, popcount(shelter) as i32 * KING_SHELTER, 0);
                }
                _ => {}
            }

            if matches!(
                pt,
                PieceType::Knight | PieceType::Bishop | PieceType::Rook | PieceType::Queen
            ) {
                let moves = match pt {
                    PieceType::Knight => attacks::knight_attacks(sq),
                    PieceType::Bishop => attacks::bishop_attacks(sq, occupied),
                    PieceType::Rook => attacks::rook_attacks(sq, occupied),
                    _ => attacks::queen_attacks(sq, occupied),
                };
                let safe = popcount(moves & !own & !enemy_pawn_attacks) as i32
                    - MOBILITY_OFFSET[pt.index()];
                score.add(
                    color,
                    safe * MOBILITY_MG[pt.index()],
                    safe * MOBILITY_EG[pt.index()],
                );
            }
        }
    }

    if popcount(pos.pieces(color, PieceType::Bishop)) >= 2 {
        score.add(color, BISHOP_PAIR.0, BISHOP_PAIR.1);
    }

    phase
}

#[inline]
fn pawn_attack_span(pawns: Bitboard, color: Color) -> Bitboard {
    match color {
        Color::White => east(north(pawns)) | west(north(pawns)),
        Color::Black => east(south(pawns)) | west(south(pawns)),
    }
}

/// Formats a score the way UCI expects: `cp <n>` or `mate <moves>`.
pub fn format_score(score: i32) -> String {
    if score.abs() >= MATE_IN_MAX_PLY {
        let plies_to_mate = MATE - score.abs();
        let moves = (plies_to_mate + 1) / 2;
        format!("mate {}", if score > 0 { moves } else { -moves })
    } else {
        format!("cp {score}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::{Position, STARTPOS_FEN};

    #[test]
    fn start_position_is_balanced() {
        // Only the tempo bonus separates the sides.
        assert_eq!(evaluate(&Position::startpos()), TEMPO);
    }

    #[test]
    fn evaluation_is_symmetric_under_colour_flip() {
        // Mirrored positions must score identically for the side to move.
        let pairs = [
            (
                "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1",
                "4k3/4p3/8/8/8/8/8/4K3 b - - 0 1",
            ),
            (
                "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 0 1",
                "rnbqk2r/pppp1ppp/5n2/2b1p3/4P3/2N5/PPPP1PPP/R1BQKBNR b KQkq - 0 1",
            ),
        ];
        for (white_view, black_view) in pairs {
            let a = evaluate(&Position::from_fen(white_view).unwrap());
            let b = evaluate(&Position::from_fen(black_view).unwrap());
            assert_eq!(a, b, "{white_view} vs {black_view}");
        }
    }

    #[test]
    fn material_advantage_dominates() {
        let up_a_queen = Position::from_fen("4k3/8/8/8/8/8/8/3QK3 w - - 0 1").unwrap();
        assert!(evaluate(&up_a_queen) > 800);
        let down_a_rook = Position::from_fen("3rk3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        assert!(evaluate(&down_a_rook) < -400);
    }

    #[test]
    fn passed_pawns_beat_blocked_ones() {
        let passed = Position::from_fen("4k3/8/8/4P3/8/8/8/4K3 w - - 0 1").unwrap();
        let blocked = Position::from_fen("4k3/3p4/8/4P3/8/8/8/4K3 w - - 0 1").unwrap();
        assert!(evaluate(&passed) > evaluate(&blocked) + 100);
    }

    #[test]
    fn score_formatting_matches_uci() {
        assert_eq!(format_score(35), "cp 35");
        assert_eq!(format_score(-120), "cp -120");
        assert_eq!(format_score(MATE - 5), "mate 3");
        assert_eq!(format_score(-(MATE - 4)), "mate -2");
    }

    #[test]
    fn startpos_evaluation_is_stable_across_a_move_pair() {
        let mut pos = Position::from_fen(STARTPOS_FEN).unwrap();
        let before = evaluate(&pos);
        pos.make_move(pos.parse_uci_move("g1f3").unwrap());
        pos.make_move(pos.parse_uci_move("g8f6").unwrap());
        // Symmetric position again, so the same score should come back.
        assert_eq!(evaluate(&pos), before);
    }
}
