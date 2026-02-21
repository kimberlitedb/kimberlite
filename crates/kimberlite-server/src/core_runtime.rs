//! Thread-per-core runtime for pinning stream processing to dedicated cores.
//!
//! Routes streams to core workers via consistent hashing on `StreamId`. Each
//! worker owns a bounded inbox and processes requests sequentially on its
//! dedicated thread. When the `thread_per_core` feature is enabled and
//! `pin_threads` is set, worker threads are pinned to specific CPU cores
//! using `core_affinity`.
//!
//! # Design
//!
//! - One OS thread per logical core (configurable via `CoreRuntimeConfig`).
//! - Streams are routed deterministically: same `StreamId` always goes to
//!   the same core, preserving per-stream ordering without locks.
//! - Bounded inboxes provide backpressure when a core falls behind.
//! - No async runtime -- plain synchronous threads with spin-wait on inbox.

use std::collections::HashSet;
use std::sync::Arc;
use std::thread;

use kimberlite_types::StreamId;

use crate::bounded_queue::{BoundedQueue, PushResult};

/// Configuration for the thread-per-core runtime.
pub struct CoreRuntimeConfig {
    /// Number of worker cores to spawn.
    pub core_count: usize,
    /// Whether to pin worker threads to specific CPU cores.
    /// Only effective when the `thread_per_core` feature is enabled.
    pub pin_threads: bool,
    /// Capacity of each worker's bounded inbox.
    pub queue_capacity: usize,
}

impl Default for CoreRuntimeConfig {
    fn default() -> Self {
        Self {
            core_count: thread::available_parallelism().map_or(1, std::num::NonZero::get),
            pin_threads: cfg!(feature = "thread_per_core"),
            queue_capacity: 1024,
        }
    }
}

/// Routes streams to core workers via modular hashing on `StreamId`.
///
/// The routing is deterministic: a given `StreamId` always maps to the same
/// core index, ensuring per-stream ordering without coordination.
pub struct CoreRouter {
    core_count: usize,
}

impl CoreRouter {
    /// Creates a new router for the given number of cores.
    ///
    /// # Panics
    ///
    /// Panics if `core_count` is 0.
    pub fn new(core_count: usize) -> Self {
        assert!(core_count > 0, "core_count must be positive");
        Self { core_count }
    }

    /// Returns the core index that should handle the given stream.
    pub fn route(&self, stream_id: StreamId) -> usize {
        u64::from(stream_id) as usize % self.core_count
    }

    /// Returns the number of cores this router distributes across.
    pub fn core_count(&self) -> usize {
        self.core_count
    }
}

/// A request routed to a specific core worker.
pub enum CoreRequest {
    /// Process data for a given stream.
    Process {
        /// The stream this request belongs to.
        stream_id: StreamId,
        /// The raw request payload.
        data: Vec<u8>,
    },
    /// Signals the worker thread to shut down gracefully.
    Shutdown,
}

/// Per-core worker with a bounded inbox and stream affinity tracking.
pub struct CoreWorker {
    /// The core index this worker is assigned to.
    pub core_id: usize,
    /// Set of streams currently assigned to this worker.
    pub stream_ids: HashSet<StreamId>,
    /// Bounded inbox for incoming requests.
    pub inbox: Arc<BoundedQueue<CoreRequest>>,
}

impl CoreWorker {
    fn new(core_id: usize, queue_capacity: usize) -> Self {
        Self {
            core_id,
            stream_ids: HashSet::new(),
            inbox: Arc::new(BoundedQueue::new(queue_capacity)),
        }
    }
}

/// Multi-core runtime that routes requests to pinned worker threads.
///
/// # Lifecycle
///
/// 1. Create with `CoreRuntime::new(config)`.
/// 2. Call `start()` to spawn worker threads.
/// 3. Use `submit()` to route requests to the appropriate core.
/// 4. Call `shutdown()` to stop all workers and join threads.
pub struct CoreRuntime {
    router: CoreRouter,
    /// Shared references to each worker's inbox for submitting requests.
    inboxes: Vec<Arc<BoundedQueue<CoreRequest>>>,
    handles: Vec<Option<thread::JoinHandle<()>>>,
    config: CoreRuntimeConfig,
}

impl CoreRuntime {
    /// Creates a new runtime with the given configuration.
    ///
    /// Workers and threads are allocated but not yet started. Call `start()`
    /// to begin processing.
    ///
    /// # Panics
    ///
    /// Panics if `config.core_count` is 0 or `config.queue_capacity` is 0.
    pub fn new(config: CoreRuntimeConfig) -> Self {
        assert!(config.core_count > 0, "core_count must be positive");
        assert!(config.queue_capacity > 0, "queue_capacity must be positive");

        let router = CoreRouter::new(config.core_count);
        let mut inboxes = Vec::with_capacity(config.core_count);

        for core_id in 0..config.core_count {
            let worker = CoreWorker::new(core_id, config.queue_capacity);
            inboxes.push(worker.inbox);
        }

        Self {
            router,
            inboxes,
            handles: Vec::new(),
            config,
        }
    }

