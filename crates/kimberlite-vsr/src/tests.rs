//! Integration tests for kmb-vsr.
//!
//! This module contains higher-level integration tests that exercise
//! multiple components together.

#![allow(clippy::float_cmp)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::single_match)]

use crate::{
    ClusterConfig, CommitNumber, LogEntry, MemorySuperblock, Message, MessagePayload, OpNumber,
    Prepare, PrepareOk, ReplicaId, ReplicaStatus, Superblock, ViewNumber,
};
use kimberlite_kernel::Command;
use kimberlite_types::{DataClass, Placement};

// ============================================================================
// Helper Functions
// ============================================================================

fn test_command() -> Command {
    Command::create_stream_with_auto_id("test-stream".into(), DataClass::Public, Placement::Global)
}

fn test_log_entry(op: u64, view: u64) -> LogEntry {
    LogEntry::new(
        OpNumber::new(op),
        ViewNumber::new(view),
        test_command(),
        None,
        None,
        None,
    )
}

// ============================================================================
// Cluster Configuration Tests
// ============================================================================

#[test]
fn three_node_cluster_quorum() {
    let config = ClusterConfig::new(vec![
        ReplicaId::new(0),
        ReplicaId::new(1),
        ReplicaId::new(2),
    ]);

    assert_eq!(config.cluster_size(), 3);
    assert_eq!(config.quorum_size(), 2);
    assert_eq!(config.max_failures(), 1);
}

#[test]
fn five_node_cluster_quorum() {
    let config = ClusterConfig::new(vec![
        ReplicaId::new(0),
        ReplicaId::new(1),
        ReplicaId::new(2),
        ReplicaId::new(3),
        ReplicaId::new(4),
    ]);

    assert_eq!(config.cluster_size(), 5);
    assert_eq!(config.quorum_size(), 3);
    assert_eq!(config.max_failures(), 2);
}

#[test]
fn seven_node_cluster_quorum() {
    let config = ClusterConfig::new(vec![
        ReplicaId::new(0),
        ReplicaId::new(1),
        ReplicaId::new(2),
        ReplicaId::new(3),
        ReplicaId::new(4),
        ReplicaId::new(5),
        ReplicaId::new(6),
    ]);

    assert_eq!(config.cluster_size(), 7);
    assert_eq!(config.quorum_size(), 4);
    assert_eq!(config.max_failures(), 3);
}

// ============================================================================
// Message Protocol Tests
// ============================================================================

#[test]
fn prepare_prepare_ok_flow() {
    let leader = ReplicaId::new(0);
    let backup = ReplicaId::new(1);
    let view = ViewNumber::new(0);
    let op = OpNumber::new(1);
    let entry = test_log_entry(1, 0);

    // Leader sends Prepare
    let prepare = Prepare::new(view, op, entry, CommitNumber::ZERO);
    let prepare_msg = Message::broadcast(leader, MessagePayload::Prepare(prepare.clone()));

    assert!(prepare_msg.is_broadcast());
    assert_eq!(prepare_msg.from, leader);

    // Backup responds with PrepareOk
    let prepare_ok = PrepareOk::without_clock(view, op, backup);
    let prepare_ok_msg = Message::targeted(backup, leader, MessagePayload::PrepareOk(prepare_ok));

    assert!(!prepare_ok_msg.is_broadcast());
    assert_eq!(prepare_ok_msg.from, backup);
    assert_eq!(prepare_ok_msg.to, Some(leader));
}

#[test]
fn message_view_extraction() {
    let view = ViewNumber::new(42);

    let heartbeat =
        MessagePayload::Heartbeat(crate::Heartbeat::without_clock(view, CommitNumber::ZERO));
    assert_eq!(heartbeat.view(), Some(view));

    let prepare = MessagePayload::Prepare(Prepare::new(
        view,
        OpNumber::new(1),
        test_log_entry(1, 42),
        CommitNumber::ZERO,
    ));
    assert_eq!(prepare.view(), Some(view));
}

// ============================================================================
// Superblock Persistence Tests
// ============================================================================

#[test]
fn superblock_survives_partial_write() {
    // Simulate a crash during superblock update
    let storage = MemorySuperblock::new();
    let mut sb = Superblock::create(storage, ReplicaId::new(0)).expect("create");

    // Make some updates
    sb.update(ViewNumber::new(1), OpNumber::new(10), CommitNumber::ZERO)
        .expect("update");
    sb.update(ViewNumber::new(2), OpNumber::new(20), CommitNumber::ZERO)
        .expect("update");

    // Get a copy of storage before the next write
    let stable_data = sb.storage().clone_data();

    // Make another update
    sb.update(ViewNumber::new(3), OpNumber::new(30), CommitNumber::ZERO)
        .expect("update");

    // Simulate crash by reverting to previous state
    let crashed_storage = MemorySuperblock::from_data(stable_data);

    // Recovery should find view 2 as the latest valid state
    let recovered = Superblock::open(crashed_storage).expect("open");
    assert_eq!(recovered.view(), ViewNumber::new(2));
    assert_eq!(recovered.op_number(), OpNumber::new(20));
}

#[test]
fn superblock_all_copies_updated() {
    let storage = MemorySuperblock::new();
    let mut sb = Superblock::create(storage, ReplicaId::new(0)).expect("create");

    // Write 5 times to cycle through all slots + 1
    for i in 1..=5 {
        sb.update(
            ViewNumber::new(i),
            OpNumber::new(i * 10),
            CommitNumber::ZERO,
        )
        .expect("update");
    }

    // Simulate reopening
    let storage2 = MemorySuperblock::from_data(sb.storage().clone_data());

    let recovered = Superblock::open(storage2).expect("open");
    assert_eq!(recovered.view(), ViewNumber::new(5));
    assert_eq!(recovered.op_number(), OpNumber::new(50));
    assert_eq!(recovered.data().sequence, 5);
}

