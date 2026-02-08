//! Zero-copy buffer pool for recycling `BytesMut` allocations.
//!
//! The server's connection handling frequently allocates and frees buffers for
//! reading and writing wire protocol frames. This pool recycles `BytesMut`
//! instances to reduce allocator pressure and improve throughput.
//!
//! Backed by `crossbeam_queue::ArrayQueue` for lock-free, bounded pooling.
//! When the pool is empty, `get()` allocates a fresh buffer. When the pool
//! is full, `put()` drops the buffer instead of returning it.

use bytes::BytesMut;
use crossbeam_queue::ArrayQueue;

/// A lock-free pool of `BytesMut` buffers for reuse across connections.
///
/// # Design
///
/// - `get()` pops a recycled buffer or allocates a new one with `default_capacity`.
/// - `put()` clears the buffer and returns it to the pool (or drops it if full).
/// - The pool is bounded to prevent unbounded memory growth after traffic spikes.
///
/// # Sizing
///
/// Choose `pool_size` based on the expected number of concurrent connections.
/// Choose `default_capacity` based on the typical wire frame size (e.g., 8 KiB).
pub struct BytesMutPool {
    pool: ArrayQueue<BytesMut>,
    default_capacity: usize,
}

impl BytesMutPool {
    /// Creates a new buffer pool.
    ///
    /// # Arguments
    ///
    /// * `pool_size` - Maximum number of buffers to keep in the pool.
    /// * `default_capacity` - Initial capacity for newly allocated buffers.
    ///
    /// # Panics
    ///
    /// Panics if `pool_size` is 0 or `default_capacity` is 0.
    pub fn new(pool_size: usize, default_capacity: usize) -> Self {
        assert!(pool_size > 0, "pool_size must be positive");
        assert!(default_capacity > 0, "default_capacity must be positive");
        Self {
            pool: ArrayQueue::new(pool_size),
            default_capacity,
        }
    }

    /// Retrieves a buffer from the pool, or allocates a new one if empty.
    ///
    /// Recycled buffers are cleared (length == 0) but retain their allocated
    /// capacity, avoiding reallocation for typical frame sizes.
    pub fn get(&self) -> BytesMut {
        self.pool
            .pop()
            .unwrap_or_else(|| BytesMut::with_capacity(self.default_capacity))
    }

    /// Returns a buffer to the pool for reuse.
    ///
    /// The buffer is cleared before being placed back. If the pool is already
    /// full, the buffer is silently dropped.
    pub fn put(&self, mut buf: BytesMut) {
        buf.clear();
        // If the pool is full, the buffer is dropped. This is intentional:
        // after a traffic spike, we shed excess buffers rather than growing
        // memory indefinitely.
        let _ = self.pool.push(buf);
    }

    /// Returns the number of buffers currently available in the pool.
    pub fn available(&self) -> usize {
        self.pool.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_returns_buffer_with_default_capacity() {
        let pool = BytesMutPool::new(4, 1024);
        let buf = pool.get();
        assert!(buf.is_empty());
        assert!(buf.capacity() >= 1024);
    }

    #[test]
    fn get_put_roundtrip() {
        let pool = BytesMutPool::new(4, 256);

        // Get a buffer, write some data, then return it
        let mut buf = pool.get();
        buf.extend_from_slice(b"hello world");
        assert_eq!(buf.len(), 11);

        let original_capacity = buf.capacity();
        pool.put(buf);

        assert_eq!(pool.available(), 1);

        // Get the recycled buffer: should be cleared but retain capacity
        let recycled = pool.get();
        assert!(recycled.is_empty(), "recycled buffer should be cleared");
        assert_eq!(
            recycled.capacity(),
            original_capacity,
            "recycled buffer should retain its capacity"
        );

        assert_eq!(pool.available(), 0);
    }

    #[test]
    fn pool_exhaustion_allocates_fresh() {
        let pool = BytesMutPool::new(2, 512);

        // Pool starts empty, so every get() allocates fresh
        let b1 = pool.get();
        let b2 = pool.get();
        let b3 = pool.get();

        assert!(b1.capacity() >= 512);
        assert!(b2.capacity() >= 512);
        assert!(b3.capacity() >= 512);
        assert_eq!(pool.available(), 0);
    }

    #[test]
    fn put_clears_buffer_contents() {
        let pool = BytesMutPool::new(4, 128);

        let mut buf = pool.get();
        buf.extend_from_slice(b"sensitive data that should be cleared");
        assert!(!buf.is_empty());

        pool.put(buf);

        let recycled = pool.get();
        assert!(recycled.is_empty(), "buffer must be cleared on put");
    }

    #[test]
    fn capacity_limit_drops_excess() {
        let pool = BytesMutPool::new(2, 64);

        // Fill the pool to capacity
        pool.put(BytesMut::with_capacity(64));
        pool.put(BytesMut::with_capacity(64));
        assert_eq!(pool.available(), 2);

        // Third put should silently drop (pool is full)
        pool.put(BytesMut::with_capacity(64));
        assert_eq!(pool.available(), 2, "pool should not exceed its capacity");
    }

    #[test]
    fn available_tracks_pool_state() {
        let pool = BytesMutPool::new(3, 128);
        assert_eq!(pool.available(), 0);

        pool.put(BytesMut::with_capacity(128));
        assert_eq!(pool.available(), 1);

        pool.put(BytesMut::with_capacity(128));
        assert_eq!(pool.available(), 2);

        let _buf = pool.get();
        assert_eq!(pool.available(), 1);

        let _buf = pool.get();
        assert_eq!(pool.available(), 0);

        // Getting from empty pool still works (fresh allocation)
        let _buf = pool.get();
        assert_eq!(pool.available(), 0);
    }

    #[test]
    #[should_panic(expected = "pool_size must be positive")]
    fn zero_pool_size_panics() {
        let _pool = BytesMutPool::new(0, 128);
    }

    #[test]
    #[should_panic(expected = "default_capacity must be positive")]
    fn zero_default_capacity_panics() {
        let _pool = BytesMutPool::new(4, 0);
    }

    #[test]
    fn multiple_roundtrips() {
        let pool = BytesMutPool::new(2, 256);

        for i in 0..10 {
            let mut buf = pool.get();
            let data = format!("iteration {i}");
            buf.extend_from_slice(data.as_bytes());
            pool.put(buf);
        }

        // Pool should have exactly 1 buffer (last put)
        assert_eq!(pool.available(), 1);
    }
}