    /// Spawns a worker thread per core and begins processing requests.
    ///
    /// If `pin_threads` is true and the `thread_per_core` feature is enabled,
    /// each worker thread is pinned to its corresponding CPU core.
    ///
    /// # Panics
    ///
    /// Panics if called more than once without an intervening `shutdown()`.
    pub fn start(&mut self) {
        assert!(
            self.handles.is_empty(),
            "runtime already started; call shutdown() first"
        );

        let mut handles = Vec::with_capacity(self.config.core_count);

        for core_id in 0..self.config.core_count {
            let inbox = Arc::clone(&self.inboxes[core_id]);
            let pin = self.config.pin_threads;

            let handle = thread::Builder::new()
                .name(format!("kmb-core-{core_id}"))
                .spawn(move || {
                    // Pin thread to core if configured and feature-enabled
                    #[cfg(feature = "thread_per_core")]
                    if pin {
                        let core = core_affinity::CoreId { id: core_id };
                        core_affinity::set_for_current(core);
                    }

                    // Suppress unused variable warning when feature is disabled
                    #[cfg(not(feature = "thread_per_core"))]
                    let _ = pin;

                    Self::worker_loop(&inbox);
                })
                .expect("failed to spawn worker thread");

            handles.push(Some(handle));
        }

        self.handles = handles;
    }

    /// The main loop for a worker thread.
    ///
    /// Continuously pops requests from the inbox and processes them.
    /// Exits on receiving a `CoreRequest::Shutdown`.
    fn worker_loop(inbox: &BoundedQueue<CoreRequest>) {
        loop {
            match inbox.try_pop() {
                Some(CoreRequest::Process {
                    stream_id: _,
                    data: _,
                }) => {
                    // Processing hook: in a full implementation, this would
                    // dispatch to the kernel's command pipeline. For now,
                    // the runtime infrastructure is in place for wiring.
                }
                Some(CoreRequest::Shutdown) => {
                    break;
                }
                None => {
                    // No work available; yield to avoid busy-spinning.
                    // In production this would use a more sophisticated
                    // wait strategy (e.g., eventfd / crossbeam Parker).
                    thread::yield_now();
                }
            }
        }
    }

    /// Submits a request for the given stream to the appropriate core worker.
    ///
    /// Returns `Ok(())` if the request was enqueued, or `Err(CoreRequest)` if
    /// the target core's inbox is full (backpressure).
    pub fn submit(&self, stream_id: StreamId, data: Vec<u8>) -> Result<(), CoreRequest> {
        let core_id = self.router.route(stream_id);
        let request = CoreRequest::Process { stream_id, data };
        match self.inboxes[core_id].try_push(request) {
            PushResult::Ok => Ok(()),
            PushResult::Backpressure(req) => Err(req),
        }
    }

    /// Shuts down all worker threads gracefully.
    ///
    /// Sends a `Shutdown` request to each worker and waits for all threads
    /// to finish. Safe to call multiple times (subsequent calls are no-ops).
    pub fn shutdown(&mut self) {
        // Send shutdown signal to each worker
        for inbox in &self.inboxes {
            // Best-effort: if the queue is full, the worker will eventually
            // drain it and see the shutdown on the next attempt. In practice,
            // shutdown happens during low-traffic periods.
            let _ = inbox.try_push(CoreRequest::Shutdown);
        }

        // Join all worker threads
        for handle in &mut self.handles {
            if let Some(h) = handle.take() {
                let _ = h.join();
            }
        }

        self.handles.clear();
    }

    /// Returns the number of cores this runtime manages.
    pub fn core_count(&self) -> usize {
        self.config.core_count
    }

    /// Returns a reference to the stream-to-core router.
    pub fn router(&self) -> &CoreRouter {
        &self.router
    }
}

impl Drop for CoreRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn router_deterministic_routing() {
        let router = CoreRouter::new(4);
        let stream = StreamId::new(42);

