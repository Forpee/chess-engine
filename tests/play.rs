//! End-to-end tests: the engine must play complete, legal games and find
//! well-known tactics.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chess_engine::movegen::generate_legal_moves;
use chess_engine::position::Position;
use chess_engine::search::{Search, SearchLimits};

fn engine(hash_mb: usize) -> Search {
    chess_engine::init();
    let mut search = Search::new(hash_mb, Arc::new(AtomicBool::new(false)));
    search.silent = true;
    search
}

/// Plays a whole game against itself. Any illegal move, panic or failure to
/// terminate shows up here.
#[test]
fn self_play_reaches_a_terminal_position() {
    let mut search = engine(16);
    let mut pos = Position::startpos();
    let limits = SearchLimits {
        nodes: Some(20_000),
        ..Default::default()
    };

    let mut plies = 0;
    loop {
        let legal = generate_legal_moves(&mut pos);
        if legal.is_empty() || pos.is_draw() || plies >= 300 {
            break;
        }
        let result = search.think(&mut pos, &limits);
        assert!(
            legal.contains(&result.best_move),
            "illegal move {} in {}",
            result.best_move,
            pos.to_fen()
        );
        pos.make_move(result.best_move);
        plies += 1;
    }

    // A 300-ply cap means the game should be well past the opening.
    assert!(
        plies > 20,
        "game ended after only {plies} plies: {}",
        pos.to_fen()
    );
    // The final position must be genuinely finished, not an engine giving up.
    let legal = generate_legal_moves(&mut pos);
    assert!(
        legal.is_empty() || pos.is_draw() || plies == 300,
        "{}",
        pos.to_fen()
    );
}

/// Losing a piece for nothing has to be visible at any sane depth.
#[test]
fn recaptures_material() {
    let mut search = engine(16);
    // White has just played Bxc6; black must take back.
    let mut pos =
        Position::from_fen("r1bqkbnr/pppp1ppp/2B5/4p3/4P3/5N2/PPPP1PPP/RNBQK2R b KQkq - 0 4")
            .unwrap();
    let best = search
        .think(&mut pos, &SearchLimits::fixed_depth(7))
        .best_move;
    assert_eq!(
        chess_engine::types::square_to_string(best.to()),
        "c6",
        "expected a recapture on c6, got {best}"
    );
}

#[test]
fn solves_standard_tactics() {
    // (position, expected move) — the first five are Win At Chess 1-5.
    let cases = [
        (
            "2rr3k/pp3pp1/1nnqbN1p/3pN3/2pP4/2P3Q1/PPB4P/R4RK1 w - - 0 1",
            "g3g6",
        ),
        ("8/7p/5k2/5p2/p1p2P2/Pr1pPK2/1P1R3P/8 b - - 0 1", "b3b2"),
        (
            "5rk1/1ppb3p/p1pb4/6q1/3P1p1r/2P1R2P/PP1BQ1P1/5RKN w - - 0 1",
            "e3g3",
        ),
        (
            "r1bq2rk/pp3pbp/2p1p1pQ/7P/3P4/2PB1N2/PP3PPR/2KR4 w - - 0 1",
            "h6h7",
        ),
        ("5k2/6pp/p1qN4/1p1p4/3P4/2PKP2Q/PP3r2/3R4 b - - 0 1", "c6c4"),
        // Rook endgame: cut the king off along the seventh rank.
        ("7k/p7/1R5K/6r1/6p1/6P1/8/8 w - - 0 1", "b6b7"),
    ];

    let mut search = engine(32);
    let limits = SearchLimits {
        nodes: Some(3_000_000),
        ..Default::default()
    };
    for (fen, expected) in cases {
        let mut pos = Position::from_fen(fen).unwrap();
        search.reset();
        let best = search.think(&mut pos, &limits).best_move;
        assert_eq!(best.to_string(), expected, "in {fen}");
    }
}

/// Forced mates must be found, scored as mates, and reported at the right
/// distance — checked against an independent brute-force mate solver.
#[test]
fn finds_forced_mates() {
    let mut search = engine(32);
    let cases = [
        ("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1", "a1a8", 1),
        // Qxd8# — capturing the defender is mate, not merely check.
        ("3r2k1/5ppp/8/8/8/8/5PPP/3QR1K1 w - - 0 1", "d1d8", 1),
        // 1. Ra8+ Rc8 2. Rxc8#
        ("6k1/2r2ppp/8/8/8/8/5PPP/R5K1 w - - 0 1", "a1a8", 3),
        // 1. Ra6 f6 2. Bxf6+ Kg7 3. Rxa8#
        ("r5rk/5p1p/5R2/4B3/8/8/7P/7K w - - 0 1", "f6a6", 5),
    ];

    for (fen, expected, plies) in cases {
        let mut pos = Position::from_fen(fen).unwrap();
        assert!(
            can_force_mate(&mut pos, plies),
            "no mate in {plies} plies exists in {fen}"
        );
        assert!(
            !can_force_mate(&mut pos, plies - 2),
            "mate is shorter than {plies} plies in {fen}"
        );

        search.reset();
        let result = search.think(&mut pos, &SearchLimits::fixed_depth(plies as u32 + 2));
        assert_eq!(
            result.score,
            chess_engine::eval::MATE - plies,
            "wrong mate distance in {fen}"
        );
        assert_eq!(result.best_move.to_string(), expected, "in {fen}");
    }
}

/// Brute-force: can the side to move force mate within `plies`? Deliberately
/// naive — it shares no code with the search it is checking.
fn can_force_mate(pos: &mut Position, plies: i32) -> bool {
    if plies <= 0 {
        return false;
    }
    for mv in generate_legal_moves(pos) {
        pos.make_move(mv);
        let replies = generate_legal_moves(pos);
        let forced = if replies.is_empty() {
            pos.in_check() // Checkmate, as opposed to stalemate.
        } else {
            plies >= 3
                && replies.iter().all(|&reply| {
                    pos.make_move(reply);
                    let mates = can_force_mate(pos, plies - 2);
                    pos.unmake_move();
                    mates
                })
        };
        pos.unmake_move();
        if forced {
            return true;
        }
    }
    false
}
