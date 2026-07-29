//! A fixed-size transposition table with depth-preferred, aging replacement.

use crate::eval::MATE_IN_MAX_PLY;
use crate::types::Move;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bound {
    /// Score is exact (a PV node).
    Exact,
    /// Score is a lower bound (fail high / beta cutoff).
    Lower,
    /// Score is an upper bound (fail low).
    Upper,
}

#[derive(Clone, Copy)]
struct Entry {
    key: u64,
    mv: Move,
    score: i32,
    depth: i16,
    bound: Bound,
    generation: u8,
}

impl Entry {
    const EMPTY: Entry = Entry {
        key: 0,
        mv: Move::NONE,
        score: 0,
        depth: -1,
        bound: Bound::Exact,
        generation: 0,
    };
}

pub struct TableHit {
    pub mv: Move,
    pub score: i32,
    pub depth: i16,
    pub bound: Bound,
}

pub struct TranspositionTable {
    entries: Vec<Entry>,
    mask: usize,
    generation: u8,
}

impl TranspositionTable {
    /// Allocates the largest power-of-two table that fits in `megabytes`.
    pub fn new(megabytes: usize) -> TranspositionTable {
        let entry_size = std::mem::size_of::<Entry>();
        let wanted = (megabytes.max(1) * 1024 * 1024) / entry_size;
        let count = wanted.next_power_of_two() / if wanted.is_power_of_two() { 1 } else { 2 };
        let count = count.max(1024);
        TranspositionTable {
            entries: vec![Entry::EMPTY; count],
            mask: count - 1,
            generation: 0,
        }
    }

    pub fn resize(&mut self, megabytes: usize) {
        *self = TranspositionTable::new(megabytes);
    }

    pub fn clear(&mut self) {
        self.entries.fill(Entry::EMPTY);
        self.generation = 0;
    }

    /// Marks the start of a new search so older entries become replaceable.
    pub fn new_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    #[inline]
    fn slot(&self, key: u64) -> usize {
        key as usize & self.mask
    }

    pub fn probe(&self, key: u64, ply: usize) -> Option<TableHit> {
        let entry = &self.entries[self.slot(key)];
        if entry.key != key || entry.depth < 0 {
            return None;
        }
        Some(TableHit {
            mv: entry.mv,
            score: score_from_tt(entry.score, ply),
            depth: entry.depth,
            bound: entry.bound,
        })
    }

    pub fn store(&mut self, key: u64, mv: Move, score: i32, depth: i16, bound: Bound, ply: usize) {
        let generation = self.generation;
        let slot = self.slot(key);
        let entry = &mut self.entries[slot];

        // Keep the existing entry only if it is from this search, at least as
        // deep, and not a strictly better (exact) bound arriving now.
        let replace = entry.key != key
            || entry.generation != generation
            || depth >= entry.depth
            || bound == Bound::Exact;
        if !replace {
            return;
        }

        // Never lose a stored move to an entry that has none.
        let mv = if mv.is_none() && entry.key == key {
            entry.mv
        } else {
            mv
        };
        *entry = Entry {
            key,
            mv,
            score: score_to_tt(score, ply),
            depth,
            bound,
            generation,
        };
    }

    /// Permille of the table in use by the current search, for `info hashfull`.
    pub fn hashfull(&self) -> usize {
        let sample = 1000.min(self.entries.len());
        let used = self.entries[..sample]
            .iter()
            .filter(|e| e.depth >= 0 && e.generation == self.generation)
            .count();
        used * 1000 / sample
    }

    /// Walks TT moves from `pos` to recover a principal variation. Used only
    /// as a fallback when the search's own PV is unavailable.
    pub fn extract_pv(&self, pos: &mut crate::position::Position, max_len: usize) -> Vec<Move> {
        let mut pv = Vec::new();
        let mut made = 0;
        while pv.len() < max_len {
            let Some(hit) = self.probe(pos.hash, 0) else {
                break;
            };
            if hit.mv.is_none() || !pos.try_make_move(hit.mv) {
                break;
            }
            made += 1;
            pv.push(hit.mv);
        }
        for _ in 0..made {
            pos.unmake_move();
        }
        pv
    }
}

/// Mate scores are stored relative to the entry's position, not the root, so
/// they stay valid when the same position is reached at a different ply.
#[inline]
fn score_to_tt(score: i32, ply: usize) -> i32 {
    if score >= MATE_IN_MAX_PLY {
        score + ply as i32
    } else if score <= -MATE_IN_MAX_PLY {
        score - ply as i32
    } else {
        score
    }
}

#[inline]
fn score_from_tt(score: i32, ply: usize) -> i32 {
    if score >= MATE_IN_MAX_PLY {
        score - ply as i32
    } else if score <= -MATE_IN_MAX_PLY {
        score + ply as i32
    } else {
        score
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::MATE;
    use crate::types::{Move, move_flags};

    #[test]
    fn stores_and_probes() {
        let mut tt = TranspositionTable::new(1);
        let mv = Move::new(12, 28, move_flags::DOUBLE_PUSH);
        tt.store(0xdead_beef, mv, 42, 5, Bound::Exact, 0);
        let hit = tt.probe(0xdead_beef, 0).expect("stored entry");
        assert_eq!(hit.mv, mv);
        assert_eq!(hit.score, 42);
        assert_eq!(hit.depth, 5);
        assert_eq!(hit.bound, Bound::Exact);
        assert!(tt.probe(0x1234, 0).is_none());
    }

    #[test]
    fn mate_scores_are_ply_adjusted() {
        let mut tt = TranspositionTable::new(1);
        // Search scores are distances from the root: mate in 7 plies seen at
        // ply 4 is mate in 3 from that position, so reaching the same position
        // at the root must read back as mate in 3.
        tt.store(1, Move::NONE, MATE - 7, 6, Bound::Exact, 4);
        assert_eq!(tt.probe(1, 4).unwrap().score, MATE - 7);
        assert_eq!(tt.probe(1, 0).unwrap().score, MATE - 3);
    }

    #[test]
    fn deeper_entries_survive_shallower_ones() {
        let mut tt = TranspositionTable::new(1);
        let deep = Move::new(1, 2, 0);
        tt.store(7, deep, 100, 10, Bound::Lower, 0);
        tt.store(7, Move::new(3, 4, 0), -100, 2, Bound::Lower, 0);
        assert_eq!(tt.probe(7, 0).unwrap().mv, deep);
    }

    #[test]
    fn clearing_empties_the_table() {
        let mut tt = TranspositionTable::new(1);
        tt.store(9, Move::new(1, 2, 0), 0, 1, Bound::Exact, 0);
        tt.clear();
        assert!(tt.probe(9, 0).is_none());
        assert_eq!(tt.hashfull(), 0);
    }
}