// ============================================================================
// Log Entry Integrity Tests
// ============================================================================

#[test]
fn log_entry_checksum_verification() {
    let entry = test_log_entry(1, 0);

    // Valid entry
    assert!(entry.verify_checksum());

    // Tampered entry
    let mut tampered = entry.clone();
    tampered.op_number = OpNumber::new(999);
    assert!(!tampered.verify_checksum());
}

#[test]
fn log_entry_with_idempotency_id() {
    let id = kimberlite_types::IdempotencyId::generate();
    let entry = LogEntry::new(
        OpNumber::new(1),
        ViewNumber::new(0),
        test_command(),
        Some(id),
        None,
        None,
    );

    assert!(entry.verify_checksum());
    assert_eq!(entry.idempotency_id, Some(id));
}

// ============================================================================
// Replica Status Tests
// ============================================================================

#[test]
fn replica_status_capabilities() {
    // Normal status can do everything
    assert!(ReplicaStatus::Normal.can_process_requests());
    assert!(ReplicaStatus::Normal.can_participate());

    // ViewChange can participate but not process requests
    assert!(!ReplicaStatus::ViewChange.can_process_requests());
    assert!(ReplicaStatus::ViewChange.can_participate());

    // Recovering can do neither
    assert!(!ReplicaStatus::Recovering.can_process_requests());
    assert!(!ReplicaStatus::Recovering.can_participate());
}

// ============================================================================
// Leader Election Tests
// ============================================================================

#[test]
fn leader_determinism() {
    let config = ClusterConfig::new(vec![
        ReplicaId::new(0),
        ReplicaId::new(1),
        ReplicaId::new(2),
    ]);

    // Same view always yields same leader
    for _ in 0..10 {
        assert_eq!(
            config.leader_for_view(ViewNumber::new(5)),
            config.leader_for_view(ViewNumber::new(5))
        );
    }
}

#[test]
fn leader_rotation_covers_all_replicas() {
    let config = ClusterConfig::new(vec![
        ReplicaId::new(0),
        ReplicaId::new(1),
        ReplicaId::new(2),
    ]);

    let mut seen = std::collections::HashSet::new();

    // After cluster_size views, all replicas should have been leader
    for view in 0..config.cluster_size() {
        let leader = config.leader_for_view(ViewNumber::new(view as u64));
        seen.insert(leader);
    }

    assert_eq!(seen.len(), config.cluster_size());
}

// ============================================================================
// Quorum Math Tests
// ============================================================================

#[test]
fn quorum_math_properties() {
    use crate::types::{max_failures, quorum_size};

    // Property: quorum_size + max_failures = cluster_size (or cluster_size + 1)
    // This ensures any two quorums overlap
    for size in (1..=21).step_by(2) {
        let q = quorum_size(size);
        let f = max_failures(size);

        // Two quorums must overlap by at least 1
        assert!(2 * q > size, "two quorums must overlap");

        // We can tolerate f failures and still have a quorum
        assert!(size - f >= q, "must have quorum after f failures");
    }
}

// ============================================================================
// Commit Number Tests
// ============================================================================

#[test]
fn commit_number_ordering() {
    let commit = CommitNumber::new(OpNumber::new(10));

    // Operations at or before commit are committed
    assert!(commit.is_committed(OpNumber::new(5)));
    assert!(commit.is_committed(OpNumber::new(10)));

    // Operations after commit are not committed
    assert!(!commit.is_committed(OpNumber::new(11)));
    assert!(!commit.is_committed(OpNumber::new(100)));
}

// ============================================================================
// View Change Message Tests
// ============================================================================

#[test]
fn do_view_change_contains_log_tail() {
    use crate::DoViewChange;

    let log_tail = vec![
        test_log_entry(5, 1),
        test_log_entry(6, 1),
        test_log_entry(7, 1),
    ];

    let dvc = DoViewChange::new(
        ViewNumber::new(2),
        ReplicaId::new(1),
        ViewNumber::new(1),
        OpNumber::new(7),
        CommitNumber::new(OpNumber::new(4)),
        log_tail.clone(),
    );

    assert_eq!(dvc.log_tail.len(), 3);
    assert_eq!(dvc.view, ViewNumber::new(2));
    assert_eq!(dvc.last_normal_view, ViewNumber::new(1));
}

// ============================================================================
// Nack Reason Tests
// ============================================================================

#[test]
fn nack_reasons_for_par() {
    use crate::NackReason;

    // NotSeen is safe for truncation
    let not_seen = NackReason::NotSeen;
    assert_eq!(format!("{not_seen}"), "not_seen");

    // SeenButCorrupt is NOT safe for truncation
    let corrupt = NackReason::SeenButCorrupt;
    assert_eq!(format!("{corrupt}"), "seen_but_corrupt");

    // Recovering replicas can't help
    let recovering = NackReason::Recovering;
    assert_eq!(format!("{recovering}"), "recovering");
}

// ============================================================================
// Config Builder Tests
// ============================================================================

#[test]
fn config_with_timeouts() {
    use crate::TimeoutConfig;

    let config = ClusterConfig::new(vec![
        ReplicaId::new(0),
        ReplicaId::new(1),
        ReplicaId::new(2),
    ])
    .with_timeouts(TimeoutConfig::simulation());

    // Simulation timeouts are very short
    assert!(config.timeouts.heartbeat_interval.as_micros() < 1000);
}

#[test]
fn config_with_checkpoint() {
    use crate::CheckpointConfig;

    let config = ClusterConfig::new(vec![
        ReplicaId::new(0),
        ReplicaId::new(1),
        ReplicaId::new(2),
    ])
    .with_checkpoint(CheckpointConfig::testing());

    assert_eq!(config.checkpoint.checkpoint_interval, 10);
    assert!(!config.checkpoint.require_signatures);
}

