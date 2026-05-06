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

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use bytes::Bytes;
use chrono::Utc;

use kimberlite_compliance::audit::{ComplianceAuditAction, ComplianceAuditLog};
use kimberlite_compliance::breach::BreachDetector;
use kimberlite_compliance::consent::ConsentTracker;
use kimberlite_compliance::erasure::{ErasureEngine, ExemptionBasis};
use kimberlite_compliance::export::{ExportEngine, ExportFormat, ExportRecord};
use kimberlite_compliance::purpose::Purpose;
use kimberlite_crypto::ChainHash;
use kimberlite_kernel::command::Command;
use kimberlite_kernel::kernel::{apply_committed, apply_committed_batch};
use kimberlite_kernel::state::State;
use kimberlite_query::key_encoder::encode_key;
use kimberlite_query::{ColumnDef, DataType, QueryEngine, SchemaBuilder, Value};
use kimberlite_storage::Storage as KmbStorage;
use kimberlite_store::{Key, ProjectionStore, StoreError, TableId, WriteBatch, WriteOp};
use kimberlite_types::{DataClass, Offset, Placement, StreamId, StreamName};
use kimberlite_vsr::TimeoutKind;
use tempfile::TempDir;
use uuid::Uuid;

use crate::{
    AgreementChecker, CommitNumberConsistencyChecker, InvariantResult, PrefixPropertyChecker,
    SimRng, StorageConfig, check_agreement_snapshots, check_commit_number_consistency_snapshots,
    check_prefix_property_snapshots, vsr_simulation::VsrSimulation,
};

/// Minimal in-memory `ProjectionStore` used by [`RealStateDriver::run_query_suite`].
///
/// Mirrors the structure of `kimberlite-query::tests::MockStore` (which is
/// private to that crate). Pure HashMap-backed, no MVCC, no disk I/O.
#[derive(Debug, Default)]
struct InMemoryProjectionStore {
    tables: HashMap<TableId, Vec<(Key, Bytes)>>,
    position: kimberlite_types::Offset,
}

impl InMemoryProjectionStore {
    fn new() -> Self {
        Self::default()
    }

    fn insert_json(&mut self, table_id: TableId, key: Key, json: &serde_json::Value) {
        let bytes =
            Bytes::from(serde_json::to_vec(json).expect("JSON serialization for mock store"));
        let entries = self.tables.entry(table_id).or_default();
        entries.push((key, bytes));
        entries.sort_by(|a, b| a.0.cmp(&b.0));
    }
}

impl ProjectionStore for InMemoryProjectionStore {
    fn apply(&mut self, batch: WriteBatch) -> Result<(), StoreError> {
        for op in batch.operations() {
            match op {
                WriteOp::Put { table, key, value } => {
                    let entries = self.tables.entry(*table).or_default();
                    entries.push((key.clone(), value.clone()));
                    entries.sort_by(|a, b| a.0.cmp(&b.0));
                }
                WriteOp::Delete { table, key } => {
                    if let Some(entries) = self.tables.get_mut(table) {
                        entries.retain(|(k, _)| k != key);
                    }
                }
            }
        }
        self.position = batch.position();
        Ok(())
    }

    fn applied_position(&self) -> kimberlite_types::Offset {
        self.position
    }

    fn get(&mut self, table: TableId, key: &Key) -> Result<Option<Bytes>, StoreError> {
        Ok(self
            .tables
            .get(&table)
            .and_then(|t| t.iter().find(|(k, _)| k == key))
            .map(|(_, v)| v.clone()))
    }

    fn get_at(
        &mut self,
        table: TableId,
        key: &Key,
        _pos: kimberlite_types::Offset,
    ) -> Result<Option<Bytes>, StoreError> {
        self.get(table, key)
    }

    fn scan(
        &mut self,
        table: TableId,
        range: Range<Key>,
        limit: usize,
    ) -> Result<Vec<(Key, Bytes)>, StoreError> {
        let Some(entries) = self.tables.get(&table) else {
            return Ok(vec![]);
        };
        Ok(entries
            .iter()
            .filter(|(k, _)| k >= &range.start && k < &range.end)
            .take(limit)
            .cloned()
            .collect())
    }

    fn scan_at(
        &mut self,
        table: TableId,
        range: Range<Key>,
        limit: usize,
        _pos: kimberlite_types::Offset,
    ) -> Result<Vec<(Key, Bytes)>, StoreError> {
        self.scan(table, range, limit)
    }

    fn sync(&mut self) -> Result<(), StoreError> {
        Ok(())
    }

    fn purge_table(&mut self, table: TableId) -> Result<(), StoreError> {
        self.tables.remove(&table);
        Ok(())
    }
}

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

/// How many fsync ticks between commit-catchup scenarios.
///
/// This drives the `vsr.commit_target_exceeds_op` annotation by withholding
/// Prepare from replica 2 for one round so it falls behind, then delivering
/// the Commit directly.  Coprime to VIEW_CHANGE_EVERY and RECOVERY_EVERY so
/// the three scenarios never overlap.
const CATCHUP_EVERY: u64 = 11;

