//! Core value types: colors, pieces, squares and the packed move encoding.

use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(u8)]
pub enum Color {
    White = 0,
    Black = 1,
}

impl Color {
    #[inline]
    pub const fn flip(self) -> Color {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }

    #[inline]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[inline]
    pub const fn from_index(i: usize) -> Color {
        if i == 0 { Color::White } else { Color::Black }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(u8)]
pub enum PieceType {
    Pawn = 0,
    Knight = 1,
    Bishop = 2,
    Rook = 3,
    Queen = 4,
    King = 5,
}

pub const PIECE_TYPES: [PieceType; 6] = [
    PieceType::Pawn,
    PieceType::Knight,
    PieceType::Bishop,
    PieceType::Rook,
    PieceType::Queen,
    PieceType::King,
];

impl PieceType {
    #[inline]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[inline]
    pub const fn from_index(i: usize) -> PieceType {
        PIECE_TYPES[i]
    }

    pub const fn to_char(self) -> char {
        match self {
            PieceType::Pawn => 'p',
            PieceType::Knight => 'n',
            PieceType::Bishop => 'b',
            PieceType::Rook => 'r',
            PieceType::Queen => 'q',
            PieceType::King => 'k',
        }
    }

    pub fn from_char(c: char) -> Option<PieceType> {
        Some(match c.to_ascii_lowercase() {
            'p' => PieceType::Pawn,
            'n' => PieceType::Knight,
            'b' => PieceType::Bishop,
            'r' => PieceType::Rook,
            'q' => PieceType::Queen,
            'k' => PieceType::King,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Piece {
    pub color: Color,
    pub piece_type: PieceType,
}

impl Piece {
    #[inline]
    pub const fn new(color: Color, piece_type: PieceType) -> Piece {
        Piece { color, piece_type }
    }

    /// Dense 0..12 index used for zobrist keys and lookup tables.
    #[inline]
    pub const fn index(self) -> usize {
        self.color as usize * 6 + self.piece_type as usize
    }

    pub const fn to_char(self) -> char {
        let c = self.piece_type.to_char();
        match self.color {
            Color::White => c.to_ascii_uppercase(),
            Color::Black => c,
        }
    }

    pub fn from_char(c: char) -> Option<Piece> {
        let piece_type = PieceType::from_char(c)?;
        let color = if c.is_ascii_uppercase() {
            Color::White
        } else {
            Color::Black
        };
        Some(Piece::new(color, piece_type))
    }
}

/// A board square, 0 = a1, 7 = h1, 63 = h8.
pub type Square = u8;

#[inline]
pub const fn square_of(file: u8, rank: u8) -> Square {
    rank * 8 + file
}

#[inline]
pub const fn file_of(sq: Square) -> u8 {
    sq & 7
}

#[inline]
pub const fn rank_of(sq: Square) -> u8 {
    sq >> 3
}

/// Mirrors a square vertically (a1 <-> a8). Used to read white-oriented tables.
#[inline]
pub const fn flip_square(sq: Square) -> Square {
    sq ^ 56
}

pub fn square_to_string(sq: Square) -> String {
    let mut s = String::with_capacity(2);
    s.push((b'a' + file_of(sq)) as char);
    s.push((b'1' + rank_of(sq)) as char);
    s
}

pub fn parse_square(s: &str) -> Option<Square> {
    let bytes = s.as_bytes();
    if bytes.len() != 2 {
        return None;
    }
    let file = bytes[0].checked_sub(b'a')?;
    let rank = bytes[1].checked_sub(b'1')?;
    if file > 7 || rank > 7 {
        return None;
    }
    Some(square_of(file, rank))
}

// Castling rights bit flags.
pub const WHITE_KING_SIDE: u8 = 1;
pub const WHITE_QUEEN_SIDE: u8 = 2;
pub const BLACK_KING_SIDE: u8 = 4;
pub const BLACK_QUEEN_SIDE: u8 = 8;

/// Move flags packed into the high nibble of a `Move`.
pub mod move_flags {
    pub const QUIET: u16 = 0;
    pub const DOUBLE_PUSH: u16 = 1;
    pub const KING_CASTLE: u16 = 2;
    pub const QUEEN_CASTLE: u16 = 3;
    pub const CAPTURE: u16 = 4;
    pub const EN_PASSANT: u16 = 5;
    pub const PROMO_KNIGHT: u16 = 8;
    pub const PROMO_BISHOP: u16 = 9;
    pub const PROMO_ROOK: u16 = 10;
    pub const PROMO_QUEEN: u16 = 11;
    pub const PROMO_CAPTURE_KNIGHT: u16 = 12;
    pub const PROMO_CAPTURE_BISHOP: u16 = 13;
    pub const PROMO_CAPTURE_ROOK: u16 = 14;
    pub const PROMO_CAPTURE_QUEEN: u16 = 15;
}

/// A move packed into 16 bits: `from | to << 6 | flags << 12`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Move(pub u16);

impl Move {
    pub const NONE: Move = Move(0);

    #[inline]
    pub const fn new(from: Square, to: Square, flags: u16) -> Move {
        Move(from as u16 | ((to as u16) << 6) | (flags << 12))
    }

    #[inline]
    pub const fn from(self) -> Square {
        (self.0 & 0x3f) as Square
    }

    #[inline]
    pub const fn to(self) -> Square {
        ((self.0 >> 6) & 0x3f) as Square
    }

    #[inline]
    pub const fn flags(self) -> u16 {
        self.0 >> 12
    }

    #[inline]
    pub const fn is_capture(self) -> bool {
        self.flags() & move_flags::CAPTURE != 0
    }

    #[inline]
    pub const fn is_promotion(self) -> bool {
        self.flags() & 0b1000 != 0
    }

    #[inline]
    pub const fn is_en_passant(self) -> bool {
        self.flags() == move_flags::EN_PASSANT
    }

    #[inline]
    pub const fn is_castle(self) -> bool {
        matches!(
            self.flags(),
            move_flags::KING_CASTLE | move_flags::QUEEN_CASTLE
        )
    }

    #[inline]
    pub const fn is_none(self) -> bool {
        self.0 == 0
    }

    /// The piece a promotion move produces, if any.
    #[inline]
    pub const fn promotion_piece(self) -> Option<PieceType> {
        if !self.is_promotion() {
            return None;
        }
        Some(match self.flags() & 0b11 {
            0 => PieceType::Knight,
            1 => PieceType::Bishop,
            2 => PieceType::Rook,
            _ => PieceType::Queen,
        })
    }
}

impl fmt::Display for Move {
    /// Long algebraic notation, as required by UCI (e.g. `e2e4`, `e7e8q`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_none() {
            return write!(f, "0000");
        }
        write!(
            f,
            "{}{}",
            square_to_string(self.from()),
            square_to_string(self.to())
        )?;
        if let Some(pt) = self.promotion_piece() {
            write!(f, "{}", pt.to_char())?;
        }
        Ok(())
    }
}

impl fmt::Debug for Move {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}