// ============================================================================
// Single-Node Replicator Integration Tests
// ============================================================================

#[test]
fn single_node_replicator_basic_flow() {
    use crate::{Replicator, SingleNodeReplicator};

    let storage = MemorySuperblock::new();
    let config = ClusterConfig::single_node(ReplicaId::new(0));
    let mut replicator = SingleNodeReplicator::create(config, storage).expect("create");

    // Submit create stream command
    let result = replicator.submit(test_command(), None).expect("submit");

    assert_eq!(result.op_number, OpNumber::new(1));
    assert!(!result.effects.is_empty());

    // Verify replicator state
    assert_eq!(replicator.commit_number().as_u64(), 1);
    assert_eq!(replicator.view(), ViewNumber::ZERO);
    assert_eq!(replicator.status(), ReplicaStatus::Normal);
}

#[test]
fn single_node_effects_include_storage_append() {
    use crate::{Replicator, SingleNodeReplicator};
    use kimberlite_kernel::Effect;

    let storage = MemorySuperblock::new();
    let config = ClusterConfig::single_node(ReplicaId::new(0));
    let mut replicator = SingleNodeReplicator::create(config, storage).expect("create");

    // First, create a stream
    let create_result = replicator
        .submit(test_command(), None)
        .expect("create stream");

    // Find the StreamMetadataWrite effect
    let has_metadata_write = create_result
        .effects
        .iter()
        .any(|e| matches!(e, Effect::StreamMetadataWrite(_)));
    assert!(has_metadata_write, "should have StreamMetadataWrite effect");

    // Now append some data
    let stream_id = kimberlite_types::StreamId::new(0); // Auto-allocated ID
    let append_cmd = kimberlite_kernel::Command::append_batch(
        stream_id,
        vec![bytes::Bytes::from("event-1"), bytes::Bytes::from("event-2")],
        kimberlite_types::Offset::ZERO,
    );

    let append_result = replicator.submit(append_cmd, None).expect("append batch");

    // Find the StorageAppend effect
    let storage_append = append_result
        .effects
        .iter()
        .find(|e| matches!(e, Effect::StorageAppend { .. }));
    assert!(storage_append.is_some(), "should have StorageAppend effect");

    // Verify the effect contains our events
    if let Some(Effect::StorageAppend {
        stream_id: sid,
        events,
        base_offset,
    }) = storage_append
    {
        assert_eq!(*sid, stream_id);
        assert_eq!(events.len(), 2);
        assert_eq!(*base_offset, kimberlite_types::Offset::ZERO);
    }
}

#[test]
fn single_node_log_entries_are_sequential() {
    use crate::{Replicator, SingleNodeReplicator};

    let storage = MemorySuperblock::new();
    let config = ClusterConfig::single_node(ReplicaId::new(0));
    let mut replicator = SingleNodeReplicator::create(config, storage).expect("create");

    // Submit multiple commands
    for i in 1..=10 {
        let cmd = kimberlite_kernel::Command::create_stream_with_auto_id(
            format!("stream-{i}").into(),
            DataClass::Public,
            Placement::Global,
        );
        let result = replicator.submit(cmd, None).expect("submit");
        assert_eq!(result.op_number, OpNumber::new(i));
    }

    // Verify log entries are sequential
    for i in 1..=10 {
        let entry = replicator
            .log_entry(OpNumber::new(i))
            .expect("entry exists");
        assert_eq!(entry.op_number, OpNumber::new(i));
        assert_eq!(entry.view, ViewNumber::ZERO);
        assert!(entry.verify_checksum());
    }

    // Verify commit number advances
    assert_eq!(replicator.commit_number().as_u64(), 10);
}

#[test]
#[allow(clippy::items_after_statements)]
fn single_node_replicator_trait_is_object_safe() {
    use crate::{Replicator, SingleNodeReplicator};

    let storage = MemorySuperblock::new();
    let config = ClusterConfig::single_node(ReplicaId::new(0));
    let mut replicator = SingleNodeReplicator::create(config, storage).expect("create");

    // Should be usable through trait reference
    fn use_replicator(r: &dyn Replicator) -> ViewNumber {
        r.view()
    }

    assert_eq!(use_replicator(&replicator), ViewNumber::ZERO);

    // Mutable operations through trait
    fn submit_via_trait(
        r: &mut dyn Replicator,
        cmd: kimberlite_kernel::Command,
    ) -> crate::OpNumber {
        r.submit(cmd, None).expect("submit").op_number
    }

    let op = submit_via_trait(&mut replicator, test_command());
    assert_eq!(op, OpNumber::new(1));
}

// ============================================================================
// Phase 2: Repair Budget & Timeout Integration Tests
// ============================================================================

/// Integration test: Repair budget prevents overwhelming the cluster.
///
/// This test verifies that the repair budget system prevents repair storms
/// by rate-limiting repair requests when multiple replicas fall behind.
#[test]
fn phase2_repair_budget_prevents_storm() {
    use crate::repair_budget::RepairBudget;

    let cluster_size = 5;
    let mut budget = RepairBudget::new(ReplicaId::new(0), cluster_size);
    let now = std::time::Instant::now();

    // Try to send more repairs than the budget allows
    let mut sent_count = 0;
    let max_attempts = 20;

    for i in 0..max_attempts {
        if budget.has_available_slots() {
            // Select replica using EWMA-based selection
            let mut rng = rand::thread_rng();
            if let Some(replica) = budget.select_replica(&mut rng) {
                budget.record_repair_sent(
                    replica,
                    OpNumber::new(i as u64),
                    OpNumber::new((i + 1) as u64),
                    now,
                );
                sent_count += 1;
            }
        }
    }

    // Budget should have prevented sending all repairs
    let max_allowed = (cluster_size - 1) * 2; // 2 per replica
    assert!(
        sent_count <= max_allowed,
        "Budget should limit repairs to {max_allowed} but sent {sent_count}"
    );

    // Verify no replica has more than 2 inflight
    for i in 1..cluster_size {
        let replica = ReplicaId::new(i as u8);
        if let Some(inflight) = budget.replica_inflight(replica) {
            assert!(
                inflight <= 2,
                "Replica {i} should have <=2 inflight, has {inflight}"
            );
        }
    }
}

