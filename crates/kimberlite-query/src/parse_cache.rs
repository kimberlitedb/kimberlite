//! Parse cache for the query engine.
//!
//! AUDIT-2026-04 S3.4 — a simple LRU cache keyed on raw SQL that
//! memoises parsed `ParsedStatement`s. Every call to
//! [`crate::QueryEngine::query`] re-parses the SQL otherwise; for
//! high-QPS compliance-reporting workloads that reuse a handful
//! of SQL strings, skipping sqlparser + custom-syntax extraction
//! shaves real latency.
//!
//! # What is NOT cached here
//!
//! The downstream `planner::plan_query` step consumes the live
//! `Schema` reference, so caching the planned `QueryPlan` would
//! require a schema-version counter that invalidates on every
//! DDL. That's a bigger change — tracked as a follow-up. This
//! parse cache is a safe zero-risk optimisation because:
//!
//! - `ParsedStatement` depends only on the SQL bytes.
//! - Queries carrying non-deterministic functions (`NOW()`,
//!   `CURRENT_TIMESTAMP`, `random()`) are still safe to cache at
//!   the parse level — only the eventual *planning* would need
//!   bypass (not reached here).
//!
//! # Thread safety
//!
//! The cache uses a `Mutex<LruCacheLike>` so a single
//! `QueryEngine` instance can be shared across threads. Cache
//! misses serialise briefly on the parse call; cache hits hold
//! the lock only for the LRU recency update.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::parser::ParsedStatement;

/// Bounded LRU parse cache.
///
/// Internal only — exposed via `QueryEngine::with_parse_cache`.
pub(crate) struct ParseCache {
    inner: Mutex<ParseCacheInner>,
    max_size: usize,
}

impl std::fmt::Debug for ParseCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Show only the cap + observable stats; the
        // ParsedStatement values themselves would bloat logs.
        let s = self.stats();
        f.debug_struct("ParseCache")
            .field("max_size", &self.max_size)
            .field("size", &s.size)
            .field("hits", &s.hits)
            .field("misses", &s.misses)
            .field("evictions", &s.evictions)
            .finish_non_exhaustive()
    }
}

struct ParseCacheInner {
    /// `sql → (insertion_counter, ParsedStatement)`. The counter
    /// gives us a total ordering for LRU eviction without a
    /// linked list; the map is bounded by `max_size`.
    entries: HashMap<String, (u64, ParsedStatement)>,
    /// Monotonic counter bumped on every `insert()` and hit.
    /// Eviction picks the entry with the smallest counter.
    tick: u64,
    hits: u64,
    misses: u64,
    evictions: u64,
}

impl ParseCache {
    /// Create a new cache with the given LRU bound. `max_size`
    /// of 0 is valid and disables caching (every call misses).
    pub(crate) fn new(max_size: usize) -> Self {
        Self {
            max_size,
            inner: Mutex::new(ParseCacheInner {
                entries: HashMap::with_capacity(max_size.min(1024)),
                tick: 0,
                hits: 0,
                misses: 0,
                evictions: 0,
            }),
        }
    }

    /// Look up `sql` in the cache. Returns the cached
    /// `ParsedStatement` via clone on hit, or `None` on miss.
    ///
    /// Updates LRU recency on hit.
    pub(crate) fn get(&self, sql: &str) -> Option<ParsedStatement> {
        let mut guard = self.inner.lock().ok()?;
        // Bump `tick` first to avoid the split-borrow between
        // reading from `entries` and mutating `tick`.
        let next = guard.tick.wrapping_add(1);
        guard.tick = next;
        if let Some((counter, stmt)) = guard.entries.get_mut(sql) {
            let cloned = stmt.clone();
            *counter = next;
            guard.hits += 1;
            Some(cloned)
        } else {
            guard.misses += 1;
            None
        }
    }

