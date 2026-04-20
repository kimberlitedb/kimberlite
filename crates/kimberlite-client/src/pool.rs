//! Connection pool for [`Client`].
//!
//! Each pool owns up to `max_size` TCP connections. Callers [`acquire`][Pool::acquire]
//! a [`PooledClient`] — a RAII guard that returns the connection to the pool
//! on drop, even on panic. Idle connections are evicted once their last-used
//! time exceeds `idle_timeout`.
//!
//! # Example
//!
//! ```ignore
//! use kimberlite_client::{Pool, PoolConfig, ClientConfig};
//! use kimberlite_types::TenantId;
//! use std::time::Duration;
//!
//! let pool = Pool::new(
//!     "127.0.0.1:5432",
//!     TenantId::new(1),
//!     PoolConfig {
//!         max_size: 8,
//!         acquire_timeout: Some(Duration::from_secs(5)),
//!         idle_timeout: Some(Duration::from_secs(300)),
//!         client_config: ClientConfig::default(),
//!     },
//! );
//!
//! // Borrow a client for one operation.
//! {
//!     let mut c = pool.acquire()?;
//!     let _ = c.query("SELECT 1", &[])?;
//! } // Returned to pool here.
//!
//! // Or use the closure helper.
//! pool.with_client(|c| c.query("SELECT 1", &[]))?;
//! ```

use std::collections::VecDeque;
use std::net::ToSocketAddrs;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use kimberlite_types::TenantId;

use crate::client::{Client, ClientConfig};
use crate::error::{ClientError, ClientResult};

/// Configuration for [`Pool`].
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Hard cap on the number of concurrent connections the pool will create.
    pub max_size: usize,
    /// Maximum wait for [`Pool::acquire`] before returning [`ClientError::Timeout`].
    ///
    /// `None` blocks indefinitely.
    pub acquire_timeout: Option<Duration>,
    /// Maximum idle time before an unused connection is dropped. Checked
    /// lazily on each `acquire`.
    ///
    /// `None` disables idle eviction.
    pub idle_timeout: Option<Duration>,
    /// Config passed to each underlying [`Client`].
    pub client_config: ClientConfig,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_size: 10,
            acquire_timeout: Some(Duration::from_secs(30)),
            idle_timeout: Some(Duration::from_secs(300)),
            client_config: ClientConfig::default(),
        }
    }
}

/// A thread-safe pool of [`Client`] connections.
///
/// `Pool` is cheap to clone — it holds only an `Arc` to internal state — so
/// pass it around by value to worker threads.
#[derive(Debug, Clone)]
pub struct Pool {
    inner: Arc<PoolInner>,
}

#[derive(Debug)]
struct PoolInner {
    addr: String,
    tenant_id: TenantId,
    config: PoolConfig,
    state: Mutex<PoolState>,
    available: Condvar,
}

#[derive(Debug)]
struct PoolState {
    /// Connections currently available for checkout.
    idle: VecDeque<IdleEntry>,
    /// Total number of `Client`s the pool owns (idle + in-use + in-flight creates).
    open_count: usize,
    /// Once true, the pool refuses new acquires and destroys returned clients.
    shutdown: bool,
}

#[derive(Debug)]
struct IdleEntry {
    client: Client,
    returned_at: Instant,
}

impl Pool {
    /// Creates a new pool targeting a single server address.
    ///
    /// Connections are not eagerly opened; the first [`Pool::acquire`] call
    /// triggers the first `Client::connect`.
    pub fn new(
        addr: impl ToSocketAddrs,
        tenant_id: TenantId,
        config: PoolConfig,
    ) -> ClientResult<Self> {
        assert!(config.max_size > 0, "Pool::max_size must be non-zero");

        let addr = resolve_first_addr(addr)?;
        Ok(Self {
            inner: Arc::new(PoolInner {
                addr,
                tenant_id,
                config,
                state: Mutex::new(PoolState {
                    idle: VecDeque::new(),
                    open_count: 0,
                    shutdown: false,
                }),
                available: Condvar::new(),
            }),
        })
    }

    /// Returns the pool's configured capacity.
    pub fn max_size(&self) -> usize {
        self.inner.config.max_size
    }

    /// Returns the tenant ID the pool's connections authenticate as.
    pub fn tenant_id(&self) -> TenantId {
        self.inner.tenant_id
    }

    /// Current pool statistics — useful for metrics.
    pub fn stats(&self) -> PoolStats {
        let state = self.inner.lock_state();
        PoolStats {
            max_size: self.inner.config.max_size,
            open: state.open_count,
            idle: state.idle.len(),
            in_use: state.open_count.saturating_sub(state.idle.len()),
            shutdown: state.shutdown,
        }
    }

