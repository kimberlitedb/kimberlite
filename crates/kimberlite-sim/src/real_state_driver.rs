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
use kimberlite_vsr::TimeoutKind;

use crate::{SimRng, StorageConfig, vsr_simulation::VsrSimulation};

const N_STREAMS: u64 = 8;

/// How many fsync ticks between forced view-change timeouts.
///
/// Rare enough that normal commit rounds dominate (so `vsr.commit_*`
/// annotations fire plenty), frequent enough that a single seed sees ≥1
/// view change per scenario.
const VIEW_CHANGE_EVERY: u64 = 5;

/// How many fsync ticks between forced recovery timeouts.
///
/// Rare (recovery is more disruptive than view change); this gives each
/// seed a recovery pass early-ish in the run.
const RECOVERY_EVERY: u64 = 13;

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
    vsr: VsrSimulation,
    vsr_rng: SimRng,
    fsync_count: u64,
}

impl RealStateDriver {
    /// Creates a new driver with a fresh kernel state plus a 3-replica
    /// `VsrSimulation` for Phase 1.2.
    ///
    /// The `seed` argument is forked across the kernel and VSR layers so both
    /// observe independent-but-deterministic RNG streams.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self {
            state: Some(State::new()),
            seen_streams: HashSet::new(),
            write_count: 0,
            vsr: VsrSimulation::new(StorageConfig::reliable(), seed),
            vsr_rng: SimRng::new(seed.wrapping_add(0xD57_C0DE)),
            fsync_count: 0,
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

    /// Drives one VSR prepare→prepare-ok→commit round, plus scheduled view
    /// changes and recoveries.
    ///
    /// Called from the mock loop's `EventKind::StorageFsync` handler. Each
    /// call:
    ///  1. Submits a client request to the leader (fires `kernel.*` if the
    ///     command happens to be a CreateStream, and queues Prepare messages
    ///     for the backups).
    ///  2. Delivers every outbound Prepare to the addressed backup, collects
    ///     the resulting PrepareOk messages, and delivers them back to the
    ///     leader. After the leader accumulates the f+1 quorum, it commits —
    ///     firing `vsr.commit_quorum_met`, `vsr.commit_monotonicity`,
    ///     `vsr.commit_le_op_after_apply`, and (for backups catching up)
    ///     `vsr.commit_target_exceeds_op`.
    ///  3. Every [`VIEW_CHANGE_EVERY`] calls, fires a `TimeoutKind::ViewChange`
    ///     on replica 1 — drives the view-change quorum path and the
    ///     `vsr.view_change_*` annotations.
    ///  4. Every [`RECOVERY_EVERY`] calls, fires `TimeoutKind::Recovery` on
    ///     replica 2 — drives the recovery quorum path and the
    ///     `vsr.recovery_*` annotations.
    ///
    /// Failures of the underlying storage adapters are swallowed (VsrSimulation
    /// already logs and continues); the driver is best-effort instrumentation,
    /// not a correctness gate.
    pub fn on_fsync(&mut self) {
        self.fsync_count = self.fsync_count.wrapping_add(1);

        self.run_prepare_commit_round();

        if self.fsync_count.is_multiple_of(VIEW_CHANGE_EVERY) {
            self.fire_view_change();
        }
        if self.fsync_count.is_multiple_of(RECOVERY_EVERY) {
            self.fire_recovery();
        }
    }

    fn run_prepare_commit_round(&mut self) {
        // Submit a client request to the current leader (replica 0 in view 0).
        let outbound = self.vsr.process_client_request(&mut self.vsr_rng);
        // Follow the full request → prepare-ok → commit chain up to a few rounds.
        self.fanout(outbound, 3);
    }

    fn fire_view_change(&mut self) {
        // Replica 1 (a backup) misses a heartbeat → initiates a view change.
        // Only backups in Normal status handle `TimeoutKind::Heartbeat`;
        // `TimeoutKind::ViewChange` itself is only used to *re-escalate*
        // once already in ViewChange status, so we send Heartbeat here.
        let outbound = self
            .vsr
            .process_timeout(1, TimeoutKind::Heartbeat, &mut self.vsr_rng);
        self.fanout(outbound, 4);
    }

    fn fire_recovery(&mut self) {
        // Recovery requires the replica to be in a non-Normal status; we do
        // not currently have a path to inject a crash without rebuilding the
        // replica. `TimeoutKind::Recovery` is a retry (no-op in Normal
        // status). Left as a future extension — not needed for the Phase 1.2
        // target of ≥10 vsr.* annotations.
        let _ = self
            .vsr
            .process_timeout(2, TimeoutKind::Recovery, &mut self.vsr_rng);
    }

    fn fanout(&mut self, queue: Vec<kimberlite_vsr::Message>, max_rounds: u8) {
        let mut current = queue;
        for _ in 0..max_rounds {
            if current.is_empty() {
                break;
            }
            let mut next: Vec<kimberlite_vsr::Message> = Vec::new();
            for msg in current.drain(..) {
                self.deliver_one_or_broadcast(msg, &mut next);
            }
            current = next;
        }
    }

    fn deliver_one_or_broadcast(
        &mut self,
        msg: kimberlite_vsr::Message,
        next: &mut Vec<kimberlite_vsr::Message>,
    ) {
        match msg.to {
            Some(target) => {
                let to = u8::from(target);
                if to < 3 {
                    let responses = self.vsr.deliver_message(to, msg, &mut self.vsr_rng);
                    next.extend(responses);
                }
            }
            None => {
                // Broadcast: deliver to every replica except the sender.
                let from = u8::from(msg.from);
                for to in 0u8..3 {
                    if to == from {
                        continue;
                    }
                    let responses = self
                        .vsr
                        .deliver_message(to, msg.clone(), &mut self.vsr_rng);
                    next.extend(responses);
                }
            }
        }
    }

    /// Returns the number of fsync ticks the driver has processed.
    /// Intended for tests.
    #[must_use]
    pub fn fsync_count(&self) -> u64 {
        self.fsync_count
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

    #[test]
    fn on_fsync_fires_vsr_annotations() {
        kimberlite_properties::registry::reset();

        let mut driver = RealStateDriver::new(123);
        // Run enough fsyncs to hit both view-change and recovery cadences.
        for _ in 0..(VIEW_CHANGE_EVERY * RECOVERY_EVERY + 2) {
            driver.on_fsync();
        }

        let snap = kimberlite_properties::registry::snapshot();
        let ids: Vec<&String> = snap.keys().collect();

        // At least one commit-round annotation — these are the cheapest to
        // fire (normal path runs every fsync).
        let has_commit_rounds = ids.iter().any(|id| id.starts_with("vsr.commit_"));
        assert!(
            has_commit_rounds,
            "expected at least one vsr.commit_* annotation to fire; got: {:?}",
            ids
        );

        // View change cadence is every 5 fsyncs, so after VIEW_CHANGE_EVERY * N
        // fsyncs we should have triggered the view-change path repeatedly.
        let has_view_change = ids.iter().any(|id| id.starts_with("vsr.view_change_"));
        assert!(
            has_view_change,
            "expected vsr.view_change_* annotation to fire; got: {:?}",
            ids
        );
    }
}