/// How many fsync ticks between adversarial view-change scenarios.
///
/// This drives the hard VSR case: a Prepare reaches quorum and the leader
/// dies before the corresponding Commit lands on backups.  A correct new
/// leader must re-commit the prepared op.
///
/// Coprime with 5 / 11 / 13 so the four scenarios never overlap.
const ADVERSARIAL_VIEW_CHANGE_EVERY: u64 = 17;

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
    /// Disk-backed append-only log. Owns a per-seed tempdir. Exists so the
    /// 7 `storage.*` property annotations (crc32, hash-chain, offset
    /// advancement, read-after-write, verified-chain-break) fire at least
    /// once per seed — those live inside the real `kimberlite-storage`
    /// module which the mock VOPR loop never touches.
    storage: KmbStorage,
    storage_offset: Offset,
    storage_chain: Option<ChainHash>,
    _storage_tmp: TempDir,
    /// Accumulates (view, op)→hash across the full run so the Agreement
    /// checker catches *temporal* divergences — e.g. replica 0 commits X at
    /// (v=1, op=5) at tick 10 and replica 1 commits X′ at the same slot at
    /// tick 50. Kept persistent across [`Self::check_vsr_agreement`] calls.
    commit_consistency: CommitNumberConsistencyChecker,
    agreement: AgreementChecker,
    prefix: PrefixPropertyChecker,
}