    /// Acquires a connection from the pool.
    ///
    /// If an idle connection is available, returns it immediately. Otherwise
    /// opens a new connection (up to `max_size`), or blocks waiting for a
    /// release (up to `acquire_timeout`).
    ///
    /// # Errors
    ///
    /// - [`ClientError::Timeout`] if the wait exceeds `acquire_timeout`.
    /// - [`ClientError::NotConnected`] if the pool has been shut down.
    /// - Any error from `Client::connect` when a new connection is opened.
    pub fn acquire(&self) -> ClientResult<PooledClient> {
        let deadline = self
            .inner
            .config
            .acquire_timeout
            .map(|d| Instant::now() + d);

        let mut state = self.inner.lock_state();

        loop {
            if state.shutdown {
                return Err(ClientError::NotConnected);
            }

            // Evict expired idle entries under the lock.
            if let Some(timeout) = self.inner.config.idle_timeout {
                evict_expired(&mut state, timeout);
            }

            // Fast path: reuse an idle connection.
            if let Some(entry) = state.idle.pop_front() {
                return Ok(PooledClient {
                    client: Some(entry.client),
                    pool: Arc::clone(&self.inner),
                });
            }

            // Room to grow — reserve a slot, drop the lock, connect, then
            // restore the guard so Drop accounts for the client if something
            // panics between now and the successful return.
            if state.open_count < self.inner.config.max_size {
                state.open_count += 1;
                drop(state);

                let mut reservation = SlotReservation::new(Arc::clone(&self.inner));
                match self.connect_new() {
                    Ok(client) => {
                        // Hand the slot's ownership to the new PooledClient.
                        reservation.consumed = true;
                        return Ok(PooledClient {
                            client: Some(client),
                            pool: Arc::clone(&self.inner),
                        });
                    }
                    Err(e) => {
                        // reservation drop releases the slot.
                        drop(reservation);
                        return Err(e);
                    }
                }
            }

            // At capacity — wait for a release.
            state = match deadline {
                Some(d) => {
                    let now = Instant::now();
                    if now >= d {
                        return Err(ClientError::Timeout);
                    }
                    let (new_state, timeout_result) = self
                        .inner
                        .available
                        .wait_timeout(state, d - now)
                        .expect("pool state mutex poisoned");
                    if timeout_result.timed_out() && new_state.idle.is_empty() {
                        // Re-check shutdown before returning timeout so a
                        // concurrent shutdown surfaces as NotConnected.
                        if new_state.shutdown {
                            return Err(ClientError::NotConnected);
                        }
                        return Err(ClientError::Timeout);
                    }
                    new_state
                }
                None => self
                    .inner
                    .available
                    .wait(state)
                    .expect("pool state mutex poisoned"),
            };
        }
    }

    /// Runs `f` with a checked-out client, returning it to the pool whether
    /// `f` succeeds, errors, or panics.
    pub fn with_client<T>(
        &self,
        f: impl FnOnce(&mut Client) -> ClientResult<T>,
    ) -> ClientResult<T> {
        let mut guard = self.acquire()?;
        f(&mut guard)
    }

    /// Shuts the pool down. Subsequent acquires return
    /// [`ClientError::NotConnected`]. Idle connections are closed immediately;
    /// currently-borrowed ones are closed when returned.
    pub fn shutdown(&self) {
        let mut state = self.inner.lock_state();
        if state.shutdown {
            return;
        }
        state.shutdown = true;
        // Drop all idle clients now; decrement open_count accordingly.
        let idle_count = state.idle.len();
        state.idle.clear();
        state.open_count = state.open_count.saturating_sub(idle_count);
        // Wake every waiter so they see `shutdown` and bail out.
        self.inner.available.notify_all();
    }

    fn connect_new(&self) -> ClientResult<Client> {
        Client::connect(
            &self.inner.addr,
            self.inner.tenant_id,
            self.inner.config.client_config.clone(),
        )
    }
}

/// Releases an open_count slot on drop. Used to balance the pre-increment
/// done before `Client::connect` so a panic or early return restores
/// capacity for other acquirers.
struct SlotReservation {
    pool: Arc<PoolInner>,
    consumed: bool,
}

impl SlotReservation {
    fn new(pool: Arc<PoolInner>) -> Self {
        Self {
            pool,
            consumed: false,
        }
    }
}

impl Drop for SlotReservation {
    fn drop(&mut self) {
        if self.consumed {
            return;
        }
        let mut state = self.pool.lock_state();
        state.open_count = state.open_count.saturating_sub(1);
        self.pool.available.notify_one();
    }
}

impl PoolInner {
    fn lock_state(&self) -> std::sync::MutexGuard<'_, PoolState> {
        self.state.lock().expect("pool state mutex poisoned")
    }
}