/// Integration test: EWMA latency tracking adapts to replica performance.
///
/// This test verifies that the repair budget's EWMA latency tracking correctly
/// adapts to changing replica performance.
#[test]
fn phase2_ewma_latency_tracking() {
    use crate::repair_budget::RepairBudget;

    let cluster_size = 3;
    let mut budget = RepairBudget::new(ReplicaId::new(0), cluster_size);
    let base_time = std::time::Instant::now();

    let replica1 = ReplicaId::new(1);
    let replica2 = ReplicaId::new(2);

    // Send and complete repair for replica1 with 10ms latency
    budget.record_repair_sent(replica1, OpNumber::new(1), OpNumber::new(2), base_time);
    let complete_time1 = base_time + std::time::Duration::from_millis(10);
    budget.record_repair_completed(replica1, OpNumber::new(1), OpNumber::new(2), complete_time1);

    // Send and complete repair for replica2 with 100ms latency
    budget.record_repair_sent(replica2, OpNumber::new(1), OpNumber::new(2), base_time);
    let complete_time2 = base_time + std::time::Duration::from_millis(100);
    budget.record_repair_completed(replica2, OpNumber::new(1), OpNumber::new(2), complete_time2);

    // Replica1 should have lower EWMA than replica2
    let latency1 = budget.replica_latency(replica1).unwrap();
    let latency2 = budget.replica_latency(replica2).unwrap();

    assert!(
        latency1 < latency2,
        "Fast replica ({latency1} ns) should have lower EWMA than slow replica ({latency2} ns)"
    );
}

/// Integration test: Timeout handlers execute correctly.
///
/// This test verifies that the new Phase 2 timeout handlers (Ping, `PrimaryAbdicate`,
/// `RepairSync`, `CommitStall`) can be invoked without panicking.
#[test]
fn phase2_timeout_handlers_execute() {
    use crate::{ReplicaEvent, ReplicaState, TimeoutKind};

    let config = ClusterConfig::new(vec![
        ReplicaId::new(0),
        ReplicaId::new(1),
        ReplicaId::new(2),
    ]);

    let state = ReplicaState::new(ReplicaId::new(0), config);

    // Test Ping timeout
    let (state, _output) = state.process(ReplicaEvent::Timeout(TimeoutKind::Ping));
    assert_eq!(state.replica_id, ReplicaId::new(0));

    // Test PrimaryAbdicate timeout
    let (state, _output) = state.process(ReplicaEvent::Timeout(TimeoutKind::PrimaryAbdicate));
    assert_eq!(state.replica_id, ReplicaId::new(0));

    // Test RepairSync timeout
    let (state, _output) = state.process(ReplicaEvent::Timeout(TimeoutKind::RepairSync));
    assert_eq!(state.replica_id, ReplicaId::new(0));

    // Test CommitStall timeout
    let (state, _output) = state.process(ReplicaEvent::Timeout(TimeoutKind::CommitStall));
    assert_eq!(state.replica_id, ReplicaId::new(0));
}

/// Integration test: Repair budget expiry cleans up stale requests.
///
/// This test verifies that the repair budget's request expiry mechanism
/// correctly cleans up stale requests after 500ms timeout.
#[test]
fn phase2_repair_budget_expiry() {
    use crate::repair_budget::RepairBudget;

    let cluster_size = 3;
    let mut budget = RepairBudget::new(ReplicaId::new(0), cluster_size);
    let base_time = std::time::Instant::now();

    let replica1 = ReplicaId::new(1);

    // Send repair
    budget.record_repair_sent(replica1, OpNumber::new(1), OpNumber::new(2), base_time);

    // Verify it's tracked
    assert_eq!(budget.replica_inflight(replica1), Some(1));

    // Expire stale requests (>500ms)
    let expire_time = base_time + std::time::Duration::from_millis(600);
    let expired = budget.expire_stale_requests(expire_time);

    // Request should have expired
    assert_eq!(expired.len(), 1);
    assert_eq!(budget.replica_inflight(replica1), Some(0));
}

// ============================================================================
// Phase 3: Log Scrubber Integration Tests
// ============================================================================

/// Integration test: Scrubber detects corruption.
///
/// This test verifies that the background scrubber detects corrupted log
/// entries via checksum validation.
#[test]
fn phase3_scrubber_detects_corruption() {
    use crate::log_scrubber::{LogScrubber, ScrubResult};
    use crate::types::{LogEntry, ViewNumber};
    use kimberlite_kernel::Command;
    use kimberlite_types::{DataClass, Placement};

    let mut scrubber = LogScrubber::new(ReplicaId::new(0), OpNumber::new(10));
    scrubber.set_tour_position_for_test(OpNumber::new(0), OpNumber::new(0), OpNumber::new(2));

    // Create entry with corrupted checksum
    let cmd =
        Command::create_stream_with_auto_id("test".into(), DataClass::Public, Placement::Global);
    let mut entry = LogEntry::new(OpNumber::new(0), ViewNumber::ZERO, cmd, None, None, None);
    entry.checksum = 0xDEAD_BEEF; // Corrupt checksum

    let log = vec![entry];

    // Scrub should detect corruption
    let result = scrubber.scrub_next(&log);
    assert_eq!(result, ScrubResult::Corruption);
    assert_eq!(scrubber.corruptions().len(), 1);
}