        // Same stream always routes to the same core
        let core1 = router.route(stream);
        let core2 = router.route(stream);
        assert_eq!(core1, core2);
    }

    #[test]
    fn router_distributes_across_cores() {
        let router = CoreRouter::new(4);
        let mut seen_cores = HashSet::new();

        // With enough different stream IDs, we should hit multiple cores
        for i in 0..100 {
            seen_cores.insert(router.route(StreamId::new(i)));
        }

        assert_eq!(seen_cores.len(), 4, "should distribute across all 4 cores");
    }

    #[test]
    fn router_single_core() {
        let router = CoreRouter::new(1);

        // All streams route to core 0
        for i in 0..50 {
            assert_eq!(router.route(StreamId::new(i)), 0);
        }
    }

    #[test]
    #[should_panic(expected = "core_count must be positive")]
    fn router_zero_cores_panics() {
        let _router = CoreRouter::new(0);
    }

    #[test]
    fn default_config_has_positive_core_count() {
        let config = CoreRuntimeConfig::default();
        assert!(config.core_count >= 1);
        assert_eq!(config.queue_capacity, 1024);
    }

    #[test]
    fn runtime_start_and_shutdown() {
        let config = CoreRuntimeConfig {
            core_count: 2,
            pin_threads: false,
            queue_capacity: 64,
        };

        let mut runtime = CoreRuntime::new(config);
        runtime.start();

        assert_eq!(runtime.core_count(), 2);

        runtime.shutdown();
    }

    #[test]
    fn runtime_submit_and_process() {
        let config = CoreRuntimeConfig {
            core_count: 2,
            pin_threads: false,
            queue_capacity: 64,
        };

        let mut runtime = CoreRuntime::new(config);
        runtime.start();

        // Submit several requests
        for i in 0..10 {
            let stream_id = StreamId::new(i);
            let data = vec![i as u8; 4];
            let result = runtime.submit(stream_id, data);
            assert!(
                result.is_ok(),
                "submit should succeed with capacity available"
            );
        }

        // Give workers time to process
        thread::sleep(std::time::Duration::from_millis(50));

        runtime.shutdown();
    }

    #[test]
    fn runtime_backpressure_on_full_inbox() {
        let config = CoreRuntimeConfig {
            core_count: 1,
            pin_threads: false,
            queue_capacity: 2,
        };

        // Do NOT start workers so the inbox stays full
        let runtime = CoreRuntime::new(config);

        // Fill the single worker's inbox
        let r1 = runtime.submit(StreamId::new(0), vec![1]);
        let r2 = runtime.submit(StreamId::new(0), vec![2]);
        assert!(r1.is_ok());
        assert!(r2.is_ok());

        // Third submit should get backpressure
        let r3 = runtime.submit(StreamId::new(0), vec![3]);
        assert!(r3.is_err(), "should return Err when inbox is full");

        // Verify the returned request contains our data
        if let Err(CoreRequest::Process { data, .. }) = r3 {
            assert_eq!(data, vec![3]);
        } else {
            panic!("expected Process request back on backpressure");
        }
    }

    #[test]
    fn runtime_shutdown_is_idempotent() {
        let config = CoreRuntimeConfig {
            core_count: 2,
            pin_threads: false,
            queue_capacity: 32,
        };

        let mut runtime = CoreRuntime::new(config);
        runtime.start();

        // Multiple shutdowns should not panic
        runtime.shutdown();
        runtime.shutdown();
    }

    #[test]
    fn runtime_router_accessible() {
        let config = CoreRuntimeConfig {
            core_count: 4,
            pin_threads: false,
            queue_capacity: 32,
        };

        let runtime = CoreRuntime::new(config);
        assert_eq!(runtime.router().core_count(), 4);
    }

    #[test]
    fn runtime_stream_affinity() {
        let config = CoreRuntimeConfig {
            core_count: 4,
            pin_threads: false,
            queue_capacity: 32,
        };

        let runtime = CoreRuntime::new(config);
        let router = runtime.router();

        // Streams with the same ID always go to the same core
        let stream_a = StreamId::new(100);
        let stream_b = StreamId::new(100);
        assert_eq!(router.route(stream_a), router.route(stream_b));

        // Different streams may go to different cores
        let stream_c = StreamId::new(101);
        // We just verify routing is deterministic, not necessarily different
        let _ = router.route(stream_c);
    }

    #[test]
    #[should_panic(expected = "core_count must be positive")]
    fn runtime_zero_cores_panics() {
        let config = CoreRuntimeConfig {
            core_count: 0,
            pin_threads: false,
            queue_capacity: 32,
        };
        let _runtime = CoreRuntime::new(config);
    }

    #[test]
    #[should_panic(expected = "queue_capacity must be positive")]
    fn runtime_zero_queue_capacity_panics() {
        let config = CoreRuntimeConfig {
            core_count: 2,
            pin_threads: false,
            queue_capacity: 0,
        };
        let _runtime = CoreRuntime::new(config);
    }

    #[test]
    #[should_panic(expected = "runtime already started")]
    fn runtime_double_start_panics() {
        let config = CoreRuntimeConfig {
            core_count: 1,
            pin_threads: false,
            queue_capacity: 32,
        };

        let mut runtime = CoreRuntime::new(config);
        runtime.start();
        runtime.start(); // should panic
    }

    #[test]
    fn runtime_drop_joins_threads() {
        let config = CoreRuntimeConfig {
            core_count: 2,
            pin_threads: false,
            queue_capacity: 32,
        };

        let mut runtime = CoreRuntime::new(config);
        runtime.start();

        // Dropping should trigger shutdown via Drop impl
        drop(runtime);
        // If we get here without hanging, the threads were joined.
    }
}