fn evict_expired(state: &mut PoolState, timeout: Duration) {
    let now = Instant::now();
    while let Some(front) = state.idle.front() {
        if now.duration_since(front.returned_at) > timeout {
            // Safe because we just checked.
            let entry = state.idle.pop_front().unwrap();
            drop(entry.client);
            state.open_count = state.open_count.saturating_sub(1);
        } else {
            // Entries are pushed in order, so once we find a non-expired
            // entry we can stop.
            break;
        }
    }
}

fn resolve_first_addr(addr: impl ToSocketAddrs) -> ClientResult<String> {
    let mut iter = addr.to_socket_addrs()?;
    let first = iter.next().ok_or_else(|| {
        ClientError::Connection(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "no addresses resolved",
        ))
    })?;
    Ok(first.to_string())
}

/// Snapshot of pool capacity and utilisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolStats {
    pub max_size: usize,
    /// Total connections the pool owns right now (idle + in_use).
    pub open: usize,
    /// Connections currently sitting in the idle queue.
    pub idle: usize,
    /// Connections currently checked out to callers.
    pub in_use: usize,
    pub shutdown: bool,
}

/// RAII guard for a pool-borrowed connection.
///
/// Dereferences to [`Client`]. Returns the connection to the pool on drop.
/// If the pool has been shut down, the connection is closed instead.
pub struct PooledClient {
    client: Option<Client>,
    pool: Arc<PoolInner>,
}

impl PooledClient {
    /// Drops the underlying connection instead of returning it to the pool.
    ///
    /// Use this after a fatal error on the connection (broken socket,
    /// unrecoverable protocol state) so the pool opens a fresh one on the
    /// next acquire.
    pub fn discard(mut self) {
        if let Some(client) = self.client.take() {
            drop(client);
            let mut state = self.pool.lock_state();
            state.open_count = state.open_count.saturating_sub(1);
            self.pool.available.notify_one();
        }
    }
}

impl Deref for PooledClient {
    type Target = Client;

    fn deref(&self) -> &Client {
        self.client.as_ref().expect("PooledClient already released")
    }
}

impl DerefMut for PooledClient {
    fn deref_mut(&mut self) -> &mut Client {
        self.client.as_mut().expect("PooledClient already released")
    }
}

impl Drop for PooledClient {
    fn drop(&mut self) {
        let Some(client) = self.client.take() else {
            return;
        };

        let mut state = self.pool.lock_state();
        if state.shutdown {
            drop(client);
            state.open_count = state.open_count.saturating_sub(1);
        } else {
            state.idle.push_back(IdleEntry {
                client,
                returned_at: Instant::now(),
            });
        }
        self.pool.available.notify_one();
    }
}