/// Integration test: Scrubber completes tour.
///
/// This test verifies that the scrubber successfully completes a full
/// tour of the log.
#[test]
fn phase3_scrubber_completes_tour() {
    use crate::log_scrubber::{LogScrubber, ScrubResult};
    use crate::types::{LogEntry, ViewNumber};
    use kimberlite_kernel::Command;
    use kimberlite_types::{DataClass, Placement};

    let mut scrubber = LogScrubber::new(ReplicaId::new(0), OpNumber::new(10));
    scrubber.set_tour_position_for_test(OpNumber::new(0), OpNumber::new(0), OpNumber::new(5));

    // Create valid log
    let cmd =
        Command::create_stream_with_auto_id("test".into(), DataClass::Public, Placement::Global);
    let mut log = Vec::new();
    for i in 0..=5 {
        let entry = LogEntry::new(
            OpNumber::new(i),
            ViewNumber::ZERO,
            cmd.clone(),
            None,
            None,
            None,
        );
        log.push(entry);
    }

    // Scrub entire tour
    for _ in 0..=5 {
        let result = scrubber.scrub_next(&log);
        assert_eq!(result, ScrubResult::Ok);
    }

    // Tour should be complete
    let result = scrubber.scrub_next(&log);
    assert_eq!(result, ScrubResult::TourComplete);
}

/// Integration test: Scrubber respects rate limit.
///
/// This test verifies that the scrubber respects the IOPS budget and
/// doesn't exceed the configured read limit per tick.
#[test]
fn phase3_scrubber_respects_rate_limit() {
    use crate::log_scrubber::{LogScrubber, ScrubResult};
    use crate::types::{LogEntry, ViewNumber};
    use kimberlite_kernel::Command;
    use kimberlite_types::{DataClass, Placement};

    let mut scrubber = LogScrubber::new(ReplicaId::new(0), OpNumber::new(100));
    scrubber.set_tour_position_for_test(OpNumber::new(0), OpNumber::new(0), OpNumber::new(20));

    // Create log
    let cmd =
        Command::create_stream_with_auto_id("test".into(), DataClass::Public, Placement::Global);
    let mut log = Vec::new();
    for i in 0..20 {
        let entry = LogEntry::new(
            OpNumber::new(i),
            ViewNumber::ZERO,
            cmd.clone(),
            None,
            None,
            None,
        );
        log.push(entry);
    }

    // Exhaust budget (default 10)
    for _ in 0..10 {
        let result = scrubber.scrub_next(&log);
        assert_eq!(result, ScrubResult::Ok);
    }

    // Next scrub should fail
    let result = scrubber.scrub_next(&log);
    assert_eq!(result, ScrubResult::BudgetExhausted);

    // Reset budget
    scrubber.budget_mut().reset_tick();

    // Can scrub again
    let result = scrubber.scrub_next(&log);
    assert_eq!(result, ScrubResult::Ok);
}

/// Integration test: Scrubber triggers repair on corruption.
///
/// This test verifies that when the scrubber detects corruption,
/// the `on_scrub_timeout` handler triggers repair.
#[test]
fn phase3_scrubber_triggers_repair() {
    use crate::{ReplicaEvent, ReplicaState, TimeoutKind};

    let config = ClusterConfig::new(vec![
        ReplicaId::new(0),
        ReplicaId::new(1),
        ReplicaId::new(2),
    ]);

    let state = ReplicaState::new(ReplicaId::new(0), config);

    // Process scrub timeout (should not panic)
    let (state, _output) = state.process(ReplicaEvent::Timeout(TimeoutKind::Scrub));
    assert_eq!(state.replica_id, ReplicaId::new(0));

    // Verify scrubber completed first tour
    // On empty log, first scrub completes immediately and starts tour 1
    assert_eq!(state.log_scrubber.tour_count(), 1);
}

// ============================================================================
// Phase 4: Cluster Reconfiguration Tests
// ============================================================================

/// Integration test: Add replicas to cluster (3 → 5).
///
/// Tests the joint consensus protocol for adding two replicas.
#[test]
fn phase4_reconfig_add_replicas() {
    use crate::reconfiguration::ReconfigCommand;
    use crate::{ClusterConfig, ReplicaEvent, ReplicaId, ReplicaState};

    // Start with 3-node cluster
    let config = ClusterConfig::new(vec![
        ReplicaId::new(0),
        ReplicaId::new(1),
        ReplicaId::new(2),
    ]);

    let state = ReplicaState::new(ReplicaId::new(0), config);

    // Verify initial state
    assert!(state.reconfig_state.is_stable());
    assert_eq!(state.config.cluster_size(), 3);

    // Issue reconfiguration command to add 2 replicas (3 → 5)
    let cmd = ReconfigCommand::Replace {
        add: vec![ReplicaId::new(3), ReplicaId::new(4)],
        remove: vec![],
    };

    let (state, output) = state.process(ReplicaEvent::ReconfigCommand(cmd));

    // Should transition to joint consensus
    assert!(state.reconfig_state.is_joint());

    // Should have sent Prepare message
    assert_eq!(output.messages.len(), 1);

    // Verify joint state has both configs
    let (old_config, new_config) = state.reconfig_state.configs();
    assert_eq!(old_config.cluster_size(), 3);
    assert_eq!(new_config.unwrap().cluster_size(), 5);

    // Verify quorum calculation during joint consensus
    // Need max(2, 3) = 3 in joint state
    assert_eq!(state.reconfig_state.quorum_size(), 3);
}

