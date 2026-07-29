//! Board state: piece placement, FEN, and make/unmake with incremental hashing.

use std::fmt;

use crate::attacks;
use crate::bitboard::*;
use crate::types::*;
use crate::zobrist;

pub const STARTPOS_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/// Everything make_move destroys and unmake_move must restore.
#[derive(Clone, Copy)]
struct StateInfo {
    mv: Move,
    captured: Option<Piece>,
    castling: u8,
    en_passant: Option<Square>,
    halfmove_clock: u16,
    hash: u64,
}

/// Per-square mask of castling rights that survive a piece touching it.
static CASTLE_MASK: [u8; 64] = {
    let mut mask = [0xffu8; 64];
    mask[0] = !WHITE_QUEEN_SIDE; // a1
    mask[4] = !(WHITE_KING_SIDE | WHITE_QUEEN_SIDE); // e1
    mask[7] = !WHITE_KING_SIDE; // h1
    mask[56] = !BLACK_QUEEN_SIDE; // a8
    mask[60] = !(BLACK_KING_SIDE | BLACK_QUEEN_SIDE); // e8
    mask[63] = !BLACK_KING_SIDE; // h8
    mask
};

#[derive(Clone)]
pub struct Position {
    /// Occupancy per piece type, colour-agnostic.
    pieces: [Bitboard; 6],
    /// Occupancy per colour.
    colors: [Bitboard; 2],
    /// Square-indexed mailbox mirroring the bitboards.
    board: [Option<Piece>; 64],
    pub side_to_move: Color,
    pub castling: u8,
    pub en_passant: Option<Square>,
    pub halfmove_clock: u16,
    pub fullmove_number: u16,
    pub hash: u64,
    history: Vec<StateInfo>,
    /// Position hashes since the last irreversible move, for repetition detection.
    repetitions: Vec<u64>,
}

impl Position {
    pub fn empty() -> Position {
        Position {
            pieces: [EMPTY; 6],
            colors: [EMPTY; 2],
            board: [None; 64],
            side_to_move: Color::White,
            castling: 0,
            en_passant: None,
            halfmove_clock: 0,
            fullmove_number: 1,
            hash: 0,
            history: Vec::with_capacity(256),
            repetitions: Vec::with_capacity(256),
        }
    }

    pub fn startpos() -> Position {
        Position::from_fen(STARTPOS_FEN).expect("start position FEN is valid")
    }

    // --- Queries -----------------------------------------------------------

    #[inline]
    pub fn piece_at(&self, sq: Square) -> Option<Piece> {
        self.board[sq as usize]
    }

    #[inline]
    pub fn occupied(&self) -> Bitboard {
        self.colors[0] | self.colors[1]
    }

    #[inline]
    pub fn color_bb(&self, color: Color) -> Bitboard {
        self.colors[color.index()]
    }

    #[inline]
    pub fn piece_type_bb(&self, pt: PieceType) -> Bitboard {
        self.pieces[pt.index()]
    }

    #[inline]
    pub fn pieces(&self, color: Color, pt: PieceType) -> Bitboard {
        self.pieces[pt.index()] & self.colors[color.index()]
    }

    #[inline]
    pub fn king_square(&self, color: Color) -> Square {
        lsb(self.pieces(color, PieceType::King))
    }

    /// All pieces of either colour attacking `sq` under the given occupancy.
    pub fn attackers_to(&self, sq: Square, occupancy: Bitboard) -> Bitboard {
        let bishops =
            self.pieces[PieceType::Bishop.index()] | self.pieces[PieceType::Queen.index()];
        let rooks = self.pieces[PieceType::Rook.index()] | self.pieces[PieceType::Queen.index()];
        (attacks::pawn_attacks(Color::White, sq) & self.pieces(Color::Black, PieceType::Pawn))
            | (attacks::pawn_attacks(Color::Black, sq) & self.pieces(Color::White, PieceType::Pawn))
            | (attacks::knight_attacks(sq) & self.pieces[PieceType::Knight.index()])
            | (attacks::king_attacks(sq) & self.pieces[PieceType::King.index()])
            | (attacks::bishop_attacks(sq, occupancy) & bishops)
            | (attacks::rook_attacks(sq, occupancy) & rooks)
    }