    /// Insert a parsed statement under `sql`. Evicts the LRU
    /// entry when at capacity.
    pub(crate) fn insert(&self, sql: String, stmt: ParsedStatement) {
        if self.max_size == 0 {
            return;
        }
        let Ok(mut guard) = self.inner.lock() else {
            return;
        };
        if guard.entries.len() >= self.max_size && !guard.entries.contains_key(&sql) {
            // Evict LRU — lowest counter.
            if let Some(oldest_key) = guard
                .entries
                .iter()
                .min_by_key(|(_, (c, _))| *c)
                .map(|(k, _)| k.clone())
            {
                guard.entries.remove(&oldest_key);
                guard.evictions += 1;
            }
        }
        let next = guard.tick.wrapping_add(1);
        guard.tick = next;
        guard.entries.insert(sql, (next, stmt));
    }

    /// Parse-cache runtime stats.
    pub(crate) fn stats(&self) -> ParseCacheStats {
        match self.inner.lock() {
            Ok(g) => ParseCacheStats {
                size: g.entries.len(),
                hits: g.hits,
                misses: g.misses,
                evictions: g.evictions,
            },
            Err(_) => ParseCacheStats::default(),
        }
    }

    /// Clear all cached entries. Useful for tests and for
    /// schema-change scenarios where the caller wants to flush.
    pub(crate) fn clear(&self) {
        if let Ok(mut g) = self.inner.lock() {
            g.entries.clear();
        }
    }
}

/// Statistics snapshot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ParseCacheStats {
    pub size: usize,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_statement;

    fn parse(sql: &str) -> ParsedStatement {
        parse_statement(sql).unwrap()
    }

    // The custom parser requires a FROM clause; use valid SQL
    // throughout. Three distinct strings for LRU + eviction tests.
    const SQL1: &str = "SELECT id FROM users WHERE id = 1";
    const SQL2: &str = "SELECT id FROM users WHERE id = 2";
    const SQL3: &str = "SELECT id FROM users WHERE id = 3";

    #[test]
    fn get_miss_on_empty_cache() {
        let c = ParseCache::new(8);
        assert!(c.get(SQL1).is_none());
        assert_eq!(c.stats().misses, 1);
        assert_eq!(c.stats().hits, 0);
    }

    #[test]
    fn insert_then_get_hits() {
        let c = ParseCache::new(8);
        c.insert(SQL1.into(), parse(SQL1));
        assert!(c.get(SQL1).is_some());
        assert_eq!(c.stats().hits, 1);
        assert_eq!(c.stats().size, 1);
    }

    #[test]
    fn distinct_sql_does_not_collide() {
        let c = ParseCache::new(8);
        c.insert(SQL1.into(), parse(SQL1));
        c.insert(SQL2.into(), parse(SQL2));
        assert!(c.get(SQL1).is_some());
        assert!(c.get(SQL2).is_some());
        assert_eq!(c.stats().size, 2);
    }

    #[test]
    fn lru_evicts_least_recently_used() {
        let c = ParseCache::new(2);
        c.insert(SQL1.into(), parse(SQL1));
        c.insert(SQL2.into(), parse(SQL2));
        // Touch SQL1 → SQL2 is LRU.
        c.get(SQL1);
        // Insert SQL3 → evicts SQL2.
        c.insert(SQL3.into(), parse(SQL3));
        assert!(c.get(SQL1).is_some());
        assert!(c.get(SQL2).is_none());
        assert!(c.get(SQL3).is_some());
        assert_eq!(c.stats().evictions, 1);
    }

    #[test]
    fn zero_max_size_disables_cache() {
        let c = ParseCache::new(0);
        c.insert(SQL1.into(), parse(SQL1));
        // Never stored — every get misses.
        assert!(c.get(SQL1).is_none());
        assert_eq!(c.stats().size, 0);
    }

    #[test]
    fn clear_drops_all_entries() {
        let c = ParseCache::new(8);
        c.insert(SQL1.into(), parse(SQL1));
        c.insert(SQL2.into(), parse(SQL2));
        c.clear();
        assert_eq!(c.stats().size, 0);
        assert!(c.get(SQL1).is_none());
    }

    #[test]
    fn insert_updates_recency_on_existing_key() {
        // Re-inserting the same key shouldn't double-count.
        let c = ParseCache::new(8);
        c.insert(SQL1.into(), parse(SQL1));
        c.insert(SQL1.into(), parse(SQL1));
        assert_eq!(c.stats().size, 1);
    }
}