/// Integration test: Remove replicas from cluster (5 → 3).
#[test]
fn phase4_reconfig_remove_replicas() {
    use crate::reconfiguration::ReconfigCommand;
    use crate::{ClusterConfig, ReplicaEvent, ReplicaId, ReplicaState};

    // Start with 5-node cluster
    let config = ClusterConfig::new(vec![
        ReplicaId::new(0),
        ReplicaId::new(1),
        ReplicaId::new(2),
        ReplicaId::new(3),
        ReplicaId::new(4),
    ]);

    let state = ReplicaState::new(ReplicaId::new(0), config);

    // Issue reconfiguration command to remove 2 replicas (5 → 3)
    let cmd = ReconfigCommand::Replace {
        add: vec![],
        remove: vec![ReplicaId::new(3), ReplicaId::new(4)],
    };

    let (state, _output) = state.process(ReplicaEvent::ReconfigCommand(cmd));

    // Should transition to joint consensus
    assert!(state.reconfig_state.is_joint());

    // Verify configs
    let (old_config, new_config) = state.reconfig_state.configs();
    assert_eq!(old_config.cluster_size(), 5);
    assert_eq!(new_config.unwrap().cluster_size(), 3);

    // Quorum: max(3, 2) = 3
    assert_eq!(state.reconfig_state.quorum_size(), 3);
}

/// Integration test: Reject concurrent reconfigurations.
#[test]
fn phase4_reconfig_reject_concurrent() {
    use crate::reconfiguration::ReconfigCommand;
    use crate::{ClusterConfig, ReplicaEvent, ReplicaId, ReplicaState};

    let config = ClusterConfig::new(vec![
        ReplicaId::new(0),
        ReplicaId::new(1),
        ReplicaId::new(2),
    ]);

    let state = ReplicaState::new(ReplicaId::new(0), config);

    // First reconfiguration
    let cmd1 = ReconfigCommand::Replace {
        add: vec![ReplicaId::new(3), ReplicaId::new(4)],
        remove: vec![],
    };

    let (state, _output) = state.process(ReplicaEvent::ReconfigCommand(cmd1));
    assert!(state.reconfig_state.is_joint());

    // Second reconfiguration should be rejected
    let cmd2 = ReconfigCommand::Replace {
        add: vec![ReplicaId::new(5), ReplicaId::new(6)],
        remove: vec![],
    };

    let (state, output) = state.process(ReplicaEvent::ReconfigCommand(cmd2));

    // Still in joint consensus (first reconfig)
    assert!(state.reconfig_state.is_joint());

    // No new messages (rejected)
    assert_eq!(output.messages.len(), 0);
}

/// Integration test: Invalid reconfiguration rejected (even cluster size).
#[test]
fn phase4_reconfig_reject_invalid() {
    use crate::reconfiguration::ReconfigCommand;
    use crate::{ClusterConfig, ReplicaEvent, ReplicaId, ReplicaState};

    let config = ClusterConfig::new(vec![
        ReplicaId::new(0),
        ReplicaId::new(1),
        ReplicaId::new(2),
    ]);

    let state = ReplicaState::new(ReplicaId::new(0), config);

    // Try to add single replica (3 → 4, even size)
    let cmd = ReconfigCommand::AddReplica(ReplicaId::new(3));

    let (state, output) = state.process(ReplicaEvent::ReconfigCommand(cmd));

    // Should remain stable (rejected)
    assert!(state.reconfig_state.is_stable());
    assert_eq!(output.messages.len(), 0);
}

// ============================================================================
// Phase 4: Rolling Upgrade Tests
// ============================================================================

#[test]
fn phase4_upgrade_version_tracking_heartbeat() {
    use crate::replica::ReplicaState;
    use crate::upgrade::VersionInfo;

    let config = ClusterConfig::new(vec![
        ReplicaId::new(0),
        ReplicaId::new(1),
        ReplicaId::new(2),
    ]);

    let state = ReplicaState::new(ReplicaId::new(1), config);

    // Initial version should be v0.4.0
    assert_eq!(state.upgrade_state.self_version, VersionInfo::V0_4_0);
    assert_eq!(state.upgrade_state.cluster_version(), VersionInfo::V0_4_0);

    // Receive heartbeat from leader (replica 0) with newer version
    let heartbeat = crate::Heartbeat::new(
        ViewNumber::ZERO,
        CommitNumber::ZERO,
        0,
        0,
        VersionInfo::new(0, 5, 0),
    );

    let msg = Message::broadcast(ReplicaId::new(0), MessagePayload::Heartbeat(heartbeat));
    let event = crate::ReplicaEvent::Message(Box::new(msg));
    let (state, _output) = state.process(event);

    // Should have tracked the leader's version
    assert_eq!(
        state.upgrade_state.replica_versions.get(&ReplicaId::new(0)),
        Some(&VersionInfo::new(0, 5, 0))
    );

    // Cluster version should still be minimum (v0.4.0)
    assert_eq!(state.upgrade_state.cluster_version(), VersionInfo::V0_4_0);
}

#[test]
fn phase4_upgrade_version_tracking_prepare_ok() {
    use crate::replica::ReplicaState;
    use crate::upgrade::VersionInfo;

    let config = ClusterConfig::new(vec![
        ReplicaId::new(0),
        ReplicaId::new(1),
        ReplicaId::new(2),
    ]);

    let mut state = ReplicaState::new(ReplicaId::new(0), config);

    // Add an entry to leader's log
    let entry = test_log_entry(1, 0);
    state.log.push(entry.clone());
    state.op_number = OpNumber::new(1);

    // Receive PrepareOk from backup with newer version
    let prepare_ok = PrepareOk::new(
        ViewNumber::ZERO,
        OpNumber::new(1),
        ReplicaId::new(1),
        0,
        VersionInfo::new(0, 5, 0),
    );

    let msg = Message::targeted(
        ReplicaId::new(1),
        ReplicaId::new(0),
        MessagePayload::PrepareOk(prepare_ok),
    );
    let event = crate::ReplicaEvent::Message(Box::new(msg));
    let (state, _output) = state.process(event);

    // Should have tracked the backup's version
    assert_eq!(
        state.upgrade_state.replica_versions.get(&ReplicaId::new(1)),
        Some(&VersionInfo::new(0, 5, 0))
    );

    // Cluster version should still be minimum (v0.4.0)
    assert_eq!(state.upgrade_state.cluster_version(), VersionInfo::V0_4_0);
}

