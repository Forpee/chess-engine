//! Standard Algebraic Notation: `Nf3`, `exd5`, `O-O`, `e8=Q+`.
//!
//! UCI speaks long algebraic (`g1f3`), which is unambiguous but not how people
//! read or write chess. This module converts in both directions for the
//! interactive play mode.

use crate::movegen::generate_legal_moves;
use crate::position::Position;
use crate::types::{Move, PieceType, Square, file_of, move_flags, rank_of, square_to_string};

/// Renders `mv` in SAN. `pos` must be the position *before* the move; it is
/// left unchanged.
pub fn to_san(pos: &mut Position, mv: Move) -> String {
    let Some(piece) = pos.piece_at(mv.from()) else {
        return mv.to_string();
    };

    let mut san = String::new();
    match mv.flags() {
        move_flags::KING_CASTLE => san.push_str("O-O"),
        move_flags::QUEEN_CASTLE => san.push_str("O-O-O"),
        _ => {
            if piece.piece_type == PieceType::Pawn {
                // Pawn captures are named by their origin file: exd5.
                if mv.is_capture() {
                    san.push((b'a' + file_of(mv.from())) as char);
                    san.push('x');
                }
                san.push_str(&square_to_string(mv.to()));
                if let Some(promoted) = mv.promotion_piece() {
                    san.push('=');
                    san.push(promoted.to_char().to_ascii_uppercase());
                }
            } else {
                san.push(piece.piece_type.to_char().to_ascii_uppercase());
                san.push_str(&disambiguation(pos, mv, piece.piece_type));
                if mv.is_capture() {
                    san.push('x');
                }
                san.push_str(&square_to_string(mv.to()));
            }
        }
    }

    // Check and mate suffixes are part of the notation.
    pos.make_move(mv);
    let opponent_has_moves = !generate_legal_moves(pos).is_empty();
    if pos.in_check() {
        san.push(if opponent_has_moves { '+' } else { '#' });
    }
    pos.unmake_move();

    san
}

/// The smallest origin hint that separates `mv` from other identical-looking
/// moves: nothing, a file, a rank, or the whole square.
fn disambiguation(pos: &mut Position, mv: Move, piece_type: PieceType) -> String {
    let rivals: Vec<Square> = generate_legal_moves(pos)
        .into_iter()
        .filter(|&other| {
            other.to() == mv.to()
                && other.from() != mv.from()
                && pos.piece_at(other.from()).map(|p| p.piece_type) == Some(piece_type)
        })
        .map(|other| other.from())
        .collect();

    if rivals.is_empty() {
        return String::new();
    }
    if !rivals
        .iter()
        .any(|&from| file_of(from) == file_of(mv.from()))
    {
        return ((b'a' + file_of(mv.from())) as char).to_string();
    }
    if !rivals
        .iter()
        .any(|&from| rank_of(from) == rank_of(mv.from()))
    {
        return ((b'1' + rank_of(mv.from())) as char).to_string();
    }
    square_to_string(mv.from())
}

/// Parses a move written in SAN or in UCI's long algebraic form.
///
/// Rather than implement a SAN grammar, this renders every legal move and
/// looks for a match, which makes accepting sloppy input (missing check marks,
/// `0-0`, wrong case) a matter of normalising both sides the same way.
pub fn parse(pos: &mut Position, text: &str) -> Option<Move> {
    let wanted = normalize(text);
    if wanted.is_empty() {
        return None;
    }

    let legal = generate_legal_moves(pos);
    let mut case_insensitive = Vec::new();

    for &mv in &legal {
        let san = normalize(&to_san(pos, mv));
        if san == wanted || mv.to_string() == wanted {
            return Some(mv);
        }
        if san.eq_ignore_ascii_case(&wanted) || mv.to_string().eq_ignore_ascii_case(&wanted) {
            case_insensitive.push(mv);
        }
    }

    // "nf3" or "E2E4" are only accepted when they can mean one thing.
    match case_insensitive.as_slice() {
        [only] => Some(*only),
        _ => None,
    }
}

/// Strips decoration that carries no information about which move was meant.
fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.trim().chars() {
        match c {
            '+' | '#' | '!' | '?' | '=' | ' ' | '-' => {}
            // 0-0 is a common way to write castling.
            '0' => out.push('O'),
            'o' => out.push('O'),
            _ => out.push(c),
        }
    }
    out
}

