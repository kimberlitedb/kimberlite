//! RealStateDriver — runs the real kernel/VSR/compliance/query code paths
//! alongside the mock VOPR simulation so property annotations actually fire.
//!
//! The mock simulation drives `SimStorage` + `KimberliteModel` with opaque
//! `EventKind::Custom(u64)` writes; those paths never touch
//! `kimberlite_kernel::apply_committed`, `kimberlite_vsr::ReplicaState`, the
//! compliance audit log, or the query executor. Without a real-code side-car,
//! only `crypto.blake3_internal_hash_exercised` registers annotations.
//!
//! This driver lives only under `cfg(any(test, feature = "sim"))`, so
//! production builds are unaffected. It is constructed inside `run_simulation`
//! after `kimberlite_properties::registry::reset()` so per-seed reporting is
//! correct.
//!
//! Phase 1.1 covers the kernel; later phases extend the same struct with VSR,
//! compliance, and query workloads.

use std::collections::HashSet;

use bytes::Bytes;

use kimberlite_kernel::command::Command;
use kimberlite_kernel::kernel::{apply_committed, apply_committed_batch};
use kimberlite_kernel::state::State;
use kimberlite_types::{DataClass, Offset, Placement, StreamId, StreamName};

const N_STREAMS: u64 = 8;

/// Drives real kimberlite-kernel code paths from inside the VOPR simulation
/// loop so property annotations register.
///
/// `RealStateDriver` owns a kernel `State` (the append-only functional core)
/// and a set of "seen" stream IDs. Each call to [`RealStateDriver::on_write`]
/// issues a real `Command` into `apply_committed`, firing the kernel's
/// always!/sometimes! annotations.
pub struct RealStateDriver {
    state: Option<State>,
    seen_streams: HashSet<StreamId>,
    write_count: u64,
}

impl RealStateDriver {
    /// Creates a new driver with a fresh kernel state.
    ///
    /// The `seed` argument is accepted for future phases that need
    /// deterministic randomness (VSR timing, query workload shuffling).
    #[must_use]
    pub fn new(_seed: u64) -> Self {
        Self {
            state: Some(State::new()),
            seen_streams: HashSet::new(),
            write_count: 0,
        }
    }

    /// Drives a write from the mock loop into the real kernel.
    ///
    /// Maps `key` to one of `N_STREAMS` stream IDs. On the first write to a
    /// given stream, issues `Command::CreateStream` to fire
    /// `kernel.stream_exists_after_create` and
    /// `kernel.stream_zero_offset_after_create`. Then appends a single-event
    /// batch; every 4th write batches two events via `apply_committed_batch`
    /// to fire `kernel.multi_event_batch` and `kernel.batch_min_effects`.
    pub fn on_write(&mut self, key: u64, value: u64) {
        let stream_id = StreamId::new((key % N_STREAMS) + 1);

        if !self.seen_streams.contains(&stream_id) {
            self.create_stream(stream_id);
            self.seen_streams.insert(stream_id);
        }

        self.write_count = self.write_count.wrapping_add(1);
        if self.write_count.is_multiple_of(4) {
            self.append_batch(stream_id, 2, value);
        } else {
            self.append_batch(stream_id, 1, value);
        }
    }

    fn create_stream(&mut self, stream_id: StreamId) {
        let state = match self.state.take() {
            Some(s) => s,
            None => return,
        };
        let cmd = Command::CreateStream {
            stream_id,
            stream_name: StreamName::new(format!("sim_stream_{}", u64::from(stream_id))),
            data_class: DataClass::Public,
            placement: Placement::Global,
        };
        match apply_committed(state, cmd) {
            Ok((new_state, _effects)) => self.state = Some(new_state),
            Err(_) => self.state = None,
        }
    }

    fn append_batch(&mut self, stream_id: StreamId, event_count: usize, seed_value: u64) {
        let state = match self.state.take() {
            Some(s) => s,
            None => return,
        };
        let Some(stream) = state.get_stream(&stream_id) else {
            self.state = Some(state);
            return;
        };
        let expected_offset = stream.current_offset;
        let events: Vec<Bytes> = (0..event_count)
            .map(|i| {
                let v = seed_value.wrapping_add(i as u64);
                Bytes::from(v.to_le_bytes().to_vec())
            })
            .collect();
        let cmd = Command::AppendBatch {
            stream_id,
            events,
            expected_offset,
        };
        if event_count == 1 {
            match apply_committed(state, cmd) {
                Ok((new_state, _effects)) => self.state = Some(new_state),
                Err(_) => self.state = None,
            }
        } else {
            match apply_committed_batch(state, vec![cmd]) {
                Ok((new_state, _effects)) => self.state = Some(new_state),
                Err(_) => self.state = None,
            }
        }
    }

    /// Returns the number of streams this driver has created. Intended for
    /// tests.
    #[must_use]
    pub fn stream_count(&self) -> usize {
        self.seen_streams.len()
    }

    /// Returns the total number of writes this driver has processed.
    /// Intended for tests.
    #[must_use]
    pub fn write_count(&self) -> u64 {
        self.write_count
    }

    /// Returns the current head offset of the given stream, if it exists.
    /// Intended for tests.
    #[must_use]
    pub fn stream_offset(&self, stream_id: StreamId) -> Option<Offset> {
        self.state
            .as_ref()
            .and_then(|s| s.get_stream(&stream_id))
            .map(|s| s.current_offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_write_creates_stream_and_appends() {
        kimberlite_properties::registry::reset();

        let mut driver = RealStateDriver::new(0);
        driver.on_write(0, 42);

        assert_eq!(driver.stream_count(), 1);
        assert_eq!(driver.write_count(), 1);

        let stream_id = StreamId::new(1);
        assert_eq!(driver.stream_offset(stream_id), Some(Offset::new(1)));

        let snap = kimberlite_properties::registry::snapshot();
        assert!(
            snap.contains_key("kernel.stream_exists_after_create"),
            "expected kernel.stream_exists_after_create to fire on first write, got: {:?}",
            snap.keys().collect::<Vec<_>>()
        );
        assert!(snap.contains_key("kernel.stream_zero_offset_after_create"));
        assert!(snap.contains_key("kernel.offset_monotonicity"));
        assert!(snap.contains_key("kernel.append_offset_consistent"));
    }

    #[test]
    fn every_fourth_write_uses_batch() {
        kimberlite_properties::registry::reset();

        let mut driver = RealStateDriver::new(0);
        for i in 0..4u64 {
            driver.on_write(0, i);
        }

        let snap = kimberlite_properties::registry::snapshot();
        assert!(
            snap.contains_key("kernel.batch_min_effects"),
            "expected kernel.batch_min_effects to fire after batched write"
        );
        assert!(
            snap.contains_key("kernel.multi_event_batch"),
            "expected kernel.multi_event_batch to fire with >1 events"
        );
    }

    #[test]
    fn multiple_streams_created_for_distinct_keys() {
        kimberlite_properties::registry::reset();

        let mut driver = RealStateDriver::new(0);
        for key in 0..N_STREAMS {
            driver.on_write(key, key);
        }
        assert_eq!(driver.stream_count(), N_STREAMS as usize);
    }
}