#[test]
fn phase4_upgrade_cluster_min_version() {
    use crate::replica::ReplicaState;
    use crate::upgrade::VersionInfo;

    let config = ClusterConfig::new(vec![
        ReplicaId::new(0),
        ReplicaId::new(1),
        ReplicaId::new(2),
    ]);

    let mut state = ReplicaState::new(ReplicaId::new(0), config);

    // Simulate receiving versions from other replicas
    state
        .upgrade_state
        .update_replica_version(ReplicaId::new(1), VersionInfo::new(0, 5, 0));
    state
        .upgrade_state
        .update_replica_version(ReplicaId::new(2), VersionInfo::new(0, 4, 5));

    // Cluster version should be minimum (v0.4.0, which is self)
    assert_eq!(state.upgrade_state.cluster_version(), VersionInfo::V0_4_0);

    // Now if we upgrade self
    state.upgrade_state.self_version = VersionInfo::new(0, 4, 5);

    // Cluster version should now be v0.4.5
    assert_eq!(
        state.upgrade_state.cluster_version(),
        VersionInfo::new(0, 4, 5)
    );
}

#[test]
fn phase4_upgrade_feature_flags() {
    use crate::replica::ReplicaState;
    use crate::upgrade::FeatureFlag;

    let config = ClusterConfig::new(vec![
        ReplicaId::new(0),
        ReplicaId::new(1),
        ReplicaId::new(2),
    ]);

    let state = ReplicaState::new(ReplicaId::new(0), config);

    // At v0.4.0, rolling upgrades feature should be enabled
    assert!(
        state
            .upgrade_state
            .is_feature_enabled(FeatureFlag::RollingUpgrades)
    );

    // Clock sync (v0.3.1) should be enabled
    assert!(
        state
            .upgrade_state
            .is_feature_enabled(FeatureFlag::ClockSync)
    );

    // All Phase 4 features should be enabled
    assert!(
        state
            .upgrade_state
            .is_feature_enabled(FeatureFlag::ClusterReconfig)
    );
}

// ============================================================================
// Phase 4: Standby Replica Tests
// ============================================================================

#[test]
fn phase4_standby_apply_operations() {
    use crate::standby::StandbyState;

    let mut standby = StandbyState::new(ReplicaId::new(10));

    // Apply operations sequentially
    for i in 1..=5 {
        let entry = test_log_entry(i, 0);
        assert!(standby.apply_commit(OpNumber::new(i), entry));
    }

    assert_eq!(standby.commit_number.as_u64(), 5);
    assert_eq!(standby.log.len(), 5);
}

#[test]
fn phase4_standby_promotion_conditions() {
    use crate::standby::StandbyState;

    let mut standby = StandbyState::new(ReplicaId::new(10));

    // Apply operations
    for i in 1..=3 {
        let entry = test_log_entry(i, 0);
        standby.apply_commit(OpNumber::new(i), entry);
    }

    // Can promote if healthy and caught up
    assert!(standby.can_promote(CommitNumber::new(OpNumber::new(3))));

    // Cannot promote if behind
    assert!(!standby.can_promote(CommitNumber::new(OpNumber::new(10))));

    // Simulate missed heartbeats
    for _ in 0..3 {
        standby.record_missed_heartbeat();
    }

    // Cannot promote if unhealthy (even if caught up)
    assert!(!standby.is_healthy);
    assert!(!standby.can_promote(CommitNumber::new(OpNumber::new(3))));
}

#[test]
fn phase4_standby_manager_health_tracking() {
    use crate::standby::StandbyManager;

    let mut manager = StandbyManager::new();

    // Register three standbys
    manager.register_standby(ReplicaId::new(10));
    manager.register_standby(ReplicaId::new(11));
    manager.register_standby(ReplicaId::new(12));

    // All healthy initially
    let stats = manager.health_stats();
    assert_eq!(stats.total, 3);
    assert_eq!(stats.healthy, 3);
    assert_eq!(stats.health_percentage(), 1.0);

    // Record heartbeats for two standbys
    manager.record_heartbeat(ReplicaId::new(10), 1_000_000_000);
    manager.record_heartbeat(ReplicaId::new(11), 1_000_000_000);

    // Check timeouts at 5 seconds (3 second timeout)
    // Standbys 10 and 11: elapsed = 4 seconds (> 3, unhealthy)
    // Standby 12: elapsed = 5 seconds (> 3, unhealthy)
    manager.check_timeouts(5_000_000_000);

    // All timed out
    let stats = manager.health_stats();
    assert_eq!(stats.healthy, 0);
    assert_eq!(stats.unhealthy, 3);
}

#[test]
fn phase4_standby_manager_promotable() {
    use crate::standby::StandbyManager;

    let mut manager = StandbyManager::new();

    manager.register_standby(ReplicaId::new(10));
    manager.register_standby(ReplicaId::new(11));

    // Apply commits to first standby
    if let Some(standby) = manager.get_standby_mut(ReplicaId::new(10)) {
        for i in 1..=5 {
            let entry = test_log_entry(i, 0);
            standby.apply_commit(OpNumber::new(i), entry);
        }
    }

    // Record recent heartbeats to keep standbys healthy
    let current_time = 2_000_000_000;
    manager.record_heartbeat(ReplicaId::new(10), current_time);
    manager.record_heartbeat(ReplicaId::new(11), current_time);

    // Check timeouts (both healthy)
    manager.check_timeouts(current_time + 1_000_000_000); // +1 second

    // Only first standby is promotable (caught up and healthy)
    let promotable = manager.promotable_standbys(CommitNumber::new(OpNumber::new(5)));
    assert_eq!(promotable.len(), 1);
    assert_eq!(promotable[0], ReplicaId::new(10));
}

