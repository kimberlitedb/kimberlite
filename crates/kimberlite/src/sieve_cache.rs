//! SIEVE eviction cache — simpler and higher hit-rate than LRU.
//!
//! SIEVE is a cache eviction algorithm from the NSDI 2024 paper that achieves
//! ~30% better hit rate than LRU with O(1) operations and simpler implementation.
//!
//! # Algorithm
//!
//! - On access (hit): set the entry's `visited` bit to `true`.
//! - On insert (miss + full): scan from the `hand` position:
//!   - If `visited == true` → reset to `false`, advance hand.
//!   - If `visited == false` → evict this entry, insert the new one here.
//!
//! The cache uses a `Vec` as a circular buffer with a `HashMap` for O(1) lookups.

use std::collections::HashMap;
use std::hash::Hash;

/// A bounded cache using the SIEVE eviction algorithm.
///
/// `K` must be `Eq + Hash + Clone`, `V` must be `Clone`.
#[derive(Debug)]
pub(crate) struct SieveCache<K, V> {
    /// Circular buffer of cache entries.
    entries: Vec<Option<Entry<K, V>>>,
    /// Maps keys to their index in `entries`.
    index: HashMap<K, usize>,
    /// Current hand position for the SIEVE scan.
    hand: usize,
    /// Maximum number of entries.
    capacity: usize,
    /// Current number of live entries.
    len: usize,
}

#[derive(Debug, Clone)]
struct Entry<K, V> {
    key: K,
    value: V,
    visited: bool,
}

impl<K, V> SieveCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    /// Creates a new SIEVE cache with the given capacity.
    ///
    /// # Panics
    ///
    /// Panics if `capacity` is 0.
    pub(crate) fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "SIEVE cache capacity must be > 0");

        let entries = (0..capacity).map(|_| None).collect();

        Self {
            entries,
            index: HashMap::with_capacity(capacity),
            hand: 0,
            capacity,
            len: 0,
        }
    }

    /// Returns the value for `key`, marking it as recently used.
    pub(crate) fn get(&mut self, key: &K) -> Option<&V> {
        let &idx = self.index.get(key)?;
        if let Some(entry) = &mut self.entries[idx] {
            entry.visited = true;
            Some(&entry.value)
        } else {
            None
        }
    }

    /// Inserts a key-value pair, evicting if at capacity.
    pub(crate) fn insert(&mut self, key: K, value: V) {
        // If key already exists, update in place
        if let Some(&idx) = self.index.get(&key) {
            if let Some(entry) = &mut self.entries[idx] {
                entry.value = value;
                entry.visited = true;
                return;
            }
        }

        // If not at capacity, find an empty slot
        if self.len < self.capacity {
            for i in 0..self.capacity {
                if self.entries[i].is_none() {
                    self.entries[i] = Some(Entry {
                        key: key.clone(),
                        value,
                        visited: false,
                    });
                    self.index.insert(key, i);
                    self.len += 1;
                    return;
                }
            }
        }

        // At capacity: SIEVE eviction scan
        let evict_idx = self.find_eviction_target();

        // Remove old entry from index
        if let Some(old_entry) = &self.entries[evict_idx] {
            self.index.remove(&old_entry.key);
        }

        // Insert new entry
        self.entries[evict_idx] = Some(Entry {
            key: key.clone(),
            value,
            visited: false,
        });
        self.index.insert(key, evict_idx);
    }

    /// Removes a key from the cache.
    #[allow(dead_code)]
    pub(crate) fn remove(&mut self, key: &K) -> Option<V> {
        let idx = self.index.remove(key)?;
        let entry = self.entries[idx].take()?;
        self.len -= 1;
        Some(entry.value)
    }

    /// Returns the number of entries in the cache.
    #[allow(dead_code)]
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// Scans from `hand` to find an entry with `visited == false`.
    /// Resets `visited` bits along the way.
    fn find_eviction_target(&mut self) -> usize {
        // Bounded loop: at most 2 full scans (first pass resets visited bits,
        // second pass finds a target). In practice, converges much faster.
        let max_iterations = self.capacity * 2;

        for _ in 0..max_iterations {
            if let Some(entry) = &mut self.entries[self.hand] {
                if entry.visited {
                    entry.visited = false;
                    self.hand = (self.hand + 1) % self.capacity;
                } else {
                    let target = self.hand;
                    self.hand = (self.hand + 1) % self.capacity;
                    return target;
                }
            } else {
                // Empty slot — shouldn't happen at capacity, but handle gracefully
                let target = self.hand;
                self.hand = (self.hand + 1) % self.capacity;
                return target;
            }
        }

        // Fallback: evict at current hand position
        let target = self.hand;
        self.hand = (self.hand + 1) % self.capacity;
        target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_insert_and_get() {
        let mut cache = SieveCache::new(3);
        cache.insert("a", 1);
        cache.insert("b", 2);
        cache.insert("c", 3);

        assert_eq!(cache.get(&"a"), Some(&1));
        assert_eq!(cache.get(&"b"), Some(&2));
        assert_eq!(cache.get(&"c"), Some(&3));
        assert_eq!(cache.len(), 3);
    }

    #[test]
    fn eviction_prefers_unvisited() {
        let mut cache = SieveCache::new(3);
        cache.insert("a", 1);
        cache.insert("b", 2);
        cache.insert("c", 3);

        // Access a and c (mark as visited), leave b unvisited
        cache.get(&"a");
        cache.get(&"c");

        // Insert d — should evict b (unvisited)
        cache.insert("d", 4);

        assert_eq!(cache.get(&"a"), Some(&1));
        assert_eq!(cache.get(&"b"), None); // evicted
        assert_eq!(cache.get(&"c"), Some(&3));
        assert_eq!(cache.get(&"d"), Some(&4));
    }

    #[test]
    fn update_existing_key() {
        let mut cache = SieveCache::new(2);
        cache.insert("a", 1);
        cache.insert("a", 10);

        assert_eq!(cache.get(&"a"), Some(&10));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn remove_entry() {
        let mut cache = SieveCache::new(3);
        cache.insert("a", 1);
        cache.insert("b", 2);

        assert_eq!(cache.remove(&"a"), Some(1));
        assert_eq!(cache.get(&"a"), None);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn capacity_one() {
        let mut cache = SieveCache::new(1);
        cache.insert("a", 1);
        assert_eq!(cache.get(&"a"), Some(&1));

        cache.insert("b", 2);
        assert_eq!(cache.get(&"a"), None);
        assert_eq!(cache.get(&"b"), Some(&2));
    }

    #[test]
    #[should_panic(expected = "capacity must be > 0")]
    fn zero_capacity_panics() {
        let _cache: SieveCache<&str, i32> = SieveCache::new(0);
    }

    #[test]
    fn eviction_wraps_around() {
        let mut cache = SieveCache::new(4);
        cache.insert("a", 1);
        cache.insert("b", 2);
        cache.insert("c", 3);
        cache.insert("d", 4);

        // Visit all entries
        cache.get(&"a");
        cache.get(&"b");
        cache.get(&"c");
        cache.get(&"d");

        // Insert e — all visited, so SIEVE resets bits and evicts
        cache.insert("e", 5);

        // One entry should have been evicted
        assert_eq!(cache.len(), 4);
        assert_eq!(cache.get(&"e"), Some(&5));
    }
}
