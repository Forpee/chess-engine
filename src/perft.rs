//! Perft: counts the leaf nodes of the move tree. The standard correctness
//! test for move generation and make/unmake.

use crate::movegen::generate_moves;
use crate::position::Position;
use crate::types::Move;

pub fn perft(pos: &mut Position, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }
    let mut nodes = 0;
    for mv in generate_moves(pos) {
        if !pos.try_make_move(mv) {
            continue;
        }
        // At depth 1 the legality check above already counted the move.
        nodes += if depth == 1 { 1 } else { perft(pos, depth - 1) };
        pos.unmake_move();
    }
    nodes
}

/// Per-root-move breakdown, matching the `divide` output of other engines.
pub fn perft_divide(pos: &mut Position, depth: u32) -> (Vec<(Move, u64)>, u64) {
    let mut results = Vec::new();
    let mut total = 0;
    for mv in generate_moves(pos) {
        if !pos.try_make_move(mv) {
            continue;
        }
        let nodes = perft(pos, depth - 1);
        pos.unmake_move();
        results.push((mv, nodes));
        total += nodes;
    }
    results.sort_by_key(|(mv, _)| mv.to_string());
    (results, total)
}