/// Renders a whole line, e.g. a principal variation. `pos` is left unchanged.
pub fn line_to_san(pos: &mut Position, moves: &[Move]) -> String {
    let mut parts = Vec::new();
    let mut made = 0;
    let mut number = pos.fullmove_number;
    let mut white_to_move = pos.side_to_move == crate::types::Color::White;

    for &mv in moves {
        if pos.piece_at(mv.from()).is_none() {
            break; // Defensive: a stale line from the table.
        }
        let san = to_san(pos, mv);
        parts.push(if white_to_move {
            format!("{number}. {san}")
        } else if parts.is_empty() {
            format!("{number}... {san}")
        } else {
            san
        });
        if !white_to_move {
            number += 1;
        }
        white_to_move = !white_to_move;
        pos.make_move(mv);
        made += 1;
    }
    for _ in 0..made {
        pos.unmake_move();
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::STARTPOS_FEN;

    fn pos(fen: &str) -> Position {
        crate::init();
        Position::from_fen(fen).unwrap()
    }

    fn san_of(fen: &str, uci: &str) -> String {
        let mut p = pos(fen);
        let mv = p.parse_uci_move(uci).expect("legal move");
        to_san(&mut p, mv)
    }

    #[test]
    fn renders_basic_moves() {
        assert_eq!(san_of(STARTPOS_FEN, "e2e4"), "e4");
        assert_eq!(san_of(STARTPOS_FEN, "g1f3"), "Nf3");
    }

    #[test]
    fn renders_captures() {
        let fen = "rnbqkbnr/ppp1pppp/8/3p4/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2";
        assert_eq!(san_of(fen, "e4d5"), "exd5");
        let fen = "rnbqkbnr/ppp1pppp/8/3P4/8/8/PPPP1PPP/RNBQKBNR b KQkq - 0 2";
        assert_eq!(san_of(fen, "d8d5"), "Qxd5");
    }

    #[test]
    fn renders_castling_and_promotion() {
        let fen = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";
        assert_eq!(san_of(fen, "e1g1"), "O-O");
        assert_eq!(san_of(fen, "e1c1"), "O-O-O");
        assert_eq!(san_of("8/4P3/8/8/8/8/8/K6k w - - 0 1", "e7e8q"), "e8=Q");
        assert_eq!(san_of("8/4P3/8/8/8/8/8/K6k w - - 0 1", "e7e8n"), "e8=N");
    }

    #[test]
    fn marks_check_and_mate() {
        assert_eq!(san_of("4k3/8/8/8/8/8/4R3/4K3 w - - 0 1", "e2e7"), "Re7+");
        assert_eq!(san_of("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1", "a1a8"), "Ra8#");
    }

    #[test]
    fn disambiguates_only_as_much_as_needed() {
        // Two knights reach d2 from different files.
        assert_eq!(san_of("4k3/8/8/8/8/8/8/1N2KN2 w - - 0 1", "b1d2"), "Nbd2");
        // A lone knight needs no hint at all.
        assert_eq!(san_of("4k3/8/8/8/8/8/8/1N2K3 w - - 0 1", "b1d2"), "Nd2");
        // Two rooks on the same file need the rank.
        assert_eq!(san_of("4k3/8/8/8/R7/8/8/R3K3 w - - 0 1", "a1a3"), "R1a3");
        // Three queens all bear on d4: a1 shares its file with a4 and its rank
        // with d1, so only the full square identifies it.
        let fen = "8/8/7k/8/Q7/8/8/Q2QK3 w - - 0 1";
        assert_eq!(san_of(fen, "a1d4"), "Qa1d4");
        assert_eq!(san_of(fen, "a4d4"), "Q4d4"); // Rank is enough.
        assert_eq!(san_of(fen, "d1d4"), "Qdd4"); // File is enough.
    }

    #[test]
    fn parses_what_it_renders() {
        // Round-trip every legal move in a set of busy positions.
        for fen in [
            STARTPOS_FEN,
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
            "4k3/8/8/Q7/8/8/8/Q3K2Q w - - 0 1",
        ] {
            let mut p = pos(fen);
            for mv in generate_legal_moves(&mut p) {
                let san = to_san(&mut p, mv);
                assert_eq!(parse(&mut p, &san), Some(mv), "{san} in {fen}");
                assert_eq!(parse(&mut p, &mv.to_string()), Some(mv), "{mv} in {fen}");
            }
        }
    }

    #[test]
    fn accepts_sloppy_input() {
        let mut p = pos("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
        let castle = p.parse_uci_move("e1g1").unwrap();
        for text in ["O-O", "0-0", "o-o", "e1g1", "  O-O  "] {
            assert_eq!(parse(&mut p, text), Some(castle), "{text}");
        }

        let mut p = pos(STARTPOS_FEN);
        let knight = p.parse_uci_move("g1f3").unwrap();
        assert_eq!(parse(&mut p, "nf3"), Some(knight));
        assert_eq!(parse(&mut p, "Nf3+"), Some(knight));
    }

    #[test]
    fn rejects_illegal_and_ambiguous_input() {
        let mut p = pos(STARTPOS_FEN);
        assert_eq!(parse(&mut p, "e5"), None); // Not legal yet.
        assert_eq!(parse(&mut p, "hello"), None);
        assert_eq!(parse(&mut p, ""), None);
        // Both bishops reach d3, so bare "Bd3" names neither of them.
        let mut p = pos("4k3/8/8/8/8/8/8/1B2KB2 w - - 0 1");
        assert_eq!(parse(&mut p, "Bd3"), None);
        assert!(parse(&mut p, "Bbd3").is_some());
        assert!(parse(&mut p, "Bfd3").is_some());
    }

    #[test]
    fn renders_a_line_with_move_numbers() {
        let mut p = pos(STARTPOS_FEN);
        let moves: Vec<Move> = ["e2e4", "e7e5", "g1f3"]
            .iter()
            .map(|uci| {
                let mv = p.parse_uci_move(uci).unwrap();
                p.make_move(mv);
                mv
            })
            .collect();
        for _ in 0..moves.len() {
            p.unmake_move();
        }
        assert_eq!(line_to_san(&mut p, &moves), "1. e4 e5 2. Nf3");
        assert_eq!(p.to_fen(), STARTPOS_FEN);
    }
}
