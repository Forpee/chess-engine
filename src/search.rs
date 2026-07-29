//! Iterative-deepening alpha-beta search.
//!
//! Principal variation search over a transposition table, with quiescence at
//! the horizon, null-move pruning, late move reductions, killer/history move
//! ordering and aspiration windows.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::eval::{self, INFINITY, MATE, MATE_IN_MAX_PLY};
use crate::movegen::{MAX_MOVES, MoveList, generate_captures, generate_moves};
use crate::position::Position;
use crate::tt::{Bound, TranspositionTable};
use crate::types::{Color, Move, PieceType};

pub const MAX_PLY: usize = 128;
/// Assumed GUI/transport latency, reserved out of every time budget.
const MOVE_OVERHEAD: u64 = 30;

#[derive(Clone, Debug)]
pub struct SearchLimits {
    pub depth: u32,
    pub movetime: Option<u64>,
    pub time: [Option<u64>; 2],
    pub increment: [u64; 2],
    pub moves_to_go: Option<u32>,
    pub nodes: Option<u64>,
    pub infinite: bool,
}

impl Default for SearchLimits {
    fn default() -> SearchLimits {
        SearchLimits {
            depth: MAX_PLY as u32 - 1,
            movetime: None,
            time: [None, None],
            increment: [0, 0],
            moves_to_go: None,
            nodes: None,
            infinite: false,
        }
    }
}

impl SearchLimits {
    pub fn fixed_depth(depth: u32) -> SearchLimits {
        SearchLimits {
            depth,
            ..Default::default()
        }
    }

    pub fn fixed_time(millis: u64) -> SearchLimits {
        SearchLimits {
            movetime: Some(millis),
            ..Default::default()
        }
    }

    /// Splits the clock into a target time to start the next iteration (soft)
    /// and a ceiling that aborts the current one (hard).
    fn budget(&self, side: Color) -> (Option<Duration>, Option<Duration>) {
        if self.infinite {
            return (None, None);
        }
        if let Some(movetime) = self.movetime {
            let ms = movetime.saturating_sub(MOVE_OVERHEAD).max(1);
            return (
                Some(Duration::from_millis(ms)),
                Some(Duration::from_millis(ms)),
            );
        }
        let Some(remaining) = self.time[side.index()] else {
            return (None, None);
        };

        let remaining = remaining.saturating_sub(MOVE_OVERHEAD).max(1);
        let increment = self.increment[side.index()];
        let soft = match self.moves_to_go {
            Some(moves) => remaining / u64::from(moves).clamp(1, 40) + increment / 2,
            None => remaining / 25 + increment / 2,
        };
        // Never commit more than a third of what is left to a single move.
        let hard = (remaining / 3).max(1);
        (
            Some(Duration::from_millis(soft.clamp(1, hard))),
            Some(Duration::from_millis(hard)),
        )
    }
}

/// Triangular PV table: `moves[ply]` holds the line found at that ply.
struct PvTable {
    moves: [[Move; MAX_PLY]; MAX_PLY],
    len: [usize; MAX_PLY],
}

impl PvTable {
    fn new() -> Box<PvTable> {
        Box::new(PvTable {
            moves: [[Move::NONE; MAX_PLY]; MAX_PLY],
            len: [0; MAX_PLY],
        })
    }

    #[inline]
    fn clear(&mut self, ply: usize) {
        self.len[ply] = 0;
    }

    /// Prepends `mv` to the child's line and stores it as this ply's PV.
    fn update(&mut self, ply: usize, mv: Move) {
        self.moves[ply][0] = mv;
        let child_len = if ply + 1 < MAX_PLY {
            self.len[ply + 1]
        } else {
            0
        };
        let copy_len = child_len.min(MAX_PLY - ply - 2);
        for i in 0..copy_len {
            self.moves[ply][i + 1] = self.moves[ply + 1][i];
        }
        self.len[ply] = copy_len + 1;
    }

    fn line(&self, ply: usize) -> &[Move] {
        &self.moves[ply][..self.len[ply]]
    }
}