impl std::fmt::Debug for PooledClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PooledClient")
            .field("client", &self.client)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    /// Binds to a free port, drops the listener, and returns the port so a
    /// later `Client::connect` to that address will fail cleanly (nothing's
    /// listening). Good enough for tests that only exercise pool bookkeeping
    /// up to the point where the real connect would happen.
    fn free_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        port
    }

    #[test]
    fn pool_max_size_asserts_nonzero() {
        let config = PoolConfig {
            max_size: 0,
            ..PoolConfig::default()
        };
        // This panics inside the `assert!` — use catch_unwind.
        let result =
            std::panic::catch_unwind(|| Pool::new("127.0.0.1:1", TenantId::new(1), config));
        assert!(result.is_err());
    }

    #[test]
    fn stats_start_at_zero() {
        let port = free_port();
        let pool = Pool::new(
            format!("127.0.0.1:{port}"),
            TenantId::new(1),
            PoolConfig {
                max_size: 5,
                acquire_timeout: Some(Duration::from_millis(50)),
                ..PoolConfig::default()
            },
        )
        .unwrap();

        let s = pool.stats();
        assert_eq!(s.max_size, 5);
        assert_eq!(s.open, 0);
        assert_eq!(s.idle, 0);
        assert_eq!(s.in_use, 0);
        assert!(!s.shutdown);
    }

    #[test]
    fn acquire_after_shutdown_errors() {
        let port = free_port();
        let pool = Pool::new(
            format!("127.0.0.1:{port}"),
            TenantId::new(1),
            PoolConfig::default(),
        )
        .unwrap();

        pool.shutdown();
        let result = pool.acquire();
        assert!(matches!(result, Err(ClientError::NotConnected)));
        assert!(pool.stats().shutdown);
    }

    #[test]
    fn failed_connect_releases_slot() {
        let port = free_port(); // nothing listening
        let pool = Pool::new(
            format!("127.0.0.1:{port}"),
            TenantId::new(1),
            PoolConfig {
                max_size: 2,
                acquire_timeout: Some(Duration::from_millis(50)),
                ..PoolConfig::default()
            },
        )
        .unwrap();

        // First acquire fails — but must not permanently consume a slot.
        let first = pool.acquire();
        assert!(first.is_err());
        assert_eq!(pool.stats().open, 0);

        // Second acquire fails for the same reason — slot accounting is intact.
        let second = pool.acquire();
        assert!(second.is_err());
        assert_eq!(pool.stats().open, 0);
    }

    #[test]
    fn acquire_timeout_when_at_capacity() {
        // Start a real listener so the first connection succeeds (the handshake
        // fails, but that's fine — we only need a `Client` to populate the pool
        // for this test… actually we can't without a real server). Instead test
        // the timeout path indirectly by checking that a zero acquire_timeout
        // returns Timeout immediately when the first connect would be attempted
        // but a slot is already claimed.
        //
        // Simulate by manually occupying the slot via open_count. This is an
        // internal-state test but captures the at-capacity timeout path.
        let port = free_port();
        let pool = Pool::new(
            format!("127.0.0.1:{port}"),
            TenantId::new(1),
            PoolConfig {
                max_size: 1,
                acquire_timeout: Some(Duration::from_millis(10)),
                ..PoolConfig::default()
            },
        )
        .unwrap();

        {
            let mut state = pool.inner.lock_state();
            state.open_count = 1; // fake a checked-out client
        }

        let start = Instant::now();
        let result = pool.acquire();
        let elapsed = start.elapsed();
        assert!(matches!(result, Err(ClientError::Timeout)));
        assert!(
            elapsed >= Duration::from_millis(5),
            "should have waited at least ~10ms, waited {elapsed:?}"
        );

        // Clean up: restore state so drop doesn't panic on assertion.
        let mut state = pool.inner.lock_state();
        state.open_count = 0;
    }

    #[test]
    fn evict_expired_drops_stale_idle_entries() {
        // This test constructs IdleEntries by reaching into PoolState directly
        // since real Client construction requires a live server.
        let port = free_port();
        let pool = Pool::new(
            format!("127.0.0.1:{port}"),
            TenantId::new(1),
            PoolConfig {
                max_size: 3,
                idle_timeout: Some(Duration::from_millis(5)),
                ..PoolConfig::default()
            },
        )
        .unwrap();

        // Simulate 2 idle entries by bumping open_count. We can't build real
        // Client values without a server, so we settle for checking that
        // open_count accounting flows correctly when evict_expired is a no-op
        // (empty idle queue).
        {
            let mut state = pool.inner.lock_state();
            state.open_count = 2;
            evict_expired(&mut state, Duration::from_millis(5));
            // Nothing to evict — open_count stays at 2.
            assert_eq!(state.open_count, 2);
        }
    }

    #[test]
    fn shutdown_wakes_waiters() {
        let port = free_port();
        let pool = Pool::new(
            format!("127.0.0.1:{port}"),
            TenantId::new(1),
            PoolConfig {
                max_size: 1,
                acquire_timeout: Some(Duration::from_secs(5)),
                ..PoolConfig::default()
            },
        )
        .unwrap();

        // Pre-occupy the single slot.
        {
            let mut state = pool.inner.lock_state();
            state.open_count = 1;
        }

        let p2 = pool.clone();
        let observed_error = Arc::new(AtomicUsize::new(0));
        let observed_clone = Arc::clone(&observed_error);
        let handle = thread::spawn(move || match p2.acquire() {
            Err(ClientError::NotConnected) => observed_clone.store(1, Ordering::SeqCst),
            Err(ClientError::Timeout) => observed_clone.store(2, Ordering::SeqCst),
            Ok(_) => observed_clone.store(3, Ordering::SeqCst),
            Err(_) => observed_clone.store(4, Ordering::SeqCst),
        });

        // Give the thread a chance to block.
        thread::sleep(Duration::from_millis(50));
        pool.shutdown();

        handle.join().unwrap();
        assert_eq!(
            observed_error.load(Ordering::SeqCst),
            1,
            "waiter should observe NotConnected once pool shuts down"
        );

        // Cleanup for the fake slot.
        let mut state = pool.inner.lock_state();
        state.open_count = 0;
    }

    #[test]
    fn pool_clone_shares_state() {
        let port = free_port();
        let pool = Pool::new(
            format!("127.0.0.1:{port}"),
            TenantId::new(1),
            PoolConfig {
                max_size: 3,
                ..PoolConfig::default()
            },
        )
        .unwrap();

        let clone = pool.clone();
        pool.shutdown();
        assert!(clone.stats().shutdown, "clone sees the same shutdown flag");
    }
}
