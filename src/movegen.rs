//! Pseudo-legal move generation.
//!
//! Moves are generated pseudo-legally and filtered by `Position::try_make_move`,
//! which rejects any move leaving the king in check. This keeps generation
//! simple at a small cost in make/unmake work.

use std::ops::{Deref, DerefMut};

use crate::attacks;
use crate::bitboard::*;
use crate::position::Position;
use crate::types::*;

/// No legal chess position exceeds this many pseudo-legal moves.
pub const MAX_MOVES: usize = 256;

/// A stack-allocated move buffer; avoids heap traffic in the search hot path.
pub struct MoveList {
    moves: [Move; MAX_MOVES],
    len: usize,
}

impl MoveList {
    pub fn new() -> MoveList {
        MoveList {
            moves: [Move::NONE; MAX_MOVES],
            len: 0,
        }
    }

    #[inline]
    pub fn push(&mut self, mv: Move) {
        debug_assert!(self.len < MAX_MOVES);
        self.moves[self.len] = mv;
        self.len += 1;
    }
}

impl Default for MoveList {
    fn default() -> MoveList {
        MoveList::new()
    }
}

impl Deref for MoveList {
    type Target = [Move];

    #[inline]
    fn deref(&self) -> &[Move] {
        &self.moves[..self.len]
    }
}

impl DerefMut for MoveList {
    #[inline]
    fn deref_mut(&mut self) -> &mut [Move] {
        &mut self.moves[..self.len]
    }
}

impl IntoIterator for MoveList {
    type Item = Move;
    type IntoIter = std::iter::Take<std::array::IntoIter<Move, MAX_MOVES>>;

    fn into_iter(self) -> Self::IntoIter {
        self.moves.into_iter().take(self.len)
    }
}

/// All pseudo-legal moves for the side to move.
pub fn generate_moves(pos: &Position) -> MoveList {
    let mut list = MoveList::new();
    generate(pos, &mut list, false);
    list
}

/// Captures and promotions only — the quiescence search move set.
pub fn generate_captures(pos: &Position) -> MoveList {
    let mut list = MoveList::new();
    generate(pos, &mut list, true);
    list
}

/// Fully legal moves. Clones nothing but does make/unmake per move, so it is
/// meant for move parsing, perft and tests rather than the inner search.
pub fn generate_legal_moves(pos: &mut Position) -> Vec<Move> {
    let mut legal = Vec::with_capacity(48);
    for mv in generate_moves(pos) {
        if pos.try_make_move(mv) {
            pos.unmake_move();
            legal.push(mv);
        }
    }
    legal
}

fn generate(pos: &Position, list: &mut MoveList, captures_only: bool) {
    let us = pos.side_to_move;
    let occupied = pos.occupied();
    let enemy = pos.color_bb(us.flip());
    let targets = if captures_only {
        enemy
    } else {
        !pos.color_bb(us)
    };

    generate_pawn_moves(pos, list, captures_only);

    for pt in [
        PieceType::Knight,
        PieceType::Bishop,
        PieceType::Rook,
        PieceType::Queen,
        PieceType::King,
    ] {
        for from in iter_bits(pos.pieces(us, pt)) {
            let mut moves = piece_attacks(pt, from, occupied) & targets;
            while moves != 0 {
                let to = pop_lsb(&mut moves);
                let flag = if contains(enemy, to) {
                    move_flags::CAPTURE
                } else {
                    move_flags::QUIET
                };
                list.push(Move::new(from, to, flag));
            }
        }
    }

    if !captures_only {
        generate_castles(pos, list);
    }
}

#[inline]
fn piece_attacks(pt: PieceType, from: Square, occupied: Bitboard) -> Bitboard {
    match pt {
        PieceType::Knight => attacks::knight_attacks(from),
        PieceType::Bishop => attacks::bishop_attacks(from, occupied),
        PieceType::Rook => attacks::rook_attacks(from, occupied),
        PieceType::Queen => attacks::queen_attacks(from, occupied),
        PieceType::King => attacks::king_attacks(from),
        PieceType::Pawn => unreachable!("pawns are generated separately"),
    }
}