/// Reduction amounts for late move reductions, indexed [depth][move number].
static REDUCTIONS: std::sync::LazyLock<[[i32; 64]; 64]> = std::sync::LazyLock::new(|| {
    let mut table = [[0i32; 64]; 64];
    for (depth, row) in table.iter_mut().enumerate().skip(1) {
        for (moves, entry) in row.iter_mut().enumerate().skip(1) {
            *entry = (0.75 + (depth as f64).ln() * (moves as f64).ln() / 2.25) as i32;
        }
    }
    table
});

pub struct Search {
    pub tt: TranspositionTable,
    stop: Arc<AtomicBool>,
    killers: [[Move; 2]; MAX_PLY],
    history: Box<[[[i32; 64]; 64]; 2]>,
    pv: Box<PvTable>,
    nodes: u64,
    seldepth: usize,
    start: Instant,
    soft_limit: Option<Duration>,
    hard_limit: Option<Duration>,
    node_limit: Option<u64>,
    stopped: bool,
    /// Suppresses `info` output; used by tests and fixed-depth analysis.
    pub silent: bool,
}

/// What a completed search found.
#[derive(Clone, Debug)]
pub struct SearchResult {
    pub best_move: Move,
    pub ponder_move: Option<Move>,
    pub score: i32,
    pub depth: u32,
    pub nodes: u64,
}

impl Search {
    pub fn new(hash_mb: usize, stop: Arc<AtomicBool>) -> Search {
        Search {
            tt: TranspositionTable::new(hash_mb),
            stop,
            killers: [[Move::NONE; 2]; MAX_PLY],
            history: Box::new([[[0; 64]; 64]; 2]),
            pv: PvTable::new(),
            nodes: 0,
            seldepth: 0,
            start: Instant::now(),
            soft_limit: None,
            hard_limit: None,
            node_limit: None,
            stopped: false,
            silent: false,
        }
    }

    /// Clears everything that must not leak between games.
    pub fn reset(&mut self) {
        self.tt.clear();
        self.killers = [[Move::NONE; 2]; MAX_PLY];
        *self.history = [[[0; 64]; 64]; 2];
    }

    /// Runs iterative deepening and returns the best move found.
    pub fn think(&mut self, pos: &mut Position, limits: &SearchLimits) -> SearchResult {
        let (soft, hard) = limits.budget(pos.side_to_move);
        self.start = Instant::now();
        self.soft_limit = soft;
        self.hard_limit = hard;
        self.node_limit = limits.nodes;
        self.nodes = 0;
        self.stopped = false;
        self.stop.store(false, Ordering::Relaxed);
        self.tt.new_generation();

        // Fall back to any legal move so we never emit an illegal bestmove.
        let legal = crate::movegen::generate_legal_moves(pos);
        let mut result = SearchResult {
            best_move: legal.first().copied().unwrap_or(Move::NONE),
            ponder_move: None,
            score: 0,
            depth: 0,
            nodes: 0,
        };
        if legal.len() <= 1 {
            // Still report something sensible, but don't burn the clock.
            if let Some(&mv) = legal.first() {
                result.best_move = mv;
                if !limits.infinite {
                    self.report(1, eval::evaluate(pos), &[mv]);
                    return result;
                }
            } else {
                return result;
            }
        }

        let mut score = 0;
        for depth in 1..=limits.depth.min(MAX_PLY as u32 - 1) {
            self.seldepth = 0;
            let iteration_score = self.aspiration_search(pos, depth, score);
            if self.stopped {
                break;
            }
            score = iteration_score;

            let line = self.pv.line(0);
            if let Some(&best) = line.first() {
                result.best_move = best;
                result.ponder_move = line.get(1).copied();
                result.score = score;
                result.depth = depth;
            }
            self.report(depth, score, self.pv.line(0));

            // Mate found: deeper search cannot improve on it.
            if score.abs() >= MATE_IN_MAX_PLY && depth as i32 >= MATE - score.abs() {
                break;
            }
            // Starting another iteration we cannot finish just wastes time.
            if let Some(soft) = self.soft_limit
                && self.start.elapsed() >= soft.mul_f64(0.6)
            {
                break;
            }
        }

        result.nodes = self.nodes;
        result
    }