    /// Whether `color` attacks `sq`. Short-circuits, so cheaper than
    /// `attackers_to` when only a yes/no answer is needed.
    pub fn is_attacked(&self, sq: Square, color: Color) -> bool {
        let occupancy = self.occupied();
        if attacks::pawn_attacks(color.flip(), sq) & self.pieces(color, PieceType::Pawn) != 0 {
            return true;
        }
        if attacks::knight_attacks(sq) & self.pieces(color, PieceType::Knight) != 0 {
            return true;
        }
        if attacks::king_attacks(sq) & self.pieces(color, PieceType::King) != 0 {
            return true;
        }
        let queens = self.pieces(color, PieceType::Queen);
        if attacks::bishop_attacks(sq, occupancy) & (self.pieces(color, PieceType::Bishop) | queens)
            != 0
        {
            return true;
        }
        attacks::rook_attacks(sq, occupancy) & (self.pieces(color, PieceType::Rook) | queens) != 0
    }

    #[inline]
    pub fn in_check(&self) -> bool {
        self.is_attacked(
            self.king_square(self.side_to_move),
            self.side_to_move.flip(),
        )
    }

    /// Enemy pieces currently giving check to the side to move.
    pub fn checkers(&self) -> Bitboard {
        let king = self.king_square(self.side_to_move);
        self.attackers_to(king, self.occupied()) & self.color_bb(self.side_to_move.flip())
    }

    /// True when only kings (plus at most one minor per side) remain, i.e.
    /// no side can force mate. Used to avoid pointless searching.
    pub fn is_insufficient_material(&self) -> bool {
        if self.pieces[PieceType::Pawn.index()] != 0
            || self.pieces[PieceType::Rook.index()] != 0
            || self.pieces[PieceType::Queen.index()] != 0
        {
            return false;
        }
        let minors =
            self.pieces[PieceType::Knight.index()] | self.pieces[PieceType::Bishop.index()];
        popcount(minors) <= 1
    }

    /// Whether the current position has occurred before in this game or search.
    pub fn is_repetition(&self) -> bool {
        let limit = self
            .repetitions
            .len()
            .saturating_sub(self.halfmove_clock as usize);
        self.repetitions[limit..]
            .iter()
            .rev()
            .skip(1)
            .step_by(2)
            .any(|&h| h == self.hash)
    }

    pub fn is_draw(&self) -> bool {
        self.halfmove_clock >= 100 || self.is_repetition() || self.is_insufficient_material()
    }

    /// Whether the side to move has any piece besides pawns and its king;
    /// null-move pruning is unsound in zugzwang-prone endings without one.
    pub fn has_non_pawn_material(&self, color: Color) -> bool {
        let pawns_and_king =
            self.pieces[PieceType::Pawn.index()] | self.pieces[PieceType::King.index()];
        self.color_bb(color) & !pawns_and_king != 0
    }

    // --- Piece placement ---------------------------------------------------

    fn add_piece(&mut self, piece: Piece, sq: Square) {
        debug_assert!(self.board[sq as usize].is_none());
        self.board[sq as usize] = Some(piece);
        self.pieces[piece.piece_type.index()] |= bb(sq);
        self.colors[piece.color.index()] |= bb(sq);
        self.hash ^= zobrist::piece(piece, sq);
    }

    fn remove_piece(&mut self, sq: Square) -> Piece {
        let piece = self.board[sq as usize].expect("no piece to remove");
        self.board[sq as usize] = None;
        self.pieces[piece.piece_type.index()] &= !bb(sq);
        self.colors[piece.color.index()] &= !bb(sq);
        self.hash ^= zobrist::piece(piece, sq);
        piece
    }

    fn move_piece(&mut self, from: Square, to: Square) {
        let piece = self.board[from as usize].expect("no piece to move");
        let delta = bb(from) | bb(to);
        self.board[from as usize] = None;
        self.board[to as usize] = Some(piece);
        self.pieces[piece.piece_type.index()] ^= delta;
        self.colors[piece.color.index()] ^= delta;
        self.hash ^= zobrist::piece(piece, from) ^ zobrist::piece(piece, to);
    }

    // --- Make / unmake -----------------------------------------------------