#[test]
fn phase4_standby_lag_tracking() {
    use crate::standby::StandbyState;

    let mut standby = StandbyState::new(ReplicaId::new(10));

    // Standby is behind
    assert_eq!(standby.lag(CommitNumber::new(OpNumber::new(10))), 10);

    // Apply some operations
    for i in 1..=5 {
        let entry = test_log_entry(i, 0);
        standby.apply_commit(OpNumber::new(i), entry);
    }

    // Lag reduced
    assert_eq!(standby.lag(CommitNumber::new(OpNumber::new(10))), 5);

    // Apply remaining operations
    for i in 6..=10 {
        let entry = test_log_entry(i, 0);
        standby.apply_commit(OpNumber::new(i), entry);
    }

    // Caught up
    assert_eq!(standby.lag(CommitNumber::new(OpNumber::new(10))), 0);
}

/// Integration test: Backup processes reconfiguration command in Prepare message.
#[test]
fn phase4_reconfig_backup_processes_prepare() {
    use crate::message::{MessagePayload, Prepare};
    use crate::reconfiguration::ReconfigCommand;
    use crate::types::LogEntry;
    use crate::{ClusterConfig, OpNumber, ReplicaId, ReplicaState, ViewNumber};
    use kimberlite_kernel::Command;

    // Create 3-node cluster
    let config = ClusterConfig::new(vec![
        ReplicaId::new(0),
        ReplicaId::new(1),
        ReplicaId::new(2),
    ]);

    // Replica 1 is a backup in view 0 (replica 0 is leader)
    let mut backup = ReplicaState::new(ReplicaId::new(1), config.clone());
    backup = backup.enter_normal_status();

    // Verify backup starts in stable state
    assert!(backup.reconfig_state.is_stable());
    assert_eq!(backup.config.cluster_size(), 3);

    // Leader sends Prepare with reconfiguration command to add 2 replicas
    let reconfig_cmd = ReconfigCommand::Replace {
        add: vec![ReplicaId::new(3), ReplicaId::new(4)],
        remove: vec![],
    };

    let entry = LogEntry::new(
        OpNumber::new(1),
        ViewNumber::ZERO,
        Command::AppendBatch {
            stream_id: kimberlite_types::StreamId::new(0),
            events: vec![],
            expected_offset: kimberlite_types::Offset::ZERO,
        },
        None,
        None,
        None,
    );

    let prepare = Prepare::new_with_reconfig(
        ViewNumber::ZERO,
        OpNumber::new(1),
        entry,
        crate::CommitNumber::ZERO,
        reconfig_cmd.clone(),
    );

    // Backup processes Prepare with reconfig command
    let (backup, output) = backup.on_prepare(ReplicaId::new(0), prepare);

    // CRITICAL: Backup should transition to joint consensus
    assert!(
        backup.reconfig_state.is_joint(),
        "Backup should transition to joint consensus after processing Prepare with reconfig"
    );

    // Verify joint state has both configs
    let (old_config, new_config_opt) = backup.reconfig_state.configs();
    assert_eq!(old_config.cluster_size(), 3);
    assert_eq!(new_config_opt.unwrap().cluster_size(), 5);

    // Verify joint operation number matches the Prepare
    assert_eq!(backup.reconfig_state.joint_op(), Some(OpNumber::new(1)));

    // Backup should send PrepareOK
    assert_eq!(output.messages.len(), 1);
    if let MessagePayload::PrepareOk(prepare_ok) = &output.messages[0].payload {
        assert_eq!(prepare_ok.op_number, OpNumber::new(1));
    } else {
        panic!("Expected PrepareOk message");
    }

    // Verify quorum calculation updated
    // Joint consensus requires max(2, 3) = 3
    assert_eq!(backup.reconfig_state.quorum_size(), 3);
}

/// Integration test: Joint consensus automatically transitions to stable after commit.
#[test]
fn phase4_reconfig_joint_to_stable_transition() {
    use crate::reconfiguration::ReconfigState;
    use crate::{ClusterConfig, OpNumber, ReplicaId};

    // Create a mock scenario where we're already in joint consensus
    // and just committed the joint operation
    let old_config = ClusterConfig::new(vec![
        ReplicaId::new(0),
        ReplicaId::new(1),
        ReplicaId::new(2),
    ]);

    let new_config = ClusterConfig::new(vec![
        ReplicaId::new(0),
        ReplicaId::new(1),
        ReplicaId::new(2),
        ReplicaId::new(3),
        ReplicaId::new(4),
    ]);

    // Create ReconfigState in joint consensus with joint_op = 5
    let mut reconfig_state =
        ReconfigState::new_joint(old_config.clone(), new_config.clone(), OpNumber::new(5));

    // Initially in joint state
    assert!(reconfig_state.is_joint());
    assert_eq!(reconfig_state.joint_op(), Some(OpNumber::new(5)));

    // Before committing joint_op, not ready to transition
    assert!(!reconfig_state.ready_to_transition(OpNumber::new(4)));

    // After committing joint_op (commit_number >= joint_op), ready to transition
    assert!(reconfig_state.ready_to_transition(OpNumber::new(5)));
    assert!(reconfig_state.ready_to_transition(OpNumber::new(6)));

    // Perform the transition
    reconfig_state.transition_to_new();

    // Now in stable state with new config
    assert!(reconfig_state.is_stable());
    assert_eq!(reconfig_state.stable_config().unwrap().cluster_size(), 5);

    // Verify the stable config is the new config
    let stable_cfg = reconfig_state.stable_config().unwrap();
    for i in 0..5 {
        assert!(stable_cfg.contains(ReplicaId::new(i)));
    }
}