    /// Re-searches with a widening window until the score lands inside it.
    fn aspiration_search(&mut self, pos: &mut Position, depth: u32, previous: i32) -> i32 {
        let mut window = 25;
        let (mut alpha, mut beta) = if depth >= 4 && previous.abs() < MATE_IN_MAX_PLY {
            (previous - window, previous + window)
        } else {
            (-INFINITY, INFINITY)
        };

        loop {
            let score = self.negamax(pos, depth as i32, alpha, beta, 0, true);
            if self.stopped {
                return score;
            }
            if score <= alpha {
                beta = (alpha + beta) / 2;
                alpha = (score - window).max(-INFINITY);
            } else if score >= beta {
                beta = (score + window).min(INFINITY);
            } else {
                return score;
            }
            window += window / 2;
        }
    }

    fn report(&self, depth: u32, score: i32, pv: &[Move]) {
        if self.silent {
            return;
        }
        let elapsed = self.start.elapsed();
        let millis = elapsed.as_millis().max(1) as u64;
        let nps = self.nodes * 1000 / millis;
        let line: Vec<String> = pv.iter().map(|mv| mv.to_string()).collect();
        println!(
            "info depth {depth} seldepth {} score {} nodes {} nps {nps} hashfull {} time {millis} pv {}",
            self.seldepth.max(depth as usize),
            eval::format_score(score),
            self.nodes,
            self.tt.hashfull(),
            line.join(" ")
        );
    }

    /// Polls the stop flag, clock and node limit. Called on a node interval so
    /// the syscall cost stays negligible.
    #[inline]
    fn check_limits(&mut self) {
        if self.stop.load(Ordering::Relaxed) {
            self.stopped = true;
            return;
        }
        if let Some(limit) = self.node_limit
            && self.nodes >= limit
        {
            self.stopped = true;
            return;
        }
        if let Some(limit) = self.hard_limit
            && self.start.elapsed() >= limit
        {
            self.stopped = true;
        }
    }

