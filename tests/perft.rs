//! Perft regression tests against published node counts.
//!
//! These exercise every corner of move generation and make/unmake: castling
//! rights, en passant (including the discovered-check case), promotions and
//! pinned pieces. The deeper cases are `#[ignore]`d — run them with
//! `cargo test --release -- --ignored`.

use chess_engine::perft::perft;
use chess_engine::position::{Position, STARTPOS_FEN};

const KIWIPETE: &str = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";
const ENDGAME: &str = "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1";
const PROMOTIONS: &str = "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1";
const TALKCHESS: &str = "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8";
const STEVEN: &str = "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10";

fn check(fen: &str, expected: &[(u32, u64)]) {
    chess_engine::init();
    let mut pos = Position::from_fen(fen).expect("valid FEN");
    for &(depth, nodes) in expected {
        assert_eq!(perft(&mut pos, depth), nodes, "perft({depth}) for {fen}");
        // Every node must have been unmade cleanly.
        assert_eq!(pos.to_fen(), fen);
    }
}

#[test]
fn perft_startpos() {
    check(
        STARTPOS_FEN,
        &[(1, 20), (2, 400), (3, 8902), (4, 197_281), (5, 4_865_609)],
    );
}

#[test]
fn perft_kiwipete() {
    check(KIWIPETE, &[(1, 48), (2, 2039), (3, 97_862), (4, 4_085_603)]);
}

#[test]
fn perft_endgame() {
    check(
        ENDGAME,
        &[(1, 14), (2, 191), (3, 2812), (4, 43_238), (5, 674_624)],
    );
}

#[test]
fn perft_promotions() {
    check(PROMOTIONS, &[(1, 6), (2, 264), (3, 9467), (4, 422_333)]);
}

#[test]
fn perft_talkchess() {
    check(
        TALKCHESS,
        &[(1, 44), (2, 1486), (3, 62_379), (4, 2_103_487)],
    );
}

#[test]
fn perft_steven() {
    check(STEVEN, &[(1, 46), (2, 2079), (3, 89_890), (4, 3_894_594)]);
}

/// Positions from Martin Sedlak's test set, each targeting one rule that is
/// easy to get subtly wrong.
#[test]
fn perft_tricky_rules() {
    // En passant captures that would leave the king in check.
    check("3k4/3p4/8/K1P4r/8/8/8/8 b - - 0 1", &[(6, 1_134_888)]);
    check("8/8/4k3/8/2p5/8/B2P2K1/8 w - - 0 1", &[(6, 1_015_133)]);
    // En passant capture giving check.
    check("8/8/1k6/2b5/2pP4/8/5K2/8 b - d3 0 1", &[(6, 1_440_467)]);
    // Castling that gives check, both sides.
    check("5k2/8/8/8/8/8/8/4K2R w K - 0 1", &[(6, 661_072)]);
    check("3k4/8/8/8/8/8/8/R3K3 w Q - 0 1", &[(6, 803_711)]);
    // Castling rights and castling prevented by attacks.
    check(
        "r3k2r/1b4bq/8/8/8/8/7B/R3K2R w KQkq - 0 1",
        &[(4, 1_274_206)],
    );
    check(
        "r3k2r/8/3Q4/8/8/5q2/8/R3K2R b KQkq - 0 1",
        &[(4, 1_720_476)],
    );
    // Promotions: out of check, giving check, and under-promoting to check.
    check("2K2r2/4P3/8/8/8/8/8/3k4 w - - 0 1", &[(6, 3_821_001)]);
    check("4k3/1P6/8/8/8/8/K7/8 w - - 0 1", &[(6, 217_342)]);
    check("8/P1k5/K7/8/8/8/8/8 w - - 0 1", &[(6, 92_683)]);
    // Stalemate and checkmate detection.
    check("K1k5/8/P7/8/8/8/8/8 w - - 0 1", &[(6, 2217)]);
    check("8/k1P5/8/1K6/8/8/8/8 w - - 0 1", &[(7, 567_584)]);
    check("8/8/2k5/5q2/5n2/8/5K2/8 b - - 0 1", &[(4, 23_527)]);
    // Discovered check.
    check("8/8/1P2K3/8/2n5/1q6/8/5k2 b - - 0 1", &[(5, 1_004_658)]);
}

/// Flipping a position's colours must not change its node counts. This checks
/// generation for both sides against each other without needing more constants.
#[test]
fn perft_is_colour_symmetric() {
    chess_engine::init();
    for fen in [
        STARTPOS_FEN,
        KIWIPETE,
        ENDGAME,
        PROMOTIONS,
        TALKCHESS,
        STEVEN,
    ] {
        let mirrored = mirror_fen(fen);
        let mut a = Position::from_fen(fen).expect("valid FEN");
        let mut b = Position::from_fen(&mirrored).expect("valid mirrored FEN");
        for depth in 1..=4 {
            assert_eq!(
                perft(&mut a, depth),
                perft(&mut b, depth),
                "depth {depth}: {fen} vs {mirrored}"
            );
        }
    }
}

/// Reflects a FEN vertically and swaps the colours.
fn mirror_fen(fen: &str) -> String {
    let fields: Vec<&str> = fen.split_whitespace().collect();
    let placement = fields[0]
        .split('/')
        .rev()
        .map(|rank| {
            rank.chars()
                .map(|c| {
                    if c.is_ascii_uppercase() {
                        c.to_ascii_lowercase()
                    } else {
                        c.to_ascii_uppercase()
                    }
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("/");

    let side = if fields[1] == "w" { "b" } else { "w" };
    let castling: String = fields[2]
        .chars()
        .map(|c| {
            if c.is_ascii_uppercase() {
                c.to_ascii_lowercase()
            } else {
                c.to_ascii_uppercase()
            }
        })
        .collect();
    let en_passant = if fields[3] == "-" {
        "-".to_string()
    } else {
        let bytes = fields[3].as_bytes();
        format!("{}{}", bytes[0] as char, (b'1' + b'8' - bytes[1]) as char)
    };

    format!(
        "{placement} {side} {castling} {en_passant} {} {}",
        fields[4], fields[5]
    )
}

#[test]
#[ignore = "slow: run with --release -- --ignored"]
fn perft_deep() {
    check(STARTPOS_FEN, &[(6, 119_060_324)]);
    check(KIWIPETE, &[(5, 193_690_690)]);
    check(ENDGAME, &[(6, 11_030_083)]);
    check(PROMOTIONS, &[(5, 15_833_292)]);
    check(TALKCHESS, &[(5, 89_941_194)]);
    check(STEVEN, &[(5, 164_075_551)]);
}
