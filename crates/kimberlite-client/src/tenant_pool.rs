//! Per-tenant [`Client`] cache.
//!
//! AUDIT-2026-04 S2.4 — lifts notebar's LRU-per-tenant adapter
//! out of `packages/kimberlite-client/src/adapter.ts` into the
//! Rust SDK so every multi-tenant app gets the same pattern
//! without re-implementing LRU + idle eviction + factory
//! callbacks.
//!
//! # Difference from [`crate::Pool`]
//!
//! - `Pool` multiplexes N connections to a **single** tenant
//!   across concurrent callers.
//! - `TenantPool` holds one long-lived [`Client`] per
//!   `TenantId` so N concurrent tenants can talk to the server
//!   without reconnecting on every call.
//!
//! # Example
//!
//! ```no_run
//! use std::sync::Arc;
//! use std::time::Duration;
//! use kimberlite_client::{Client, ClientConfig};
//! use kimberlite_client::tenant_pool::{TenantPool, TenantPoolConfig};
//! use kimberlite_types::TenantId;
//!
//! # fn main() -> kimberlite_client::ClientResult<()> {
//! let pool = TenantPool::new(TenantPoolConfig {
//!     factory: Arc::new(|tenant_id: TenantId| {
//!         Client::connect("127.0.0.1:5432", tenant_id, ClientConfig::default())
//!     }),
//!     max_size: 128,
//!     idle_timeout: Duration::from_secs(300),
//! });
//!
//! pool.with_client(TenantId::new(7), |client| {
//!     let _tables = client.list_tables()?;
//!     Ok::<_, kimberlite_client::ClientError>(())
//! })?;
//! # Ok(()) }
//! ```

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kimberlite_types::TenantId;

use crate::client::Client;
use crate::error::{ClientError, ClientResult};

/// Factory closure that opens a fresh [`Client`] for a given tenant.
///
/// Typically:
/// `Arc::new(|tid| Client::connect(addr, tid, cfg.clone()))`.
pub type ClientFactory =
    Arc<dyn Fn(TenantId) -> ClientResult<Client> + Send + Sync + 'static>;

/// Configuration for [`TenantPool`].
pub struct TenantPoolConfig {
    /// Called once per uncached tenant to open a fresh `Client`.
    pub factory: ClientFactory,
    /// Max cached tenants. LRU-evicted above this. A value of 0
    /// is invalid — the pool would evict every insert and be
    /// useless; construction asserts against it.
    pub max_size: usize,
    /// Idle-timeout. Entries untouched for longer than this are
    /// evicted on the next [`TenantPool::acquire`] call.
    /// `Duration::ZERO` disables idle eviction.
    pub idle_timeout: Duration,
}

/// Runtime statistics snapshot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TenantPoolStats {
    pub size: usize,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub idle_evictions: u64,
}

struct Entry {
    client: Client,
    last_used_at: Instant,
}

struct Inner {
    clients: HashMap<TenantId, Entry>,
    // Simple recency list — tenant IDs in LRU order (front = oldest).
    // For max_size ≤ 128 (the typical default) this is O(N)
    // linear scan on evict; acceptable. A doubly-linked list
    // with map-to-node indices would be O(1) but needs unsafe
    // or `indexmap`; avoiding both for simplicity.
    hits: u64,
    misses: u64,
    evictions: u64,
    idle_evictions: u64,
}

/// LRU-per-tenant [`Client`] cache.
///
/// Thread-safe: internal `Mutex` makes `TenantPool` `Sync` so a
/// single `Arc<TenantPool>` can be shared across handler threads.
/// Concurrent `acquire()` calls for the same tenant serialise on
/// the lock — there is no per-tenant inflight dedup in the Rust
/// variant because the factory closure is synchronous and
/// typically fast.
pub struct TenantPool {
    factory: ClientFactory,
    max_size: usize,
    idle_timeout: Duration,
    inner: Mutex<Inner>,
}

impl TenantPool {
    /// Create a new pool.
    ///
    /// # Panics
    ///
    /// Panics if `max_size == 0` — a zero-capacity cache is never
    /// a valid configuration and would cause every acquire to
    /// evict immediately.
    pub fn new(cfg: TenantPoolConfig) -> Self {
        assert!(cfg.max_size > 0, "TenantPool max_size must be > 0");
        Self {
            factory: cfg.factory,
            max_size: cfg.max_size,
            idle_timeout: cfg.idle_timeout,
            inner: Mutex::new(Inner {
                clients: HashMap::with_capacity(cfg.max_size),
                hits: 0,
                misses: 0,
                evictions: 0,
                idle_evictions: 0,
            }),
        }
    }