fn generate_pawn_moves(pos: &Position, list: &mut MoveList, captures_only: bool) {
    let us = pos.side_to_move;
    let pawns = pos.pieces(us, PieceType::Pawn);
    if pawns == 0 {
        return;
    }
    let empty = !pos.occupied();
    let enemy = pos.color_bb(us.flip());

    let white = us == Color::White;
    let forward: i8 = if white { 8 } else { -8 };
    let double_rank = if white { RANKS[2] } else { RANKS[5] };
    let promo_rank = if white { RANK_8 } else { RANK_1 };
    let push = |b: Bitboard| if white { b << 8 } else { b >> 8 };
    let origin = |to: Square, steps: i8| (to as i8 - forward * steps) as Square;

    let single = push(pawns) & empty;
    let promotions = single & promo_rank;

    for to in iter_bits(promotions) {
        push_promotions(list, origin(to, 1), to, false);
    }

    if !captures_only {
        for to in iter_bits(single & !promo_rank) {
            list.push(Move::new(origin(to, 1), to, move_flags::QUIET));
        }
        for to in iter_bits(push(single & double_rank) & empty) {
            list.push(Move::new(origin(to, 2), to, move_flags::DOUBLE_PUSH));
        }
    }

    for from in iter_bits(pawns) {
        let mut captures = attacks::pawn_attacks(us, from) & enemy;
        while captures != 0 {
            let to = pop_lsb(&mut captures);
            if contains(promo_rank, to) {
                push_promotions(list, from, to, true);
            } else {
                list.push(Move::new(from, to, move_flags::CAPTURE));
            }
        }
    }

    if let Some(ep) = pos.en_passant {
        // Pawns that could capture *onto* the ep square are exactly those a
        // pawn of the opposite colour on that square would attack.
        for from in iter_bits(attacks::pawn_attacks(us.flip(), ep) & pawns) {
            list.push(Move::new(from, ep, move_flags::EN_PASSANT));
        }
    }
}

fn push_promotions(list: &mut MoveList, from: Square, to: Square, capture: bool) {
    let base = if capture {
        move_flags::PROMO_CAPTURE_KNIGHT
    } else {
        move_flags::PROMO_KNIGHT
    };
    // Queen first: it is nearly always best, so it gets searched first.
    for offset in [3, 0, 1, 2] {
        list.push(Move::new(from, to, base + offset));
    }
}