    fn negamax(
        &mut self,
        pos: &mut Position,
        mut depth: i32,
        mut alpha: i32,
        mut beta: i32,
        ply: usize,
        is_pv: bool,
    ) -> i32 {
        self.pv.clear(ply);
        if self.nodes.is_multiple_of(2048) {
            self.check_limits();
        }
        if self.stopped {
            return 0;
        }

        let root = ply == 0;
        if !root {
            if pos.is_draw() {
                return 0;
            }
            if ply >= MAX_PLY - 1 {
                return eval::evaluate(pos);
            }
            // Mate distance pruning: a shorter mate elsewhere bounds this node.
            alpha = alpha.max(-MATE + ply as i32);
            beta = beta.min(MATE - ply as i32 - 1);
            if alpha >= beta {
                return alpha;
            }
        }

        let in_check = pos.in_check();
        if in_check {
            depth += 1; // Check extension.
        }
        if depth <= 0 {
            return self.quiescence(pos, alpha, beta, ply);
        }

        self.nodes += 1;

        let tt_hit = self.tt.probe(pos.hash, ply);
        let mut tt_move = Move::NONE;
        if let Some(hit) = &tt_hit {
            tt_move = hit.mv;
            if !is_pv && hit.depth as i32 >= depth {
                let usable = match hit.bound {
                    Bound::Exact => true,
                    Bound::Lower => hit.score >= beta,
                    Bound::Upper => hit.score <= alpha,
                };
                if usable {
                    return hit.score;
                }
            }
        }

        let static_eval = if in_check {
            -INFINITY
        } else {
            eval::evaluate(pos)
        };

        if !is_pv && !in_check && beta.abs() < MATE_IN_MAX_PLY {
            // Reverse futility: so far ahead that giving up a margin still wins.
            if depth <= 6 && static_eval - 85 * depth >= beta {
                return static_eval;
            }

            // Null move: if passing still fails high, the real move will too.
            if depth >= 3 && static_eval >= beta && pos.has_non_pawn_material(pos.side_to_move) {
                let reduction = 3 + depth / 5;
                pos.make_null_move();
                let score = -self.negamax(pos, depth - reduction, -beta, -beta + 1, ply + 1, false);
                pos.unmake_null_move();
                if self.stopped {
                    return 0;
                }
                if score >= beta {
                    // Don't return unproven mate scores from a null search.
                    return if score >= MATE_IN_MAX_PLY {
                        beta
                    } else {
                        score
                    };
                }
            }
        }

        let mut moves = generate_moves(pos);
        let scores = self.score_moves(pos, &moves, tt_move, ply);
        let mut ordered = MoveOrder::new(&mut moves, scores);

        let mut best_score = -INFINITY;
        let mut best_move = Move::NONE;
        let mut bound = Bound::Upper;
        let mut legal_moves = 0;
        let mut quiets_tried: Vec<Move> = Vec::new();

        while let Some(mv) = ordered.next() {
            if !pos.try_make_move(mv) {
                continue;
            }
            legal_moves += 1;
            let quiet = !mv.is_capture() && !mv.is_promotion();
            if quiet {
                quiets_tried.push(mv);
            }

            let mut score;
            if legal_moves == 1 {
                score = -self.negamax(pos, depth - 1, -beta, -alpha, ply + 1, is_pv);
            } else {
                // Late move reductions: search likely-bad quiet moves shallower.
                let reduction = if depth >= 3 && legal_moves > 3 && quiet && !in_check {
                    let base = REDUCTIONS[(depth as usize).min(63)][legal_moves.min(63)];
                    (base - i32::from(is_pv)).clamp(0, depth - 2)
                } else {
                    0
                };

                score = -self.negamax(
                    pos,
                    depth - 1 - reduction,
                    -alpha - 1,
                    -alpha,
                    ply + 1,
                    false,
                );
                if score > alpha && reduction > 0 {
                    score = -self.negamax(pos, depth - 1, -alpha - 1, -alpha, ply + 1, false);
                }
                if score > alpha && score < beta {
                    score = -self.negamax(pos, depth - 1, -beta, -alpha, ply + 1, is_pv);
                }
            }
            pos.unmake_move();

            if self.stopped {
                return 0;
            }

            if score > best_score {
                best_score = score;
                best_move = mv;
                if score > alpha {
                    alpha = score;
                    bound = Bound::Exact;
                    self.pv.update(ply, mv);
                }
                if alpha >= beta {
                    bound = Bound::Lower;
                    if quiet {
                        self.record_cutoff(pos.side_to_move, mv, depth, ply, &quiets_tried);
                    }
                    break;
                }
            }
        }

        if legal_moves == 0 {
            return if in_check { -MATE + ply as i32 } else { 0 };
        }

        self.tt
            .store(pos.hash, best_move, best_score, depth as i16, bound, ply);
        best_score
    }

    /// Searches captures until the position is quiet, so the evaluation is not
    /// taken in the middle of an exchange.
    fn quiescence(&mut self, pos: &mut Position, mut alpha: i32, beta: i32, ply: usize) -> i32 {
        if self.nodes.is_multiple_of(2048) {
            self.check_limits();
        }
        if self.stopped {
            return 0;
        }
        self.nodes += 1;
        self.seldepth = self.seldepth.max(ply);

        if pos.is_draw() {
            return 0;
        }
        if ply >= MAX_PLY - 1 {
            return eval::evaluate(pos);
        }

        let stand_pat = eval::evaluate(pos);
        if stand_pat >= beta {
            return stand_pat;
        }
        alpha = alpha.max(stand_pat);

        let mut moves = generate_captures(pos);
        let scores = self.score_moves(pos, &moves, Move::NONE, ply);
        let mut ordered = MoveOrder::new(&mut moves, scores);

        let mut best = stand_pat;
        while let Some(mv) = ordered.next() {
            // Delta pruning: even winning this material would not reach alpha.
            if !mv.is_promotion() {
                let captured = pos
                    .piece_at(mv.to())
                    .map(|p| eval::piece_value(p.piece_type))
                    .unwrap_or(eval::piece_value(PieceType::Pawn));
                if stand_pat + captured + 200 < alpha {
                    continue;
                }
            }
            if !pos.try_make_move(mv) {
                continue;
            }
            let score = -self.quiescence(pos, -beta, -alpha, ply + 1);
            pos.unmake_move();

            if self.stopped {
                return 0;
            }
            if score > best {
                best = score;
                if score > alpha {
                    alpha = score;
                }
                if alpha >= beta {
                    break;
                }
            }
        }
        best
    }