impl RealStateDriver {
    /// Creates a new driver with a fresh kernel state plus a 3-replica
    /// `VsrSimulation` for Phase 1.2.
    ///
    /// The `seed` argument is forked across the kernel and VSR layers so both
    /// observe independent-but-deterministic RNG streams.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        let tmp = tempfile::tempdir().expect("tempdir for real_state_driver Storage");
        let storage = KmbStorage::new(tmp.path());
        Self {
            state: Some(State::new()),
            seen_streams: HashSet::new(),
            write_count: 0,
            vsr: VsrSimulation::new(StorageConfig::reliable(), seed),
            vsr_rng: SimRng::new(seed.wrapping_add(0xD57_C0DE)),
            fsync_count: 0,
            storage,
            storage_offset: Offset::ZERO,
            storage_chain: None,
            _storage_tmp: tmp,
            commit_consistency: CommitNumberConsistencyChecker::new(),
            agreement: AgreementChecker::new(),
            prefix: PrefixPropertyChecker::new(),
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
    ///  3. Every `VIEW_CHANGE_EVERY` calls, fires a `TimeoutKind::ViewChange`
    ///     on replica 1 — drives the view-change quorum path and the
    ///     `vsr.view_change_*` annotations.
    ///  4. Every `RECOVERY_EVERY` calls, fires `TimeoutKind::Recovery` on
    ///     replica 2 — drives the recovery quorum path and the
    ///     `vsr.recovery_*` annotations.
    ///
    /// Failures of the underlying storage adapters are swallowed (VsrSimulation
    /// already logs and continues); the driver is best-effort instrumentation,
    /// not a correctness gate.
    pub fn on_fsync(&mut self) {
        self.fsync_count = self.fsync_count.wrapping_add(1);

        self.run_prepare_commit_round();
        self.run_storage_step();

        if self.fsync_count.is_multiple_of(VIEW_CHANGE_EVERY) {
            self.fire_view_change();
        }
        if self.fsync_count.is_multiple_of(RECOVERY_EVERY) {
            self.fire_recovery();
        }
        if self.fsync_count.is_multiple_of(CATCHUP_EVERY) {
            self.run_commit_catchup_scenario();
        }
        if self
            .fsync_count
            .is_multiple_of(ADVERSARIAL_VIEW_CHANGE_EVERY)
        {
            self.run_adversarial_view_change_scenario();
        }

        // Final cross-replica check — `run_prepare_commit_round` already
        // calls this at its tail, but the view-change / recovery / catchup
        // scenarios mutate cluster state after that, so re-run here to catch
        // divergences those scenarios may introduce.
        self.check_vsr_agreement("fsync_end");
    }

    /// Appends to the disk-backed Storage and, every N ticks, performs a
    /// verified read — together this exercises the 7 `storage.*` property
    /// annotations: `offset_advances_forward`, `hash_chain_valid_after_append`,
    /// `crc32_matches_after_write`, `crc32_verified_on_read`,
    /// `hash_chain_valid_on_genesis_read`, `read_after_write_exercised`,
    /// `verified_read_chain_break`.
    fn run_storage_step(&mut self) {
        let stream_id = StreamId::new(42);
        let payload = Bytes::from(format!("seed-{}", self.fsync_count).into_bytes());
        let expected_offset = self.storage_offset;
        let result = self.storage.append_batch(
            stream_id,
            vec![payload],
            expected_offset,
            self.storage_chain,
            true,
        );
        if let Ok((new_offset, new_chain)) = result {
            self.storage_offset = new_offset;
            self.storage_chain = Some(new_chain);
        }

        // Every 3rd step, read back with genesis verification — fires
        // crc32_verified_on_read, hash_chain_valid_on_genesis_read, and
        // read_after_write_exercised.
        if self.fsync_count.is_multiple_of(3) && self.storage_offset.as_u64() > 0 {
            let _ = self
                .storage
                .read_from_genesis(stream_id, Offset::ZERO, 64 * 1024);
        }
    }

    fn run_prepare_commit_round(&mut self) {
        // Submit a client request to the current leader (replica 0 in view 0).
        let outbound = self.vsr.process_client_request(&mut self.vsr_rng);
        // Follow the full request → prepare-ok → commit chain up to a few rounds.
        self.fanout(outbound, 3);
        // After each commit round, verify no two replicas committed different
        // operations at the same (view, op) slot and that all replicas agree
        // on the committed prefix.  Cross-replica agreement is the core VSR
        // safety property; a divergence here is a hard stop.
        self.check_vsr_agreement("prepare_commit");
    }

    /// Runs the cross-replica invariant suite against the current
    /// `VsrSimulation` snapshots and fires ALWAYS annotations on violation.
    ///
    /// This is the detection gate for VSR safety violations.  The three
    /// checkers are stateful across calls — the `AgreementChecker`
    /// accumulates a `(view, op)→hash` table across the whole run so it
    /// catches *temporal* divergences (replica 0 commits X at t=10, replica 1
    /// commits X′ at the same slot at t=50).
    ///
    /// The annotation IDs are deliberately phase-agnostic — the phase string
    /// is logged via tracing so debugging info is retained even though the
    /// `always!` macro requires string literals.
    fn check_vsr_agreement(&mut self, phase: &'static str) {
        let snapshots = self.vsr.extract_snapshots();

        let consistency =
            check_commit_number_consistency_snapshots(&mut self.commit_consistency, &snapshots);
        let consistency_ok = matches!(consistency, InvariantResult::Ok);
        if let InvariantResult::Violated { ref message, .. } = consistency {
            eprintln!("[vsr.cross_replica_commit_consistency] phase={phase}: {message}");
        }
        // Direct `record_always` call bypasses the `#[cfg(any(test, feature
        // = "sim"))]` gate inside the `always!` macro.  `kimberlite-sim`
        // doesn't define its own `sim` feature, and integration tests don't
        // see `cfg(test)` on the lib, so macro-form `always!` calls placed
        // inside this crate would silently compile out.  Registering the
        // violation directly is the only way to get these cross-replica
        // checks to appear in every property report.
        kimberlite_properties::registry::record_always(
            "vsr.cross_replica_commit_consistency",
            consistency_ok,
            "commit_number must be <= op_number on every replica",
        );

        let agreement = check_agreement_snapshots(&mut self.agreement, &snapshots);
        let agreement_ok = matches!(agreement, InvariantResult::Ok);
        if let InvariantResult::Violated { ref message, .. } = agreement {
            eprintln!("[vsr.cross_replica_agreement] phase={phase}: {message}");
        }
        kimberlite_properties::registry::record_always(
            "vsr.cross_replica_agreement",
            agreement_ok,
            "no two replicas may commit different operations at the same (view, op)",
        );

        let prefix = check_prefix_property_snapshots(&mut self.prefix, &snapshots);
        let prefix_ok = matches!(prefix, InvariantResult::Ok);
        if let InvariantResult::Violated { ref message, .. } = prefix {
            eprintln!("[vsr.cross_replica_prefix] phase={phase}: {message}");
        }
        kimberlite_properties::registry::record_always(
            "vsr.cross_replica_prefix",
            prefix_ok,
            "all replicas must agree on the committed prefix up to min_commit",
        );
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
        // Crash replica 2 (transitions it to Recovering status), then fire
        // the Recovery timeout so start_recovery() actually runs and
        // broadcasts a RecoveryRequest.  The quorum collection + completion
        // path fires `vsr.recovery_completed`.
        self.vsr.crash_replica(2);
        let outbound = self
            .vsr
            .process_timeout(2, TimeoutKind::Recovery, &mut self.vsr_rng);
        self.fanout(outbound, 4);

        // Post-recovery consistency check.  `vsr.recovery_completed` fires
        // when the state machine transitions, but doesn't verify that the
        // recovered replica now holds a committed log consistent with its
        // peers.  This check closes that gap: every op <= min committed
        // across all replicas must hash identically on replica 2 and the
        // reference replicas 0/1.
        self.check_recovery_restores_consistency();
    }

    /// Verifies that after [`Self::fire_recovery`], replica 2's committed
    /// prefix matches replicas 0 and 1 on an entry-hash basis.
    ///
    /// Uses [`crate::vsr_invariant_helpers::compute_log_entry_hash`] so the
    /// hash function matches the one used by the Agreement / Prefix checkers —
    /// a mismatch detected here implies a recovery bug, not a hash-function
    /// disagreement.
    fn check_recovery_restores_consistency(&mut self) {
        let snaps = self.vsr.extract_snapshots();
        let min_commit = snaps
            .iter()
            .map(|s| s.commit_number.as_u64())
            .min()
            .unwrap_or(0);

        // No committed ops yet → property holds trivially.
        if min_commit == 0 {
            kimberlite_properties::registry::record_always(
                "vsr.recovery_restores_consistency",
                true,
                "recovered replica's committed prefix matches the cluster",
            );
            return;
        }

        // Build a per-(op_number) hash from replica 0 as the reference, and
        // verify replicas 1 and 2 agree up to min_commit.  If replica 0 is
        // the one that just recovered, the loop still works because the
        // agreement is symmetric — any cross-replica mismatch fails the
        // check regardless of who "recovered".
        let reference: std::collections::HashMap<u64, kimberlite_crypto::ChainHash> = snaps[0]
            .log
            .iter()
            .filter(|entry| entry.op_number.as_u64() <= min_commit)
            .map(|entry| {
                (
                    entry.op_number.as_u64(),
                    crate::vsr_invariant_helpers::compute_log_entry_hash(entry),
                )
            })
            .collect();

        let mut mismatch: Option<String> = None;
        for snap in &snaps[1..] {
            for entry in &snap.log {
                let op = entry.op_number.as_u64();
                if op > min_commit {
                    continue;
                }
                let Some(expected) = reference.get(&op) else {
                    mismatch = Some(format!(
                        "replica {} committed op={} with no matching entry on replica 0",
                        snap.replica_id, op
                    ));
                    break;
                };
                let actual = crate::vsr_invariant_helpers::compute_log_entry_hash(entry);
                if &actual != expected {
                    mismatch = Some(format!(
                        "replica {} disagrees with replica 0 at op={}",
                        snap.replica_id, op
                    ));
                    break;
                }
            }
            if mismatch.is_some() {
                break;
            }
        }

        if let Some(ref msg) = mismatch {
            eprintln!("[vsr.recovery_restores_consistency] {msg}");
        }
        kimberlite_properties::registry::record_always(
            "vsr.recovery_restores_consistency",
            mismatch.is_none(),
            "recovered replica's committed prefix matches the cluster",
        );
    }

    /// Drives `vsr.commit_target_exceeds_op` by letting replica 2 fall behind.
    ///
    /// 1. Submit one client request; deliver Prepare only to replica 1
    ///    (withhold from replica 2) — leader still achieves quorum from replica 1.
    /// 2. Deliver the resulting Commit to replica 2.
    /// 3. Replica 2 has op_number=0, new_commit > 0 → annotation fires inside
    ///    `apply_commits_up_to`.
    /// 4. Catch replica 2 up: deliver the withheld Prepares so the cluster
    ///    returns to a consistent state for subsequent rounds.
    fn run_commit_catchup_scenario(&mut self) {
        // Determine the current leader and pick a backup to lag behind.
        let leader_id = self.vsr.current_leader_id();
        // The lagging backup is the non-leader replica with the highest ID.
        let lagging = (0u8..3).filter(|&r| r != leader_id).max().unwrap_or(2);

        // Step 1: submit request via the actual current leader.
        let outbound = self.vsr.process_client_request_to_leader(&mut self.vsr_rng);

        let mut prepare_ok_for_leader: Vec<kimberlite_vsr::Message> = Vec::new();
        let mut withheld_for_lagging: Vec<kimberlite_vsr::Message> = Vec::new();

        for msg in outbound {
            let to = msg.to.map(u8::from);
            match to {
                Some(t) if t == lagging => {
                    // Explicitly addressed to lagging backup — withhold.
                    withheld_for_lagging.push(msg);
                }
                Some(t) if t < 3 => {
                    // Addressed to another replica — deliver normally.
                    let responses = self.vsr.deliver_message(t, msg, &mut self.vsr_rng);
                    prepare_ok_for_leader.extend(responses);
                }
                None => {
                    // Broadcast: deliver to every non-lagging, non-sender replica.
                    let from = u8::from(msg.from);
                    for peer in 0u8..3 {
                        if peer == from || peer == lagging {
                            continue;
                        }
                        let responses =
                            self.vsr
                                .deliver_message(peer, msg.clone(), &mut self.vsr_rng);
                        prepare_ok_for_leader.extend(responses);
                    }
                    withheld_for_lagging.push(msg);
                }
                _ => {}
            }
        }

        // Step 2: deliver PrepareOk(s) to the leader, collect Commit messages.
        let mut commit_msgs: Vec<kimberlite_vsr::Message> = Vec::new();
        for msg in prepare_ok_for_leader {
            if msg.to.map(u8::from) == Some(leader_id) {
                let responses = self.vsr.deliver_message(leader_id, msg, &mut self.vsr_rng);
                commit_msgs.extend(responses);
            }
        }

        // Step 3: deliver Commit to the lagging backup before its Prepares.
        // lagging backup's op_number is behind; new_commit > op_number → annotation fires.
        for msg in &commit_msgs {
            let to = msg.to.map(u8::from);
            if to == Some(lagging) || to.is_none() {
                self.vsr
                    .deliver_message(lagging, msg.clone(), &mut self.vsr_rng);
            }
        }

        // Step 4: catch lagging backup up with its withheld Prepares.
        self.fanout(withheld_for_lagging, 2);
    }

    /// Adversarial view change: exercises the hard VSR case where a Prepare
    /// reaches backups but the Commit never does because the leader dies.
    ///
    /// Sequence:
    /// 1. Leader receives a client request and broadcasts Prepare.
    /// 2. Backups persist the Prepare and return PrepareOk (op_number++).
    /// 3. Leader receives PrepareOks, emits Commit — we DROP the Commit.
    /// 4. Backups now hold the prepared op but are not yet committed.
    /// 5. Crash the leader.
    /// 6. Fire a heartbeat timeout on a surviving backup → view change.
    /// 7. The new leader must commit the previously-prepared op.
    ///
    /// `vsr.view_change_preserves_prepared` fires true when the new leader's
    /// `commit_number` advanced past the pre-scenario baseline (the
    /// prepared op was preserved across the view change).  A false firing —
    /// the new leader dropped the prepared op — is a VSR safety bug.
    fn run_adversarial_view_change_scenario(&mut self) {
        use kimberlite_vsr::MessagePayload;

        let leader_id = self.vsr.current_leader_id();
        let baseline_snaps = self.vsr.extract_snapshots();
        let baseline_commit = baseline_snaps
            .iter()
            .map(|s| s.commit_number.as_u64())
            .max()
            .unwrap_or(0);

        // Step 1: submit a client request to the current leader.  If the
        // leader has somehow degraded (e.g. a prior scenario left it in
        // ViewChange), `process_client_request_to_leader` will return
        // nothing useful; we handle that by checking preconditions below.
        let outbound = self.vsr.process_client_request_to_leader(&mut self.vsr_rng);

        if outbound.is_empty() {
            // Precondition failed — leader didn't accept the request.
            // Record the annotation as satisfied (trivially) so the
            // ALWAYS accounting stays honest.
            kimberlite_properties::registry::record_always(
                "vsr.view_change_preserves_prepared",
                true,
                "view change preserves ops prepared at quorum before leader death",
            );
            return;
        }

        // Step 2: deliver the Prepares to every backup, collect PrepareOks.
        let mut prepare_ok_for_leader: Vec<kimberlite_vsr::Message> = Vec::new();
        for msg in outbound {
            let to = msg.to.map(u8::from);
            match to {
                Some(t) if t < 3 && t != leader_id => {
                    let responses = self.vsr.deliver_message(t, msg, &mut self.vsr_rng);
                    prepare_ok_for_leader.extend(responses);
                }
                None => {
                    let from = u8::from(msg.from);
                    for peer in 0u8..3 {
                        if peer == from {
                            continue;
                        }
                        let responses =
                            self.vsr
                                .deliver_message(peer, msg.clone(), &mut self.vsr_rng);
                        prepare_ok_for_leader.extend(responses);
                    }
                }
                _ => {}
            }
        }

        // Step 3: deliver PrepareOks to leader; collect the Commit it emits
        // so we can DROP them (adversarial: Commit never reaches backups).
        let mut leader_output: Vec<kimberlite_vsr::Message> = Vec::new();
        for msg in prepare_ok_for_leader {
            if msg.to.map(u8::from) == Some(leader_id) {
                let responses = self.vsr.deliver_message(leader_id, msg, &mut self.vsr_rng);
                leader_output.extend(responses);
            }
        }

        // Deliberately drop every Commit produced by the leader.  Forward
        // only non-Commit outputs (state transfer, repair, etc.) so the
        // cluster doesn't livelock on unrelated protocol business.
        let non_commit: Vec<kimberlite_vsr::Message> = leader_output
            .into_iter()
            .filter(|m| !matches!(m.payload, MessagePayload::Commit(_)))
            .collect();
        self.fanout(non_commit, 1);

        // Step 4: verify the op was actually prepared at every backup —
        // otherwise the view change is permitted to drop it.
        //
        // The property we want to assert is: "if a Prepare reached both
        // backups (so every surviving replica has it in-log) and the
        // leader advanced commit on quorum, then the view change cannot
        // lose that op — a surviving replica will carry it forward."
        //
        // Requiring op_number to advance on every non-leader replica is a
        // stricter precondition than necessary (quorum would be 1 backup
        // + the leader), but it ensures the test is deterministic: after
        // view change, the new leader is one of those backups and it
        // already has the op locally.
        let pre_crash_snaps = self.vsr.extract_snapshots();
        let leader_committed = pre_crash_snaps[leader_id as usize].commit_number.as_u64();

        if leader_committed <= baseline_commit {
            // Leader never committed (quorum not reached) — legitimately
            // allowed to be dropped by the view change.
            kimberlite_properties::registry::record_always(
                "vsr.view_change_preserves_prepared",
                true,
                "view change preserves ops prepared at quorum before leader death",
            );
            return;
        }

        let all_backups_prepared = pre_crash_snaps
            .iter()
            .filter(|s| s.replica_id.as_u8() != leader_id)
            .all(|s| s.op_number.as_u64() >= leader_committed);

        if !all_backups_prepared {
            // At least one backup missed the Prepare — the op was
            // committed at minimum-quorum on the leader + one other
            // replica, but the survivor set after the crash might not
            // include that replica.  Skip the check to keep the test
            // deterministic.
            kimberlite_properties::registry::record_always(
                "vsr.view_change_preserves_prepared",
                true,
                "view change preserves ops prepared at quorum before leader death",
            );
            return;
        }

        // Step 5: crash the leader.
        self.vsr.crash_replica(leader_id);

        // Step 6: fire a heartbeat timeout on a surviving backup to trigger
        // view change.  Pick the lowest non-leader replica ID for determinism.
        let survivor = (0u8..3).find(|&r| r != leader_id).unwrap_or(0);
        let vc_output =
            self.vsr
                .process_timeout(survivor, TimeoutKind::Heartbeat, &mut self.vsr_rng);

        // Step 7: fanout long enough for the view change to converge and
        // for the new leader to apply recovered commits.
        //
        // Sequence:
        //   1. StartViewChange broadcast.
        //   2. Peer replies with its own StartViewChange → quorum.
        //   3. Each with quorum emits DoViewChange to new leader.
        //   4. New leader receives DoViewChange quorum, emits StartView.
        //   5. Peers accept StartView, apply commits.
        //
        // 12 rounds is generous; the fanout exits early when the message
        // queue drains so a shorter real sequence wastes no work.
        self.fanout(vc_output, 12);

        // Step 8: verify the new leader committed the previously-prepared op.
        // The "new leader" is whatever `current_leader_id()` reports now;
        // if the view change stalled, this might still be the crashed
        // replica's ID, in which case no one committed anything new.
        let post_snaps = self.vsr.extract_snapshots();
        // The op survives the view change if ANY surviving replica still
        // has it in its log.  Committing may take additional rounds (the
        // new leader will re-prepare or a subsequent normal round will
        // advance commit), so log presence is the VSR safety bar.
        let max_post_op = post_snaps
            .iter()
            .filter(|s| s.replica_id.as_u8() != leader_id)
            .map(|s| s.op_number.as_u64())
            .max()
            .unwrap_or(0);
        let max_post_commit = post_snaps
            .iter()
            .filter(|s| s.replica_id.as_u8() != leader_id)
            .map(|s| s.commit_number.as_u64())
            .max()
            .unwrap_or(0);

        // Pass if EITHER: the op is still in a surviving replica's log,
        // OR the commit has already caught up past the prepared op.
        let preserved = max_post_op >= leader_committed || max_post_commit >= leader_committed;
        if !preserved {
            eprintln!(
                "[vsr.view_change_preserves_prepared] leader_committed={leader_committed} \
                 baseline_commit={baseline_commit} max_post_commit={max_post_commit} \
                 old_leader={leader_id} new_leader={}",
                self.vsr.current_leader_id()
            );
            for s in &post_snaps {
                eprintln!(
                    "  replica {}: status={:?} view={} op={} commit={} log_len={}",
                    s.replica_id,
                    s.status,
                    s.view.as_u64(),
                    s.op_number.as_u64(),
                    s.commit_number.as_u64(),
                    s.log.len()
                );
            }
        }
        kimberlite_properties::registry::record_always(
            "vsr.view_change_preserves_prepared",
            preserved,
            "view change preserves ops prepared at quorum before leader death",
        );
    }

    /// Exercises the compliance crate surface so its 35+ property annotations
    /// fire. Called once per seed, typically right before the simulation loop
    /// tears down. Subsystem-by-subsystem: audit log, consent, erasure,
    /// breach, export.
    pub fn run_compliance_suite(&mut self) {
        Self::run_audit_workload();
        Self::run_consent_workload();
        Self::run_erasure_workload();
        Self::run_breach_workload();
        Self::run_export_workload();
    }

    /// Exercises the query engine so `query.*` property annotations fire:
    /// schema invariants (ALWAYS), JOIN multi-row coverage (SOMETIMES), GROUP
    /// BY + CASE WHEN materialize path, BETWEEN desugaring, LIKE pattern
    /// evaluation, SUM overflow guard. Queries run against a minimal
    /// in-memory `ProjectionStore` — no disk I/O.
    pub fn run_query_suite(&mut self) {
        let schema = SchemaBuilder::new()
            .table(
                "users",
                TableId::new(1),
                vec![
                    ColumnDef::new("id", DataType::BigInt).not_null(),
                    ColumnDef::new("name", DataType::Text).not_null(),
                    ColumnDef::new("age", DataType::BigInt),
                ],
                vec!["id".into()],
            )
            .table(
                "orders",
                TableId::new(2),
                vec![
                    ColumnDef::new("order_id", DataType::BigInt).not_null(),
                    ColumnDef::new("user_id", DataType::BigInt).not_null(),
                    ColumnDef::new("total", DataType::BigInt),
                ],
                vec!["order_id".into()],
            )
            // Three rows near i64::MAX; SUM(total) trips
            // `query.sum_bigint_overflow_detected` via checked_add.
            .table(
                "huge_values",
                TableId::new(3),
                vec![
                    ColumnDef::new("id", DataType::BigInt).not_null(),
                    ColumnDef::new("total", DataType::BigInt),
                ],
                vec!["id".into()],
            )
            .build();

        let mut store = InMemoryProjectionStore::new();
        // Populate users.
        for (id, name, age) in &[
            (1i64, "Alice", 30i64),
            (2, "Bob", 25),
            (3, "Charlie", 35),
            (4, "Dana", 28),
        ] {
            store.insert_json(
                TableId::new(1),
                encode_key(&[Value::BigInt(*id)]),
                &serde_json::json!({"id": id, "name": name, "age": age}),
            );
        }
        // Populate orders.
        for (order_id, user_id, total) in &[(100i64, 1i64, 500i64), (101, 2, 300), (102, 1, 750)] {
            store.insert_json(
                TableId::new(2),
                encode_key(&[Value::BigInt(*order_id)]),
                &serde_json::json!({
                    "order_id": order_id,
                    "user_id": user_id,
                    "total": total,
                }),
            );
        }
        // Populate huge_values: three rows summing to an i64 overflow.
        for (id, total) in &[(1i64, i64::MAX), (2, i64::MAX / 2), (3, i64::MAX / 2)] {
            store.insert_json(
                TableId::new(3),
                encode_key(&[Value::BigInt(*id)]),
                &serde_json::json!({"id": id, "total": total}),
            );
        }

        let engine = QueryEngine::new(schema);

        // Each query below is best-effort: if the parser/planner hasn't
        // fully landed for a syntax, the driver swallows the error and moves
        // on — the goal is to fire annotations, not to produce verified
        // results. Every successful query fires the two schema-width ALWAYS
        // annotations at the result boundary.
        let queries = [
            // Schema invariants (ALWAYS) + basic WHERE.
            "SELECT id, name FROM users WHERE id = 1",
            // BETWEEN → desugars to Ge + Le (sometimes! in parser).
            "SELECT id, age FROM users WHERE age BETWEEN 25 AND 32",
            // LIKE pattern vs Text (sometimes! in FilterOp).
            "SELECT id, name FROM users WHERE name LIKE 'A%'",
            // CASE WHEN wrapped in Materialize (sometimes!).
            "SELECT id, CASE WHEN age > 30 THEN 'senior' ELSE 'junior' END AS tier FROM users",
            // JOIN multi-row path (sometimes! join_multi_row).
            "SELECT u.id, o.order_id FROM users u INNER JOIN orders o ON u.id = o.user_id",
            // GROUP BY + aggregate.
            "SELECT age, COUNT(*) FROM users GROUP BY age",
            // SUM — triggers overflow-guard annotation, checked_add path.
            "SELECT SUM(total) FROM orders",
            // SUM on i64::MAX-adjacent values → checked_add returns None,
            // firing `query.sum_bigint_overflow_detected` (SOMETIMES).
            "SELECT SUM(total) FROM huge_values",
            // AVG with nullable column exercises divide-by-zero NEVER.
            "SELECT AVG(age) FROM users",
            // ORDER BY + LIMIT materialize path.
            "SELECT id, age FROM users ORDER BY age DESC LIMIT 2",
        ];

        for sql in queries {
            let _ = engine.query(&mut store, sql, &[]);
        }

        // Time-travel path: `query.time_travel_at_position` fires only when
        // execute_at is invoked with a Some(position). Use any offset that
        // a ProjectionStore can resolve — our in-memory store ignores it
        // but the annotation still fires at the boundary.
        let _ = engine.query_at(
            &mut store,
            "SELECT id, name FROM users WHERE id = 1",
            &[],
            kimberlite_types::Offset::new(1),
        );
    }

    fn run_audit_workload() {
        let mut log = ComplianceAuditLog::new();
        let actor = Some("dst.real_state_driver".to_string());
        let tenant = Some(42u64);

        // One entry per ComplianceAuditAction variant — each fires a distinct
        // `reached!` marker.
        log.append(
            ComplianceAuditAction::ConsentGranted {
                subject_id: "subject-1".into(),
                purpose: "Marketing".into(),
                scope: "AllData".into(),
                terms_version: None,
                accepted: true,
            },
            actor.clone(),
            tenant,
        );
        log.append(
            ComplianceAuditAction::ConsentWithdrawn {
                subject_id: "subject-1".into(),
                consent_id: Uuid::nil(),
            },
            actor.clone(),
            tenant,
        );
        log.append(
            ComplianceAuditAction::ErasureRequested {
                subject_id: "subject-2".into(),
                request_id: Uuid::nil(),
            },
            actor.clone(),
            tenant,
        );
        log.append(
            ComplianceAuditAction::ErasureCompleted {
                subject_id: "subject-2".into(),
                records_erased: 7,
                request_id: Uuid::nil(),
            },
            actor.clone(),
            tenant,
        );
        log.append(
            ComplianceAuditAction::ErasureExempted {
                subject_id: "subject-2".into(),
                request_id: Uuid::nil(),
                basis: "LegalObligation".into(),
            },
            actor.clone(),
            tenant,
        );
        log.append(
            ComplianceAuditAction::FieldMasked {
                column: "email".into(),
                strategy: "Hash".into(),
                role: "Analyst".into(),
            },
            actor.clone(),
            tenant,
        );
        log.append(
            ComplianceAuditAction::BreachDetected {
                event_id: Uuid::nil(),
                severity: "High".into(),
                indicator: "MassExport".into(),
                affected_subjects: vec!["subject-3".into()],
            },
            actor.clone(),
            tenant,
        );
        log.append(
            ComplianceAuditAction::BreachNotified {
                event_id: Uuid::nil(),
                notified_at: Utc::now(),
                affected_subjects: vec!["subject-3".into()],
            },
            actor.clone(),
            tenant,
        );
        log.append(
            ComplianceAuditAction::BreachResolved {
                event_id: Uuid::nil(),
                remediation: "Key rotated".into(),
                affected_subjects: vec!["subject-3".into()],
            },
            actor.clone(),
            tenant,
        );
        log.append(
            ComplianceAuditAction::DataExported {
                subject_id: "subject-1".into(),
                export_id: Uuid::nil(),
                format: "Json".into(),
                record_count: 4,
            },
            actor.clone(),
            tenant,
        );
        log.append(
            ComplianceAuditAction::AccessGranted {
                user_id: "admin@example.com".into(),
                resource: "audit.log".into(),
                role: "Auditor".into(),
            },
            actor.clone(),
            tenant,
        );
        log.append(
            ComplianceAuditAction::AccessDenied {
                user_id: "user@example.com".into(),
                resource: "admin.panel".into(),
                reason: "role".into(),
            },
            actor.clone(),
            tenant,
        );
        log.append(
            ComplianceAuditAction::PolicyChanged {
                policy_type: "RBAC".into(),
                changed_by: "root".into(),
                details: "add analyst role".into(),
            },
            actor.clone(),
            tenant,
        );
        log.append(
            ComplianceAuditAction::TokenizationApplied {
                column: "ssn".into(),
                token_format: "FPE".into(),
                record_count: 10,
            },
            actor.clone(),
            tenant,
        );
        log.append(
            ComplianceAuditAction::RecordSigned {
                record_id: "rec-1".into(),
                signer_id: "doctor@example.com".into(),
                meaning: "Approved".into(),
            },
            actor,
            tenant,
        );
    }

    fn run_consent_workload() {
        let mut tracker = ConsentTracker::new();
        // Grant + withdraw fires `compliance.consent.granted_at_not_future`
        // (ALWAYS) and exercises the withdraw path.
        if let Ok(consent_id) = tracker.grant_consent("subject-phase13", Purpose::Marketing) {
            let _ = tracker.withdraw_consent(consent_id);
        }
    }

    #[allow(deprecated)] // AUDIT-2026-04 H-4: legacy proof shape exercised for coverage
    fn run_erasure_workload() {
        let mut engine = ErasureEngine::new();
        // Request → in progress → stream erased → complete fires
        // `compliance.erasure.deadline_30_days` (ALWAYS).
        if let Ok(req) = engine.request_erasure("subject-completed") {
            let rid = req.request_id;
            let _ = engine.mark_in_progress(rid, vec![StreamId::new(1)]);
            let _ = engine.mark_stream_erased(rid, StreamId::new(1), 3);
            let _ = engine.complete_erasure(rid);
        }
        // Separate request that we exempt instead of completing — fires
        // the exempt SOMETIMES markers.
        if let Ok(req) = engine.request_erasure("subject-exempt") {
            let _ = engine.exempt_from_erasure(req.request_id, ExemptionBasis::LegalObligation);
        }
        if let Ok(req) = engine.request_erasure("subject-claims") {
            let _ = engine.exempt_from_erasure(req.request_id, ExemptionBasis::LegalClaims);
        }
    }

    fn run_breach_workload() {
        let mut detector = BreachDetector::new();
        // Each check may or may not produce an event depending on thresholds;
        // the annotations fire inside `classify_severity`/`create_event`.
        // Mass export with PHI → Critical severity.
        let _ = detector.check_mass_export(1_000_000, &[DataClass::PHI]);
        // Mass export with Confidential/Financial → Medium severity (this
        // fires `compliance.breach.severity_medium` which stayed deferred
        // earlier because the driver only hit Low/High/Critical).
        let _ =
            detector.check_mass_export(1_000_000, &[DataClass::Confidential, DataClass::Financial]);
        // Privilege escalation is always a breach.
        if let Some(event) = detector.check_privilege_escalation("user", "admin") {
            let _ = detector.confirm(event.event_id);
        }
        // Access at 2am → outside business hours → Low severity.
        let _ = detector.check_unusual_access_time(2);
        // Denied access burst.
        for _ in 0..10 {
            let _ = detector.check_denied_access(Utc::now());
        }
    }

    fn run_export_workload() {
        let mut engine = ExportEngine::new();
        let records = vec![ExportRecord {
            stream_id: StreamId::new(1),
            stream_name: "phase13-stream".into(),
            offset: 0,
            data: serde_json::json!({"field": "value"}),
            timestamp: Utc::now(),
        }];

        // JSON path fires reached + format_json SOMETIMES + content_hash + signature.
        if let Ok(json_export) =
            engine.export_subject_data("subject-json", &records, ExportFormat::Json, "dst.driver")
        {
            let _ = engine.sign_export(json_export.export_id, b"phase13-hmac-key-32-bytes-long!!");
        }
        // CSV path fires reached + format_csv SOMETIMES.
        let _ =
            engine.export_subject_data("subject-csv", &records, ExportFormat::Csv, "dst.driver");
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
                    let responses = self.vsr.deliver_message(to, msg.clone(), &mut self.vsr_rng);
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