    /// Applies a pseudo-legal move unconditionally. See `try_make_move` for the
    /// legality-checked variant used by search.
    pub fn make_move(&mut self, mv: Move) {
        let us = self.side_to_move;
        let them = us.flip();
        let from = mv.from();
        let to = mv.to();
        let moving = self.board[from as usize].expect("move from an empty square");

        let captured = if mv.is_en_passant() {
            Some(Piece::new(them, PieceType::Pawn))
        } else {
            self.board[to as usize]
        };

        self.history.push(StateInfo {
            mv,
            captured,
            castling: self.castling,
            en_passant: self.en_passant,
            halfmove_clock: self.halfmove_clock,
            hash: self.hash,
        });
        self.repetitions.push(self.hash);

        if let Some(ep) = self.en_passant {
            self.hash ^= zobrist::en_passant(file_of(ep));
        }
        self.en_passant = None;

        if moving.piece_type == PieceType::Pawn || captured.is_some() {
            self.halfmove_clock = 0;
        } else {
            self.halfmove_clock += 1;
        }

        if mv.is_en_passant() {
            let captured_sq = if us == Color::White { to - 8 } else { to + 8 };
            self.remove_piece(captured_sq);
        } else if captured.is_some() {
            self.remove_piece(to);
        }

        self.move_piece(from, to);

        if let Some(promo) = mv.promotion_piece() {
            self.remove_piece(to);
            self.add_piece(Piece::new(us, promo), to);
        }

        match mv.flags() {
            move_flags::KING_CASTLE => self.move_piece(to + 1, to - 1),
            move_flags::QUEEN_CASTLE => self.move_piece(to - 2, to + 1),
            move_flags::DOUBLE_PUSH => {
                let ep = if us == Color::White { to - 8 } else { to + 8 };
                self.en_passant = Some(ep);
                self.hash ^= zobrist::en_passant(file_of(ep));
            }
            _ => {}
        }

        self.hash ^= zobrist::castling(self.castling);
        self.castling &= CASTLE_MASK[from as usize] & CASTLE_MASK[to as usize];
        self.hash ^= zobrist::castling(self.castling);

        self.side_to_move = them;
        self.hash ^= zobrist::side_to_move();
        if self.side_to_move == Color::White {
            self.fullmove_number += 1;
        }
    }

    pub fn unmake_move(&mut self) {
        let state = self.history.pop().expect("no move to unmake");
        let mv = state.mv;
        let them = self.side_to_move;
        let us = them.flip();
        let from = mv.from();
        let to = mv.to();

        self.side_to_move = us;
        if them == Color::White {
            self.fullmove_number -= 1;
        }

        match mv.flags() {
            move_flags::KING_CASTLE => self.move_piece(to - 1, to + 1),
            move_flags::QUEEN_CASTLE => self.move_piece(to + 1, to - 2),
            _ => {}
        }

        if mv.is_promotion() {
            self.remove_piece(to);
            self.add_piece(Piece::new(us, PieceType::Pawn), to);
        }

        self.move_piece(to, from);

        if let Some(captured) = state.captured {
            let sq = if mv.is_en_passant() {
                if us == Color::White { to - 8 } else { to + 8 }
            } else {
                to
            };
            self.add_piece(captured, sq);
        }

        self.castling = state.castling;
        self.en_passant = state.en_passant;
        self.halfmove_clock = state.halfmove_clock;
        self.hash = state.hash;
        self.repetitions.pop();
    }

    /// Makes `mv` only if it leaves the mover's king safe. Returns false and
    /// leaves the position untouched otherwise.
    pub fn try_make_move(&mut self, mv: Move) -> bool {
        let us = self.side_to_move;
        self.make_move(mv);
        if self.is_attacked(self.king_square(us), self.side_to_move) {
            self.unmake_move();
            false
        } else {
            true
        }
    }

    /// Passes the turn without moving. Only legal when not in check.
    pub fn make_null_move(&mut self) {
        self.history.push(StateInfo {
            mv: Move::NONE,
            captured: None,
            castling: self.castling,
            en_passant: self.en_passant,
            halfmove_clock: self.halfmove_clock,
            hash: self.hash,
        });
        self.repetitions.push(self.hash);

        if let Some(ep) = self.en_passant {
            self.hash ^= zobrist::en_passant(file_of(ep));
        }
        self.en_passant = None;
        self.halfmove_clock += 1;
        self.side_to_move = self.side_to_move.flip();
        self.hash ^= zobrist::side_to_move();
        if self.side_to_move == Color::White {
            self.fullmove_number += 1;
        }
    }