    /// Remembers a quiet move that caused a cutoff, and penalises the quiet
    /// moves that were tried before it.
    fn record_cutoff(&mut self, side: Color, mv: Move, depth: i32, ply: usize, tried: &[Move]) {
        if self.killers[ply][0] != mv {
            self.killers[ply][1] = self.killers[ply][0];
            self.killers[ply][0] = mv;
        }
        let bonus = (depth * depth).min(400);
        let table = &mut self.history[side.index()];
        for &other in tried {
            let entry = &mut table[other.from() as usize][other.to() as usize];
            *entry += if other == mv { bonus } else { -bonus };
            *entry = (*entry).clamp(-HISTORY_MAX, HISTORY_MAX);
        }
    }

    fn score_moves(
        &self,
        pos: &Position,
        moves: &[Move],
        tt_move: Move,
        ply: usize,
    ) -> [i32; MAX_MOVES] {
        let mut scores = [0i32; MAX_MOVES];
        let history = &self.history[pos.side_to_move.index()];
        for (i, &mv) in moves.iter().enumerate() {
            scores[i] = if mv == tt_move {
                i32::MAX
            } else if mv.is_capture() || mv.is_promotion() {
                let victim = if mv.is_en_passant() {
                    eval::piece_value(PieceType::Pawn)
                } else {
                    pos.piece_at(mv.to())
                        .map(|p| eval::piece_value(p.piece_type))
                        .unwrap_or(0)
                };
                let attacker = pos
                    .piece_at(mv.from())
                    .map(|p| eval::piece_value(p.piece_type))
                    .unwrap_or(0);
                let promo = mv.promotion_piece().map_or(0, eval::piece_value);
                // MVV-LVA: take the most valuable victim with the least valuable piece.
                CAPTURE_BASE + victim * 16 - attacker + promo
            } else if mv == self.killers[ply][0] {
                KILLER_BASE
            } else if mv == self.killers[ply][1] {
                KILLER_BASE - 1
            } else {
                history[mv.from() as usize][mv.to() as usize]
            };
        }
        scores
    }
}

const CAPTURE_BASE: i32 = 1 << 24;
const KILLER_BASE: i32 = 1 << 23;
const HISTORY_MAX: i32 = (1 << 22) - 1;

/// Yields moves best-first by selection sort, so nodes that cut off early
/// never pay to order the whole list.
struct MoveOrder<'a> {
    moves: &'a mut [Move],
    scores: [i32; MAX_MOVES],
    index: usize,
}