fn generate_castles(pos: &Position, list: &mut MoveList) {
    let us = pos.side_to_move;
    let them = us.flip();
    let occupied = pos.occupied();
    let (king_side, queen_side, home) = match us {
        Color::White => (WHITE_KING_SIDE, WHITE_QUEEN_SIDE, 0u8),
        Color::Black => (BLACK_KING_SIDE, BLACK_QUEEN_SIDE, 56u8),
    };
    let king = home + 4;

    // Castling out of check is illegal, and so is crossing an attacked square.
    if pos.castling & (king_side | queen_side) == 0 || pos.is_attacked(king, them) {
        return;
    }

    if pos.castling & king_side != 0
        && occupied & (bb(home + 5) | bb(home + 6)) == 0
        && !pos.is_attacked(home + 5, them)
    {
        list.push(Move::new(king, home + 6, move_flags::KING_CASTLE));
    }
    if pos.castling & queen_side != 0
        && occupied & (bb(home + 1) | bb(home + 2) | bb(home + 3)) == 0
        && !pos.is_attacked(home + 3, them)
    {
        list.push(Move::new(king, home + 2, move_flags::QUEEN_CASTLE));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::STARTPOS_FEN;

    fn legal_count(fen: &str) -> usize {
        let mut pos = Position::from_fen(fen).unwrap();
        generate_legal_moves(&mut pos).len()
    }

    #[test]
    fn startpos_has_twenty_moves() {
        assert_eq!(legal_count(STARTPOS_FEN), 20);
    }

    #[test]
    fn pinned_pieces_cannot_move_away() {
        // The knight on e2 is pinned by the rook on e8, so only the four king
        // steps off the e-file are legal.
        let fen = "4rk2/8/8/8/8/8/4N3/4K3 w - - 0 1";
        assert_eq!(legal_count(fen), 4);
        let mut pos = Position::from_fen(fen).unwrap();
        assert!(
            !generate_legal_moves(&mut pos)
                .iter()
                .any(|m| m.from() == 12)
        );
    }

    #[test]
    fn king_must_escape_check() {
        // Only king moves and the knight interposition/capture are legal.
        let fen = "4k3/8/8/8/7b/8/8/4K3 w - - 0 1";
        let mut pos = Position::from_fen(fen).unwrap();
        for mv in generate_legal_moves(&mut pos) {
            pos.make_move(mv);
            assert!(!pos.is_attacked(pos.king_square(Color::White), Color::Black));
            pos.unmake_move();
        }
    }

    #[test]
    fn castling_is_blocked_and_gated_correctly() {
        // Free to castle both ways.
        let mut pos = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
        let castles = generate_legal_moves(&mut pos)
            .iter()
            .filter(|m| m.is_castle())
            .count();
        assert_eq!(castles, 2);

        // f1 attacked by the black rook: king side is out.
        let mut pos = Position::from_fen("5rk1/8/8/8/8/8/8/R3K2R w KQ - 0 1").unwrap();
        let castles: Vec<_> = generate_legal_moves(&mut pos)
            .into_iter()
            .filter(|m| m.is_castle())
            .collect();
        assert_eq!(castles.len(), 1);
        assert_eq!(castles[0].to_string(), "e1c1");

        // In check: no castling at all.
        let mut pos = Position::from_fen("4r1k1/8/8/8/8/8/8/R3K2R w KQ - 0 1").unwrap();
        assert!(!generate_legal_moves(&mut pos).iter().any(|m| m.is_castle()));

        // b1 occupied blocks queen side but not king side.
        let mut pos = Position::from_fen("6k1/8/8/8/8/8/8/RN2K2R w KQ - 0 1").unwrap();
        let castles: Vec<_> = generate_legal_moves(&mut pos)
            .into_iter()
            .filter(|m| m.is_castle())
            .collect();
        assert_eq!(castles.len(), 1);
        assert_eq!(castles[0].to_string(), "e1g1");
    }

    #[test]
    fn promotions_generate_all_four_pieces() {
        let mut pos = Position::from_fen("8/4P3/8/8/8/8/8/K6k w - - 0 1").unwrap();
        let promos: Vec<String> = generate_legal_moves(&mut pos)
            .iter()
            .filter(|m| m.is_promotion())
            .map(|m| m.to_string())
            .collect();
        assert_eq!(promos.len(), 4);
        assert!(promos.contains(&"e7e8q".to_string()));
        assert!(promos.contains(&"e7e8n".to_string()));
    }

    #[test]
    fn en_passant_that_exposes_the_king_is_illegal() {
        // Capturing exd6 e.p. would clear the fifth rank for the h5 rook.
        let mut pos = Position::from_fen("8/8/8/K2pP2r/8/8/8/7k w - d6 0 1").unwrap();
        assert!(
            !generate_legal_moves(&mut pos)
                .iter()
                .any(|m| m.is_en_passant())
        );
    }

    #[test]
    fn capture_generation_is_a_subset_of_all_moves() {
        for fen in [
            STARTPOS_FEN,
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 b - - 0 1",
        ] {
            let pos = Position::from_fen(fen).unwrap();
            let all: Vec<Move> = generate_moves(&pos).to_vec();
            for mv in generate_captures(&pos) {
                assert!(
                    all.contains(&mv),
                    "{mv} missing from full generation in {fen}"
                );
                assert!(mv.is_capture() || mv.is_promotion());
            }
        }
    }
}