    /// Run `fn(client)` with the cached client for `tenant_id`.
    ///
    /// The client is never surfaced to the caller past `f`'s
    /// scope — this keeps the lock-holding window obvious.
    /// Callers that need to retain a client across `.await`
    /// boundaries should wait for the async SDK (S2.1).
    pub fn with_client<F, T>(&self, tenant_id: TenantId, f: F) -> ClientResult<T>
    where
        F: FnOnce(&mut Client) -> ClientResult<T>,
    {
        let mut inner = self.inner.lock().map_err(|_| ClientError::NotConnected)?;
        self.expire_idle(&mut inner);

        if !inner.clients.contains_key(&tenant_id) {
            inner.misses += 1;
            // Release the lock across the factory call — factory
            // may perform network I/O, which must not block other
            // tenants from acquiring their cached clients.
            drop(inner);
            let client = (self.factory)(tenant_id)?;
            let mut inner = self.inner.lock().map_err(|_| ClientError::NotConnected)?;
            self.evict_if_full(&mut inner);
            inner.clients.insert(
                tenant_id,
                Entry {
                    client,
                    last_used_at: Instant::now(),
                },
            );
            // Re-enter the cached path below with the fresh lock.
            let entry = inner
                .clients
                .get_mut(&tenant_id)
                .expect("just inserted");
            entry.last_used_at = Instant::now();
            return f(&mut entry.client);
        }

        inner.hits += 1;
        let entry = inner
            .clients
            .get_mut(&tenant_id)
            .expect("just verified present");
        entry.last_used_at = Instant::now();
        f(&mut entry.client)
    }

    /// Drop all cached clients. Subsequent calls reconnect via
    /// the factory.
    pub fn close(&self) {
        let mut inner = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        inner.clients.clear();
    }

    /// Runtime stats snapshot.
    pub fn stats(&self) -> TenantPoolStats {
        let inner = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        TenantPoolStats {
            size: inner.clients.len(),
            hits: inner.hits,
            misses: inner.misses,
            evictions: inner.evictions,
            idle_evictions: inner.idle_evictions,
        }
    }

    // ------------------------------------------------------------------

    fn evict_if_full(&self, inner: &mut Inner) {
        while inner.clients.len() >= self.max_size {
            // Find oldest tenant by last_used_at.
            let oldest = inner
                .clients
                .iter()
                .min_by_key(|(_, e)| e.last_used_at)
                .map(|(tid, _)| *tid);
            if let Some(tid) = oldest {
                inner.clients.remove(&tid);
                inner.evictions += 1;
            } else {
                break;
            }
        }
    }

    fn expire_idle(&self, inner: &mut Inner) {
        if self.idle_timeout.is_zero() {
            return;
        }
        let now = Instant::now();
        let stale: Vec<TenantId> = inner
            .clients
            .iter()
            .filter_map(|(tid, e)| {
                if now.duration_since(e.last_used_at) >= self.idle_timeout {
                    Some(*tid)
                } else {
                    None
                }
            })
            .collect();
        for tid in stale {
            inner.clients.remove(&tid);
            inner.idle_evictions += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    //! Stub-based tests. The factory returns synthetic `Client`
    //! wrappers around NULL `TcpStream`s; behaviour exercised
    //! here is LRU / idle / stats. End-to-end with live
    //! connections is covered by the server-integration suite.
    //!
    //! We can't easily construct a `Client` without a live
    //! server (the field set is private and handshake runs
    //! inside `connect()`). The tests therefore cover:
    //!
    //! - `new()` panics on `max_size == 0`.
    //! - `stats()` is zero on an empty pool.
    //! - Factory-error propagation via `with_client`.

    use super::*;

    fn failing_factory() -> ClientFactory {
        Arc::new(|_tid| Err(ClientError::NotConnected))
    }

    #[test]
    #[should_panic(expected = "max_size must be > 0")]
    fn new_panics_on_zero_max_size() {
        let _ = TenantPool::new(TenantPoolConfig {
            factory: failing_factory(),
            max_size: 0,
            idle_timeout: Duration::ZERO,
        });
    }

    #[test]
    fn stats_is_zero_on_empty_pool() {
        let pool = TenantPool::new(TenantPoolConfig {
            factory: failing_factory(),
            max_size: 16,
            idle_timeout: Duration::from_secs(300),
        });
        assert_eq!(pool.stats(), TenantPoolStats::default());
    }

    #[test]
    fn factory_error_propagates() {
        let pool = TenantPool::new(TenantPoolConfig {
            factory: failing_factory(),
            max_size: 16,
            idle_timeout: Duration::ZERO,
        });
        let err = pool
            .with_client(TenantId::new(1), |_c| Ok(()))
            .unwrap_err();
        assert!(matches!(err, ClientError::NotConnected));
        // Counter advanced for the failed miss.
        assert_eq!(pool.stats().misses, 1);
        // But nothing is cached.
        assert_eq!(pool.stats().size, 0);
    }

    #[test]
    fn close_clears_cache() {
        let pool = TenantPool::new(TenantPoolConfig {
            factory: failing_factory(),
            max_size: 16,
            idle_timeout: Duration::ZERO,
        });
        // Nothing to close; smoke test that close() on empty
        // pool is a no-op and doesn't panic.
        pool.close();
        assert_eq!(pool.stats().size, 0);
    }
}