impl<'a> MoveOrder<'a> {
    fn new(moves: &'a mut MoveList, scores: [i32; MAX_MOVES]) -> MoveOrder<'a> {
        MoveOrder {
            moves,
            scores,
            index: 0,
        }
    }

    #[allow(clippy::should_implement_trait)]
    fn next(&mut self) -> Option<Move> {
        if self.index >= self.moves.len() {
            return None;
        }
        let mut best = self.index;
        for i in self.index + 1..self.moves.len() {
            if self.scores[i] > self.scores[best] {
                best = i;
            }
        }
        self.moves.swap(self.index, best);
        self.scores.swap(self.index, best);
        let mv = self.moves[self.index];
        self.index += 1;
        Some(mv)
    }
}

/// Fixed-depth search over a set of positions; a reproducible speed and
/// node-count baseline for comparing engine versions.
pub fn bench(depth: u32) -> u64 {
    const POSITIONS: [&str; 8] = [
        crate::position::STARTPOS_FEN,
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
        "4rrk1/pp1n1pp1/3bp2p/3p3P/3P1P2/2N1P3/PPQ2P2/2KR2R1 w - - 0 22",
        "8/8/8/2k5/8/2K1R3/8/8 w - - 0 1",
    ];

    crate::init();
    let stop = Arc::new(AtomicBool::new(false));
    let mut search = Search::new(64, stop);
    search.silent = true;

    let start = Instant::now();
    let mut nodes = 0;
    for fen in POSITIONS {
        let mut pos = Position::from_fen(fen).expect("bench FEN is valid");
        search.reset();
        let result = search.think(&mut pos, &SearchLimits::fixed_depth(depth));
        nodes += result.nodes;
    }
    let millis = start.elapsed().as_millis().max(1) as u64;
    println!("{nodes} nodes {} nps ({millis} ms)", nodes * 1000 / millis);
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search_fen(fen: &str, depth: u32) -> SearchResult {
        crate::init();
        let mut pos = Position::from_fen(fen).unwrap();
        let mut search = Search::new(16, Arc::new(AtomicBool::new(false)));
        search.silent = true;
        search.think(&mut pos, &SearchLimits::fixed_depth(depth))
    }

    #[test]
    fn finds_mate_in_one() {
        let result = search_fen("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1", 3);
        assert_eq!(result.best_move.to_string(), "a1a8");
        assert_eq!(result.score, MATE - 1);
    }

    #[test]
    fn finds_a_mate_that_removes_the_defender() {
        // Qxd8# — the rook guarding the back rank is the one being captured.
        let result = search_fen("3r2k1/5ppp/8/8/8/8/5PPP/3QR1K1 w - - 0 1", 6);
        assert_eq!(result.best_move.to_string(), "d1d8");
        assert_eq!(result.score, MATE - 1);
    }

    #[test]
    fn finds_mate_in_two() {
        // 1. Ra8+ Rc8 2. Rxc8#
        let result = search_fen("6k1/2r2ppp/8/8/8/8/5PPP/R5K1 w - - 0 1", 5);
        assert_eq!(result.best_move.to_string(), "a1a8");
        assert_eq!(result.score, MATE - 3);
    }

    #[test]
    fn detects_being_mated() {
        // Black is checkmated; white to move has already won.
        let result = search_fen("R5k1/5ppp/8/8/8/8/8/6K1 b - - 0 1", 2);
        assert_eq!(result.best_move, Move::NONE);
    }

    #[test]
    fn stalemate_scores_zero() {
        let result = search_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1", 2);
        assert_eq!(result.best_move, Move::NONE);
    }

    #[test]
    fn wins_hanging_material() {
        // The undefended queen on d5 must be taken.
        let result = search_fen("4k3/8/8/3q4/4P3/8/8/4K3 w - - 0 1", 6);
        assert_eq!(result.best_move.to_string(), "e4d5");
    }

    #[test]
    fn avoids_losing_the_queen() {
        // Qd1xd8 loses the queen to the rook; the engine must not play it.
        let result = search_fen("3rk3/8/8/8/8/8/8/3QK3 w - - 0 1", 6);
        assert_ne!(result.best_move.to_string(), "d1d8");
    }

    #[test]
    fn deeper_search_returns_a_legal_move() {
        let result = search_fen(
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            7,
        );
        let mut pos = Position::from_fen(
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        )
        .unwrap();
        assert!(crate::movegen::generate_legal_moves(&mut pos).contains(&result.best_move));
        assert!(result.depth >= 7);
    }

    #[test]
    fn stop_flag_ends_an_infinite_search() {
        crate::init();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let handle = std::thread::spawn(move || {
            let mut pos = Position::startpos();
            let mut search = Search::new(16, flag);
            search.silent = true;
            let limits = SearchLimits {
                infinite: true,
                ..Default::default()
            };
            search.think(&mut pos, &limits)
        });
        std::thread::sleep(Duration::from_millis(150));
        stop.store(true, Ordering::Relaxed);
        let result = handle.join().unwrap();
        assert!(!result.best_move.is_none());
    }

    #[test]
    fn respects_a_movetime_limit() {
        crate::init();
        let mut pos = Position::startpos();
        let mut search = Search::new(16, Arc::new(AtomicBool::new(false)));
        search.silent = true;
        let start = Instant::now();
        search.think(&mut pos, &SearchLimits::fixed_time(300));
        assert!(start.elapsed() < Duration::from_millis(1500));
    }
}
