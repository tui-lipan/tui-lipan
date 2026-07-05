//! Two-generation bounded cache.
//!
//! Replaces the "evict one arbitrary entry when full" pattern
//! (`remove(map.keys().next())`) that several hot caches used. That pattern keeps
//! a hashbrown table pinned at capacity with continuous remove+insert churn,
//! which accumulates tombstones and makes both `keys().next()` scans and ordinary
//! probes progressively more expensive - the "the more I resize, the heavier it
//! gets" failure mode for the layout-measure caches.
//!
//! Clearing the whole map on overflow avoids the tombstone churn but throws away
//! the entire working set, so a cyclic access pattern (a terminal resize sweeping
//! the same widths back and forth) hits ~0% afterwards.
//!
//! This keeps two generations: lookups check the current generation then the
//! previous one; when the current generation fills, it is demoted to `prev`
//! (dropping the older `prev`) and a fresh `cur` is started. Eviction is O(1)
//! (a map swap + clear), there is no tombstone accumulation, and up to one full
//! prior generation survives so resize-style sweeps keep hitting.
//!
//! Effective capacity is therefore up to `2 * per_generation_cap` live entries.

use std::collections::HashMap;
use std::hash::{BuildHasher, Hash};

pub(crate) struct GenerationalCache<K, V, S> {
    cur: HashMap<K, V, S>,
    prev: HashMap<K, V, S>,
    per_generation_cap: usize,
}

impl<K, V, S> GenerationalCache<K, V, S>
where
    K: Eq + Hash,
    S: BuildHasher + Default,
{
    pub(crate) fn new(per_generation_cap: usize) -> Self {
        Self {
            cur: HashMap::default(),
            prev: HashMap::default(),
            per_generation_cap: per_generation_cap.max(1),
        }
    }

    pub(crate) fn get(&self, key: &K) -> Option<&V> {
        self.cur.get(key).or_else(|| self.prev.get(key))
    }

    pub(crate) fn insert(&mut self, key: K, value: V) {
        if self.cur.len() >= self.per_generation_cap {
            std::mem::swap(&mut self.cur, &mut self.prev);
            self.cur.clear();
        }
        self.cur.insert(key, value);
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.cur.len() + self.prev.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::FxBuildHasher;

    type Cache<K, V> = GenerationalCache<K, V, FxBuildHasher>;

    #[test]
    fn keeps_previous_generation_so_cyclic_access_still_hits() {
        // Working set (2 * cap) exceeds a single generation; a clear-on-overflow
        // cache would hit ~0% on the second sweep, this should hit on `prev`.
        let cap = 8;
        let mut c: Cache<u32, u32> = Cache::new(cap);
        // Fill exactly two generations worth of distinct keys.
        for k in 0..(2 * cap as u32) {
            c.insert(k, k);
        }
        // The most recent `2 * cap` keys should all still be resolvable.
        let mut hits = 0;
        for k in 0..(2 * cap as u32) {
            if c.get(&k).copied() == Some(k) {
                hits += 1;
            }
        }
        assert_eq!(hits, 2 * cap, "both live generations must be retained");
        assert!(c.len() <= 2 * cap);
    }

    #[test]
    fn evicts_oldest_generation_first() {
        let cap = 4;
        let mut c: Cache<u32, u32> = Cache::new(cap);
        // Generation 0: keys 0..4
        for k in 0..4 {
            c.insert(k, k);
        }
        // Generation 1: keys 4..8 (gen 0 demoted to prev, still live)
        for k in 4..8 {
            c.insert(k, k);
        }
        // Generation 2: inserting key 8 swaps, dropping gen 0 (keys 0..4)
        c.insert(8, 8);
        assert_eq!(c.get(&0), None, "oldest generation should be evicted");
        assert_eq!(c.get(&4).copied(), Some(4), "prior generation retained");
        assert_eq!(c.get(&8).copied(), Some(8), "newest entry present");
    }

    #[test]
    fn insert_overwrites_within_current_generation() {
        let mut c: Cache<u32, u32> = Cache::new(8);
        c.insert(1, 10);
        c.insert(1, 20);
        assert_eq!(c.get(&1).copied(), Some(20));
    }
}
