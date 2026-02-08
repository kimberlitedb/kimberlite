//! Bounded queue with backpressure for the VSR event loop.
//!
//! Uses `crossbeam-queue::ArrayQueue` for a lock-free, bounded MPSC queue.
//! When the queue is full, `try_push` returns the item back to the caller
//! to signal backpressure.
//!
//! # Sizing
//!
//! Size the queue using Little's Law: `capacity = throughput * latency`.
//! For example, at 100k ops/sec with 10ms latency: `capacity = 100_000 * 0.01 = 1000`.

use crossbeam_queue::ArrayQueue;

/// Result of attempting to push to a full queue.
#[derive(Debug)]
pub enum PushResult<T> {
    /// Item was successfully enqueued.
    Ok,
    /// Queue is full. Returns the item for the caller to handle.
    Backpressure(T),
}

/// A bounded, lock-free queue with backpressure signaling.
///
/// When the queue is full, producers receive their item back instead of
/// blocking. This enables the server to send `ServerBusy` responses to
/// clients rather than accumulating unbounded memory.
#[derive(Debug)]
pub struct BoundedQueue<T> {
    inner: ArrayQueue<T>,
}

impl<T> BoundedQueue<T> {
    /// Creates a new bounded queue with the given capacity.
    ///
    /// # Panics
    ///
    /// Panics if `capacity` is 0.
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "queue capacity must be positive");
        Self {
            inner: ArrayQueue::new(capacity),
        }
    }

    /// Attempts to push an item onto the queue.
    ///
    /// Returns `PushResult::Ok` if successful, or `PushResult::Backpressure(item)`
    /// if the queue is full.
    pub fn try_push(&self, item: T) -> PushResult<T> {
        match self.inner.push(item) {
            Ok(()) => PushResult::Ok,
            Err(item) => PushResult::Backpressure(item),
        }
    }

    /// Attempts to pop an item from the queue.
    ///
    /// Returns `None` if the queue is empty.
    pub fn try_pop(&self) -> Option<T> {
        self.inner.pop()
    }

    /// Pops up to `max` items from the queue into a `Vec`.
    ///
    /// Returns an empty `Vec` if the queue is empty.
    pub fn pop_batch(&self, max: usize) -> Vec<T> {
        let mut batch = Vec::with_capacity(max.min(self.inner.len()));
        for _ in 0..max {
            match self.inner.pop() {
                Some(item) => batch.push(item),
                None => break,
            }
        }
        batch
    }

    /// Returns the number of items currently in the queue.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns true if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Returns true if the queue is full.
    pub fn is_full(&self) -> bool {
        self.inner.is_full()
    }

    /// Returns the capacity of the queue.
    pub fn capacity(&self) -> usize {
        self.inner.capacity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_push_pop() {
        let q = BoundedQueue::new(3);

        assert!(matches!(q.try_push(1), PushResult::Ok));
        assert!(matches!(q.try_push(2), PushResult::Ok));
        assert!(matches!(q.try_push(3), PushResult::Ok));

        assert_eq!(q.try_pop(), Some(1));
        assert_eq!(q.try_pop(), Some(2));
        assert_eq!(q.try_pop(), Some(3));
        assert_eq!(q.try_pop(), None);
    }

    #[test]
    fn backpressure_when_full() {
        let q = BoundedQueue::new(2);

        assert!(matches!(q.try_push(1), PushResult::Ok));
        assert!(matches!(q.try_push(2), PushResult::Ok));

        // Queue is full, should get backpressure
        match q.try_push(3) {
            PushResult::Backpressure(v) => assert_eq!(v, 3),
            PushResult::Ok => panic!("expected backpressure"),
        }
    }

    #[test]
    fn pop_batch_drains() {
        let q = BoundedQueue::new(10);
        for i in 0..5 {
            let _ = q.try_push(i);
        }

        let batch = q.pop_batch(3);
        assert_eq!(batch, vec![0, 1, 2]);
        assert_eq!(q.len(), 2);

        let batch = q.pop_batch(10);
        assert_eq!(batch, vec![3, 4]);
        assert!(q.is_empty());
    }

    #[test]
    fn pop_batch_empty() {
        let q: BoundedQueue<i32> = BoundedQueue::new(10);
        let batch = q.pop_batch(5);
        assert!(batch.is_empty());
    }

    #[test]
    fn capacity_and_len() {
        let q = BoundedQueue::new(5);
        assert_eq!(q.capacity(), 5);
        assert_eq!(q.len(), 0);
        assert!(q.is_empty());
        assert!(!q.is_full());

        for i in 0..5 {
            let _ = q.try_push(i);
        }
        assert_eq!(q.len(), 5);
        assert!(!q.is_empty());
        assert!(q.is_full());
    }

    #[test]
    #[should_panic(expected = "queue capacity must be positive")]
    fn zero_capacity_panics() {
        let _q: BoundedQueue<i32> = BoundedQueue::new(0);
    }
}