    pub fn unmake_null_move(&mut self) {
        let state = self.history.pop().expect("no null move to unmake");
        self.repetitions.pop();
        if self.side_to_move == Color::White {
            self.fullmove_number -= 1;
        }
        self.side_to_move = self.side_to_move.flip();
        self.castling = state.castling;
        self.en_passant = state.en_passant;
        self.halfmove_clock = state.halfmove_clock;
        self.hash = state.hash;
    }

    // --- FEN ---------------------------------------------------------------

    pub fn from_fen(fen: &str) -> Result<Position, String> {
        let mut pos = Position::empty();
        let mut fields = fen.split_whitespace();

        let placement = fields.next().ok_or("FEN: missing piece placement")?;
        let mut rank: i32 = 7;
        let mut file: i32 = 0;
        for c in placement.chars() {
            match c {
                '/' => {
                    if file != 8 {
                        return Err(format!("FEN: rank {} has {file} files", 8 - rank));
                    }
                    rank -= 1;
                    file = 0;
                    if rank < 0 {
                        return Err("FEN: too many ranks".into());
                    }
                }
                '1'..='8' => file += c as i32 - '0' as i32,
                _ => {
                    let piece = Piece::from_char(c).ok_or(format!("FEN: bad piece '{c}'"))?;
                    if file > 7 || rank < 0 {
                        return Err("FEN: piece placement out of bounds".into());
                    }
                    pos.add_piece(piece, square_of(file as u8, rank as u8));
                    file += 1;
                }
            }
        }
        if rank != 0 || file != 8 {
            return Err("FEN: piece placement is incomplete".into());
        }

        pos.side_to_move = match fields.next().unwrap_or("w") {
            "w" => Color::White,
            "b" => Color::Black,
            other => return Err(format!("FEN: bad side to move '{other}'")),
        };

        let rights = fields.next().unwrap_or("-");
        if rights != "-" {
            for c in rights.chars() {
                pos.castling |= match c {
                    'K' => WHITE_KING_SIDE,
                    'Q' => WHITE_QUEEN_SIDE,
                    'k' => BLACK_KING_SIDE,
                    'q' => BLACK_QUEEN_SIDE,
                    other => return Err(format!("FEN: bad castling right '{other}'")),
                };
            }
        }

        let ep = fields.next().unwrap_or("-");
        pos.en_passant = if ep == "-" {
            None
        } else {
            Some(parse_square(ep).ok_or(format!("FEN: bad en passant square '{ep}'"))?)
        };

        pos.halfmove_clock = fields.next().unwrap_or("0").parse().unwrap_or(0);
        pos.fullmove_number = fields.next().unwrap_or("1").parse().unwrap_or(1);

        for color in [Color::White, Color::Black] {
            if popcount(pos.pieces(color, PieceType::King)) != 1 {
                return Err(format!("FEN: {color:?} must have exactly one king"));
            }
        }

        pos.hash ^= zobrist::castling(pos.castling);
        pos.hash ^= zobrist::side_key(pos.side_to_move);
        if let Some(sq) = pos.en_passant {
            pos.hash ^= zobrist::en_passant(file_of(sq));
        }
        Ok(pos)
    }

    pub fn to_fen(&self) -> String {
        let mut fen = String::new();
        for rank in (0..8).rev() {
            let mut empty = 0;
            for file in 0..8 {
                match self.piece_at(square_of(file, rank)) {
                    Some(piece) => {
                        if empty > 0 {
                            fen.push_str(&empty.to_string());
                            empty = 0;
                        }
                        fen.push(piece.to_char());
                    }
                    None => empty += 1,
                }
            }
            if empty > 0 {
                fen.push_str(&empty.to_string());
            }
            if rank > 0 {
                fen.push('/');
            }
        }

        fen.push(' ');
        fen.push(if self.side_to_move == Color::White {
            'w'
        } else {
            'b'
        });

        fen.push(' ');
        if self.castling == 0 {
            fen.push('-');
        } else {
            for (flag, c) in [
                (WHITE_KING_SIDE, 'K'),
                (WHITE_QUEEN_SIDE, 'Q'),
                (BLACK_KING_SIDE, 'k'),
                (BLACK_QUEEN_SIDE, 'q'),
            ] {
                if self.castling & flag != 0 {
                    fen.push(c);
                }
            }
        }

        fen.push(' ');
        match self.en_passant {
            Some(sq) => fen.push_str(&square_to_string(sq)),
            None => fen.push('-'),
        }
        fen.push_str(&format!(
            " {} {}",
            self.halfmove_clock, self.fullmove_number
        ));
        fen
    }

    /// Parses a UCI move string against this position, resolving the flags
    /// (capture, castle, en passant, promotion) that UCI notation omits.
    pub fn parse_uci_move(&self, text: &str) -> Option<Move> {
        let moves = crate::movegen::generate_legal_moves(&mut self.clone());
        moves.iter().copied().find(|mv| mv.to_string() == text)
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  +-----------------+")?;
        for rank in (0..8).rev() {
            write!(f, "{} |", rank + 1)?;
            for file in 0..8 {
                match self.piece_at(square_of(file, rank)) {
                    Some(piece) => write!(f, " {}", piece.to_char())?,
                    None => write!(f, " .")?,
                }
            }
            writeln!(f, " |")?;
        }
        writeln!(f, "  +-----------------+")?;
        writeln!(f, "    a b c d e f g h")?;
        writeln!(f, "FEN: {}", self.to_fen())?;
        write!(f, "Key: {:016x}", self.hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startpos_roundtrips_through_fen() {
        let pos = Position::startpos();
        assert_eq!(pos.to_fen(), STARTPOS_FEN);
        assert_eq!(popcount(pos.occupied()), 32);
        assert_eq!(pos.king_square(Color::White), parse_square("e1").unwrap());
    }

    #[test]
    fn fen_roundtrips_for_tricky_positions() {
        for fen in [
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
            "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
            "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2",
        ] {
            assert_eq!(Position::from_fen(fen).unwrap().to_fen(), fen);
        }
    }

    #[test]
    fn bad_fen_is_rejected() {
        assert!(Position::from_fen("").is_err());
        assert!(Position::from_fen("8/8/8/8/8/8/8/8 w - - 0 1").is_err()); // no kings
        assert!(Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP w KQkq - 0 1").is_err());
        assert!(Position::from_fen("xnbqkbnr/8/8/8/8/8/8/4K3 w - - 0 1").is_err());
    }

    #[test]
    fn make_unmake_restores_state() {
        let fens = [
            STARTPOS_FEN,
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        ];
        for fen in fens {
            let mut pos = Position::from_fen(fen).unwrap();
            let before = pos.clone();
            for mv in crate::movegen::generate_moves(&pos) {
                if !pos.try_make_move(mv) {
                    continue;
                }
                pos.unmake_move();
                assert_eq!(pos.to_fen(), before.to_fen(), "after {mv}");
                assert_eq!(pos.hash, before.hash, "hash after {mv}");
            }
        }
    }

    #[test]
    fn hash_is_incrementally_consistent() {
        let mut pos = Position::startpos();
        for text in ["e2e4", "c7c5", "e1e2", "d8a5", "e2e1", "a5d8"] {
            let mv = pos.parse_uci_move(text).unwrap();
            pos.make_move(mv);
            let rebuilt = Position::from_fen(&pos.to_fen()).unwrap();
            assert_eq!(pos.hash, rebuilt.hash, "after {text}");
        }
    }

    #[test]
    fn castling_moves_the_rook() {
        let mut pos = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
        let mv = pos.parse_uci_move("e1g1").unwrap();
        assert!(mv.is_castle());
        pos.make_move(mv);
        assert_eq!(pos.to_fen(), "r3k2r/8/8/8/8/8/8/R4RK1 b kq - 1 1");
        pos.unmake_move();
        assert_eq!(pos.to_fen(), "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");

        let mv = pos.parse_uci_move("e1c1").unwrap();
        pos.make_move(mv);
        assert_eq!(pos.to_fen(), "r3k2r/8/8/8/8/8/8/2KR3R b kq - 1 1");
    }

    #[test]
    fn en_passant_removes_the_right_pawn() {
        let mut pos = Position::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2").unwrap();
        let mv = pos.parse_uci_move("e5d6").unwrap();
        assert!(mv.is_en_passant());
        pos.make_move(mv);
        assert_eq!(pos.to_fen(), "4k3/8/3P4/8/8/8/8/4K3 b - - 0 2");
        pos.unmake_move();
        assert_eq!(pos.to_fen(), "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2");
    }

    #[test]
    fn repetition_is_detected() {
        let mut pos = Position::startpos();
        for text in ["g1f3", "g8f6", "f3g1", "f6g8"] {
            pos.make_move(pos.parse_uci_move(text).unwrap());
        }
        assert!(pos.is_repetition());
    }
}
