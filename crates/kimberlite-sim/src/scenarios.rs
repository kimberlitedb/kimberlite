//! Extended VOPR test scenarios for comprehensive simulation testing.
//!
//! This module provides pre-configured test scenarios that combine various
//! fault injection patterns to test specific correctness properties.

use crate::{
    ByzantineInjector, GrayFailureInjector, NetworkConfig, SimRng, StorageConfig, SwizzleClogger,
};

// ============================================================================
// Scenario Types
// ============================================================================

/// Predefined test scenarios for VOPR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioType {
    /// Baseline: no faults, normal operation
    Baseline,
    /// Swizzle-clogging: intermittent network congestion
    SwizzleClogging,
    /// Gray failures: partial node failures (slow, intermittent, partial)
    GrayFailures,
    /// Multi-tenant isolation: concurrent tenants with fault injection
    MultiTenantIsolation,
    /// Time compression: accelerated time to test long-running scenarios
    TimeCompression,
    /// Combined: all fault types enabled
    Combined,
    /// Byzantine: view change log merge overwrites committed entries (Bug #1)
    ByzantineViewChangeMerge,
    /// Byzantine: commit number desynchronization (Bug #2)
    ByzantineCommitDesync,
    /// Byzantine: inflated commit number in DoViewChange (Bug #3)
    ByzantineInflatedCommit,
    /// Byzantine: invalid entry metadata (Bug #4)
    ByzantineInvalidMetadata,
    /// Byzantine: malicious view change selection (Bug #5)
    ByzantineMaliciousViewChange,
    /// Byzantine: leader selection race condition (Bug #6)
    ByzantineLeaderRace,

    // AUDIT-2026-03 H-1: Complete Byzantine Attack Coverage
    /// Byzantine: Replay old messages from previous view
    ByzantineReplayOldView,
    /// Byzantine: Corrupt message checksums
    ByzantineCorruptChecksums,
    /// Byzantine: Block DoViewChange messages to specific replicas
    ByzantineViewChangeBlocking,
    /// Byzantine: Flood replicas with excessive Prepare messages
    ByzantinePrepareFlood,
    /// Byzantine: Selectively ignore messages from specific replicas
    ByzantineSelectiveSilence,

    // Phase 3A Bug-Specific Scenarios
    /// Byzantine: DoViewChange log_tail length mismatch (Bug 3.1)
    ByzantineDvcTailLengthMismatch,
    /// Byzantine: DoViewChange with identical claims (Bug 3.3)
    ByzantineDvcIdenticalClaims,
    /// Byzantine: Oversized StartView log_tail (Bug 3.4 - DoS)
    ByzantineOversizedStartView,
    /// Byzantine: Invalid repair range (Bug 3.5)
    ByzantineInvalidRepairRange,
    /// Byzantine: Invalid kernel command (Bug 3.2)
    ByzantineInvalidKernelCommand,

    // Corruption Detection Scenarios
    /// Corruption: Random bit flip in log entry
    CorruptionBitFlip,
    /// Corruption: Checksum validation test
    CorruptionChecksumValidation,
    /// Corruption: Silent disk failure
    CorruptionSilentDiskFailure,

    // Recovery & Crash Scenarios
    /// Crash during commit application
    CrashDuringCommit,
    /// Crash during view change
    CrashDuringViewChange,
    /// Recovery with corrupt log
    RecoveryCorruptLog,

    // Gray Failure Scenarios
    /// Gray failure: Slow disk I/O
    GrayFailureSlowDisk,
    /// Gray failure: Intermittent network
    GrayFailureIntermittentNetwork,

    // Race Condition Scenarios
    /// Race: Concurrent view changes
    RaceConcurrentViewChanges,
    /// Race: Commit during DoViewChange
    RaceCommitDuringDvc,

    // Phase 1: Clock Synchronization Scenarios
    /// Clock: Drift detection and tolerance
    ClockDrift,
    /// Clock: Offset exceeds tolerance
    ClockOffsetExceeded,
    /// Clock: NTP-style failures
    ClockNtpFailure,
    /// Clock: Backward jump during partition (monotonicity test)
    ClockBackwardJump,

    // Phase 1: Client Session Scenarios
    /// Client Session: Successive crashes (VRR Bug #1)
    ClientSessionCrash,
    /// Client Session: View change lockout prevention (VRR Bug #2)
    ClientSessionViewChangeLockout,
    /// Client Session: Deterministic eviction
    ClientSessionEviction,

    // Phase 2: Repair Budget & Timeout Scenarios
    /// Repair: Budget prevents repair storms
    RepairBudgetPreventsStorm,
    /// Repair: EWMA-based smart replica selection
    RepairEwmaSelection,
    /// Repair: Sync timeout escalates to state transfer
    RepairSyncTimeout,
    /// Timeout: Primary abdicate when partitioned
    PrimaryAbdicatePartition,
    /// Timeout: Commit stall detection
    CommitStallDetection,
    /// Timeout: Ping heartbeat regular health checks
    PingHeartbeat,
    /// Timeout: Commit message fallback via heartbeat
    CommitMessageFallback,
    /// Timeout: Start view change window prevents split-brain
    StartViewChangeWindow,
    /// Timeout: Comprehensive timeout coverage test
    TimeoutComprehensive,

    // Phase 3: Storage Integrity Scenarios
    /// Scrub: Detects corruption via checksum validation
    ScrubDetectsCorruption,
    /// Scrub: Tour completes within time limit
    ScrubCompletesTour,
    /// Scrub: Rate limiting respects IOPS budget
    ScrubRateLimited,
    /// Scrub: Triggers repair on corruption detection
    ScrubTriggersRepair,

    // Phase 4: Cluster Reconfiguration Scenarios
    /// Reconfig: Add replicas (3 → 5)
    ReconfigAddReplicas,
    /// Reconfig: Remove replicas (5 → 3)
    ReconfigRemoveReplicas,
    /// Reconfig: During network partition
    ReconfigDuringPartition,
    /// Reconfig: View change during joint consensus
    ReconfigDuringViewChange,
    /// Reconfig: Concurrent reconfiguration requests
    ReconfigConcurrentRequests,
    /// Reconfig: Joint quorum validation (both configs)
    ReconfigJointQuorumValidation,

    // Phase 4.2: Rolling Upgrade Scenarios
    /// Upgrade: Gradual rollout (sequential upgrade of replicas)
    UpgradeGradualRollout,
    /// Upgrade: Replica failure during upgrade
    UpgradeWithFailure,
    /// Upgrade: Rollback to previous version
    UpgradeRollback,
    /// Upgrade: Feature flag activation
    UpgradeFeatureActivation,

    // Phase 4.3: Standby Replica Scenarios
    /// Standby: Follows log without participating in quorum
    StandbyFollowsLog,
    /// Standby: Promotion to active replica
    StandbyPromotion,
    /// Standby: Read scaling (multiple standby replicas serve queries)
    StandbyReadScaling,

    // Phase 3.2: RBAC (Role-Based Access Control) Scenarios
    /// RBAC: Unauthorized column access attempt
    RbacUnauthorizedColumnAccess,
    /// RBAC: Role escalation attack prevention
    RbacRoleEscalationAttack,
    /// RBAC: Row-level security enforcement
    RbacRowLevelSecurity,
    /// RBAC: Audit trail completeness
    RbacAuditTrailComplete,
}

impl ScenarioType {
    /// Returns a human-readable name for the scenario.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Baseline => "Baseline (No Faults)",
            Self::SwizzleClogging => "Swizzle-Clogging",
            Self::GrayFailures => "Gray Failures",
            Self::MultiTenantIsolation => "Multi-Tenant Isolation",
            Self::TimeCompression => "Time Compression",
            Self::Combined => "Combined Faults",
            Self::ByzantineViewChangeMerge => "Byzantine: View Change Merge",
            Self::ByzantineCommitDesync => "Byzantine: Commit Desync",
            Self::ByzantineInflatedCommit => "Byzantine: Inflated Commit",
            Self::ByzantineInvalidMetadata => "Byzantine: Invalid Metadata",
            Self::ByzantineMaliciousViewChange => "Byzantine: Malicious View Change",
            Self::ByzantineLeaderRace => "Byzantine: Leader Race",
            Self::ByzantineReplayOldView => "Byzantine: Replay Old View",
            Self::ByzantineCorruptChecksums => "Byzantine: Corrupt Checksums",
            Self::ByzantineViewChangeBlocking => "Byzantine: View Change Blocking",
            Self::ByzantinePrepareFlood => "Byzantine: Prepare Flood",
            Self::ByzantineSelectiveSilence => "Byzantine: Selective Silence",
            Self::ByzantineDvcTailLengthMismatch => "Byzantine: DVC Tail Length Mismatch",
            Self::ByzantineDvcIdenticalClaims => "Byzantine: DVC Identical Claims",
            Self::ByzantineOversizedStartView => "Byzantine: Oversized StartView",
            Self::ByzantineInvalidRepairRange => "Byzantine: Invalid Repair Range",
            Self::ByzantineInvalidKernelCommand => "Byzantine: Invalid Kernel Command",
            Self::CorruptionBitFlip => "Corruption: Bit Flip",
            Self::CorruptionChecksumValidation => "Corruption: Checksum Validation",
            Self::CorruptionSilentDiskFailure => "Corruption: Silent Disk Failure",
            Self::CrashDuringCommit => "Crash: During Commit",
            Self::CrashDuringViewChange => "Crash: During View Change",
            Self::RecoveryCorruptLog => "Recovery: Corrupt Log",
            Self::GrayFailureSlowDisk => "Gray Failure: Slow Disk",
            Self::GrayFailureIntermittentNetwork => "Gray Failure: Intermittent Network",
            Self::RaceConcurrentViewChanges => "Race: Concurrent View Changes",
            Self::RaceCommitDuringDvc => "Race: Commit During DVC",
            Self::ClockDrift => "Clock: Drift Detection",
            Self::ClockOffsetExceeded => "Clock: Offset Exceeded",
            Self::ClockNtpFailure => "Clock: NTP Failure",
            Self::ClockBackwardJump => "Clock: Backward Jump",
            Self::ClientSessionCrash => "Client Session: Crash Recovery",
            Self::ClientSessionViewChangeLockout => "Client Session: View Change Lockout",
            Self::ClientSessionEviction => "Client Session: Eviction",
            Self::RepairBudgetPreventsStorm => "Repair: Budget Prevents Storm",
            Self::RepairEwmaSelection => "Repair: EWMA Selection",
            Self::RepairSyncTimeout => "Repair: Sync Timeout",
            Self::PrimaryAbdicatePartition => "Timeout: Primary Abdicate",
            Self::CommitStallDetection => "Timeout: Commit Stall",
            Self::PingHeartbeat => "Timeout: Ping Heartbeat",
            Self::CommitMessageFallback => "Timeout: Commit Message Fallback",
            Self::StartViewChangeWindow => "Timeout: Start View Change Window",
            Self::TimeoutComprehensive => "Timeout: Comprehensive",
            Self::ScrubDetectsCorruption => "Scrub: Detects Corruption",
            Self::ScrubCompletesTour => "Scrub: Completes Tour",
            Self::ScrubRateLimited => "Scrub: Rate Limited",
            Self::ScrubTriggersRepair => "Scrub: Triggers Repair",
            Self::ReconfigAddReplicas => "Reconfig: Add Replicas",
            Self::ReconfigRemoveReplicas => "Reconfig: Remove Replicas",
            Self::ReconfigDuringPartition => "Reconfig: During Partition",
            Self::ReconfigDuringViewChange => "Reconfig: During View Change",
            Self::ReconfigConcurrentRequests => "Reconfig: Concurrent Requests",
            Self::ReconfigJointQuorumValidation => "Reconfig: Joint Quorum Validation",
            Self::UpgradeGradualRollout => "Upgrade: Gradual Rollout",
            Self::UpgradeWithFailure => "Upgrade: With Failure",
            Self::UpgradeRollback => "Upgrade: Rollback",
            Self::UpgradeFeatureActivation => "Upgrade: Feature Activation",
            Self::StandbyFollowsLog => "Standby: Follows Log",
            Self::StandbyPromotion => "Standby: Promotion",
            Self::StandbyReadScaling => "Standby: Read Scaling",
            Self::RbacUnauthorizedColumnAccess => "RBAC: Unauthorized Column Access",
            Self::RbacRoleEscalationAttack => "RBAC: Role Escalation Attack",
            Self::RbacRowLevelSecurity => "RBAC: Row-Level Security",
            Self::RbacAuditTrailComplete => "RBAC: Audit Trail Complete",
        }
    }

    /// Returns a description of what this scenario tests.
    #[allow(clippy::too_many_lines)]
    pub fn description(&self) -> &'static str {
        match self {
            Self::Baseline => "Normal operation without faults to establish baseline performance",
            Self::SwizzleClogging => "Intermittent network congestion and link flapping",
            Self::GrayFailures => {
                "Partial node failures: slow responses, intermittent errors, read-only nodes"
            }
            Self::MultiTenantIsolation => {
                "Multiple tenants with independent data, testing isolation under faults"
            }
            Self::TimeCompression => "10x accelerated time to test long-running operations",
            Self::Combined => "All fault types enabled simultaneously for stress testing",
            Self::ByzantineViewChangeMerge => {
                "Attack: Force view change after commits, inject conflicting entries (targets vsr_agreement)"
            }
            Self::ByzantineCommitDesync => {
                "Attack: Send StartView with high commit_number but truncated log (targets vsr_prefix_property)"
            }
            Self::ByzantineInflatedCommit => {
                "Attack: Byzantine replica claims impossibly high commit_number (targets vsr_durability)"
            }
            Self::ByzantineInvalidMetadata => {
                "Attack: Send Prepare with mismatched entry metadata (targets vsr_agreement)"
            }
            Self::ByzantineMaliciousViewChange => {
                "Attack: Send DoViewChange with inconsistent log (targets vsr_view_change_safety)"
            }
            Self::ByzantineLeaderRace => {
                "Attack: Create asymmetric partition during leader selection (targets vsr_agreement)"
            }
            Self::ByzantineReplayOldView => {
                "Attack: Re-send messages from previous view to confuse replicas (AUDIT-2026-03 H-1)"
            }
            Self::ByzantineCorruptChecksums => {
                "Attack: Send log entries with invalid checksums (AUDIT-2026-03 H-1)"
            }
            Self::ByzantineViewChangeBlocking => {
                "Attack: Withhold DoViewChange from specific replicas to delay view change (AUDIT-2026-03 H-1)"
            }
            Self::ByzantinePrepareFlood => {
                "Attack: Overwhelm replicas with excessive Prepare messages (AUDIT-2026-03 H-1)"
            }
            Self::ByzantineSelectiveSilence => {
                "Attack: Ignore messages from specific replicas to create asymmetric partitions (AUDIT-2026-03 H-1)"
            }
            Self::ByzantineDvcTailLengthMismatch => {
                "Attack: Send DoViewChange with log_tail length != (op_number - commit_number) (Bug 3.1)"
            }
            Self::ByzantineDvcIdenticalClaims => {
                "Attack: Multiple replicas send DoViewChange with identical (last_normal_view, op_number) (Bug 3.3)"
            }
            Self::ByzantineOversizedStartView => {
                "Attack: Send StartView with millions of entries to exhaust memory (Bug 3.4 - DoS)"
            }
            Self::ByzantineInvalidRepairRange => {
                "Attack: Send RepairRequest with start >= end to confuse replica (Bug 3.5)"
            }
            Self::ByzantineInvalidKernelCommand => {
                "Attack: Send Prepare with command that causes kernel error (Bug 3.2)"
            }
            Self::CorruptionBitFlip => {
                "Test: Random bit flip in log entry, verify checksum detects it"
            }
            Self::CorruptionChecksumValidation => {
                "Test: Corrupt checksum field, verify validation rejects it"
            }
            Self::CorruptionSilentDiskFailure => {
                "Test: Simulate silent disk corruption, verify detection and repair"
            }
            Self::CrashDuringCommit => {
                "Test: Replica crashes mid-commit, verify recovery maintains consistency"
            }
            Self::CrashDuringViewChange => {
                "Test: Replica crashes during view change, verify safe recovery"
            }
            Self::RecoveryCorruptLog => {
                "Test: Replica recovers with corrupt log, verify repair from healthy peers"
            }
            Self::GrayFailureSlowDisk => {
                "Test: Disk I/O randomly slow, verify system maintains liveness"
            }
            Self::GrayFailureIntermittentNetwork => {
                "Test: Network intermittently drops packets, verify eventual consistency"
            }
            Self::RaceConcurrentViewChanges => {
                "Test: Multiple view changes triggered simultaneously, verify single leader emerges"
            }
            Self::RaceCommitDuringDvc => {
                "Test: Commit happens while DoViewChange in progress, verify safety"
            }
            Self::ClockDrift => {
                "Test: Gradual clock drift across replicas, verify tolerance detection within bounds"
            }
            Self::ClockOffsetExceeded => {
                "Test: Clock offset exceeds CLOCK_OFFSET_TOLERANCE_MS (500ms), verify rejection"
            }
            Self::ClockNtpFailure => {
                "Test: Simulate NTP server failure (no clock samples), verify graceful degradation"
            }
            Self::ClockBackwardJump => {
                "Test: Primary partitioned with backward clock jump, verify monotonicity preserved across view change"
            }
            Self::ClientSessionCrash => {
                "Test: Client crash and restart with request number reset, verify no collisions (VRR Bug #1)"
            }
            Self::ClientSessionViewChangeLockout => {
                "Test: Uncommitted requests during view change, verify no client lockout (VRR Bug #2)"
            }
            Self::ClientSessionEviction => {
                "Test: Session eviction when max_sessions exceeded, verify deterministic by timestamp"
            }
            Self::RepairBudgetPreventsStorm => {
                "Test: Multiple lagging replicas, verify repair budget prevents message queue overflow"
            }
            Self::RepairEwmaSelection => {
                "Test: Replicas with varying latency, verify EWMA-based smart replica selection"
            }
            Self::RepairSyncTimeout => {
                "Test: Repair stuck for >100 ops, verify escalation to state transfer"
            }
            Self::PrimaryAbdicatePartition => {
                "Test: Leader partitioned from quorum, verify abdication prevents deadlock"
            }
            Self::CommitStallDetection => {
                "Test: Pipeline growth without commit progress, verify stall detection and backpressure"
            }
            Self::PingHeartbeat => {
                "Test: Ping timeout triggers regular heartbeats, verify network health monitoring"
            }
            Self::CommitMessageFallback => {
                "Test: Commit message delayed/dropped, verify heartbeat fallback notifies backups"
            }
            Self::StartViewChangeWindow => {
                "Test: View change window timeout, verify split-brain prevention via delayed installation"
            }
            Self::TimeoutComprehensive => {
                "Test: All timeout types under various fault conditions, verify complete liveness coverage"
            }
            Self::ScrubDetectsCorruption => {
                "Test: Inject corrupted entry (bad checksum), verify scrubber detects it"
            }
            Self::ScrubCompletesTour => {
                "Test: Scrubber tours entire log within reasonable time (IOPS budget permitting)"
            }
            Self::ScrubRateLimited => {
                "Test: Scrubbing respects IOPS budget (max 10 reads/tick), doesn't impact production"
            }
            Self::ScrubTriggersRepair => {
                "Test: Corruption detection triggers automatic repair to restore data integrity"
            }
            Self::ReconfigAddReplicas => {
                "Test: Joint consensus safely adds replicas (3 → 5) without split-brain"
            }
            Self::ReconfigRemoveReplicas => {
                "Test: Joint consensus safely removes replicas (5 → 3) with quorum preservation"
            }
            Self::ReconfigDuringPartition => {
                "Test: Reconfigurations survive network partitions and view changes"
            }
            Self::ReconfigDuringViewChange => {
                "Test: View change during joint consensus preserves reconfiguration state"
            }
            Self::ReconfigConcurrentRequests => {
                "Test: Concurrent reconfiguration requests are rejected (one at a time)"
            }
            Self::ReconfigJointQuorumValidation => {
                "Test: Joint consensus requires quorum in BOTH old and new configs"
            }
            Self::UpgradeGradualRollout => {
                "Test: Sequential upgrade of replicas without service disruption"
            }
            Self::UpgradeWithFailure => {
                "Test: Replica failure during upgrade, verify cluster remains operational"
            }
            Self::UpgradeRollback => "Test: Rollback to previous version when issues detected",
            Self::UpgradeFeatureActivation => {
                "Test: Features activate only when all replicas upgraded"
            }
            Self::StandbyFollowsLog => {
                "Test: Standby replicas receive log updates but never send PrepareOK (Kani Proof #68)"
            }
            Self::StandbyPromotion => {
                "Test: Standby promotion to active preserves log consistency (Kani Proof #69)"
            }
            Self::StandbyReadScaling => {
                "Test: Multiple standbys serve eventually consistent reads, no quorum impact"
            }
            Self::RbacUnauthorizedColumnAccess => {
                "Test: User attempts to access denied column (e.g., SSN), verify query rewriting filters it out"
            }
            Self::RbacRoleEscalationAttack => {
                "Test: User attempts to escalate from User to Admin role, verify enforcement prevents it"
            }
            Self::RbacRowLevelSecurity => {
                "Test: Multi-tenant query without tenant_id filter, verify WHERE clause injection isolates tenants"
            }
            Self::RbacAuditTrailComplete => {
                "Test: All access attempts (allowed and denied) are logged with role, timestamp, and decision"
            }
        }
    }

    /// Returns all scenario types.
    pub fn all() -> &'static [ScenarioType] {
        &[
            Self::Baseline,
            Self::SwizzleClogging,
            Self::GrayFailures,
            Self::MultiTenantIsolation,
            Self::TimeCompression,
            Self::Combined,
            Self::ByzantineViewChangeMerge,
            Self::ByzantineCommitDesync,
            Self::ByzantineInflatedCommit,
            Self::ByzantineInvalidMetadata,
            Self::ByzantineMaliciousViewChange,
            Self::ByzantineLeaderRace,
            Self::ByzantineReplayOldView,
            Self::ByzantineCorruptChecksums,
            Self::ByzantineViewChangeBlocking,
            Self::ByzantinePrepareFlood,
            Self::ByzantineSelectiveSilence,
            Self::ByzantineDvcTailLengthMismatch,
            Self::ByzantineDvcIdenticalClaims,
            Self::ByzantineOversizedStartView,
            Self::ByzantineInvalidRepairRange,
            Self::ByzantineInvalidKernelCommand,
            Self::CorruptionBitFlip,
            Self::CorruptionChecksumValidation,
            Self::CorruptionSilentDiskFailure,
            Self::CrashDuringCommit,
            Self::CrashDuringViewChange,
            Self::RecoveryCorruptLog,
            Self::GrayFailureSlowDisk,
            Self::GrayFailureIntermittentNetwork,
            Self::RaceConcurrentViewChanges,
            Self::RaceCommitDuringDvc,
            Self::ClockDrift,
            Self::ClockOffsetExceeded,
            Self::ClockNtpFailure,
            Self::ClockBackwardJump,
            Self::ClientSessionCrash,
            Self::ClientSessionViewChangeLockout,
            Self::ClientSessionEviction,
            Self::RepairBudgetPreventsStorm,
            Self::RepairEwmaSelection,
            Self::RepairSyncTimeout,
            Self::PrimaryAbdicatePartition,
            Self::CommitStallDetection,
            Self::PingHeartbeat,
            Self::CommitMessageFallback,
            Self::StartViewChangeWindow,
            Self::TimeoutComprehensive,
            Self::ScrubDetectsCorruption,
            Self::ScrubCompletesTour,
            Self::ScrubRateLimited,
            Self::ScrubTriggersRepair,
            Self::ReconfigAddReplicas,
            Self::ReconfigRemoveReplicas,
            Self::ReconfigDuringPartition,
            Self::ReconfigDuringViewChange,
            Self::ReconfigConcurrentRequests,
            Self::ReconfigJointQuorumValidation,
            Self::UpgradeGradualRollout,
            Self::UpgradeWithFailure,
            Self::UpgradeRollback,
            Self::UpgradeFeatureActivation,
            Self::RbacUnauthorizedColumnAccess,
            Self::RbacRoleEscalationAttack,
            Self::RbacRowLevelSecurity,
            Self::RbacAuditTrailComplete,
        ]
    }
}

// ============================================================================
// Scenario Configuration
// ============================================================================

/// Configuration for a specific test scenario.
#[derive(Debug, Clone)]
pub struct ScenarioConfig {
    /// Scenario type.
    pub scenario_type: ScenarioType,
    /// Network configuration.
    pub network_config: NetworkConfig,
    /// Storage configuration.
    pub storage_config: StorageConfig,
    /// Swizzle-clogger (if enabled).
    pub swizzle_clogger: Option<SwizzleClogger>,
    /// Gray failure injector (if enabled).
    pub gray_failure_injector: Option<GrayFailureInjector>,
    /// Byzantine fault injector (if enabled).
    pub byzantine_injector: Option<ByzantineInjector>,
    /// Number of tenants (for multi-tenant scenarios).
    pub num_tenants: usize,
    /// Time compression factor (1.0 = normal, 10.0 = 10x faster).
    pub time_compression_factor: f64,
    /// Maximum simulation time (nanoseconds).
    pub max_time_ns: u64,
    /// Maximum events per simulation.
    pub max_events: u64,
}

impl ScenarioConfig {
    /// Creates a new scenario configuration for the given type.
    pub fn new(scenario_type: ScenarioType, seed: u64) -> Self {
        let mut rng = SimRng::new(seed);

        match scenario_type {
            ScenarioType::Baseline => Self::baseline(),
            ScenarioType::SwizzleClogging => Self::swizzle_clogging(&mut rng),
            ScenarioType::GrayFailures => Self::gray_failures(),
            ScenarioType::MultiTenantIsolation => Self::multi_tenant_isolation(&mut rng),
            ScenarioType::TimeCompression => Self::time_compression(),
            ScenarioType::Combined => Self::combined(&mut rng),
            ScenarioType::ByzantineViewChangeMerge => Self::byzantine_view_change_merge(),
            ScenarioType::ByzantineCommitDesync => Self::byzantine_commit_desync(),
            ScenarioType::ByzantineInflatedCommit => Self::byzantine_inflated_commit(),
            ScenarioType::ByzantineInvalidMetadata => Self::byzantine_invalid_metadata(),
            ScenarioType::ByzantineMaliciousViewChange => Self::byzantine_malicious_view_change(),
            ScenarioType::ByzantineLeaderRace => Self::byzantine_leader_race(),
            ScenarioType::ByzantineReplayOldView => Self::byzantine_replay_old_view(),
            ScenarioType::ByzantineCorruptChecksums => Self::byzantine_corrupt_checksums(),
            ScenarioType::ByzantineViewChangeBlocking => Self::byzantine_view_change_blocking(),
            ScenarioType::ByzantinePrepareFlood => Self::byzantine_prepare_flood(),
            ScenarioType::ByzantineSelectiveSilence => Self::byzantine_selective_silence(),
            ScenarioType::ByzantineDvcTailLengthMismatch => {
                Self::byzantine_dvc_tail_length_mismatch()
            }
            ScenarioType::ByzantineDvcIdenticalClaims => Self::byzantine_dvc_identical_claims(),
            ScenarioType::ByzantineOversizedStartView => Self::byzantine_oversized_start_view(),
            ScenarioType::ByzantineInvalidRepairRange => Self::byzantine_invalid_repair_range(),
            ScenarioType::ByzantineInvalidKernelCommand => Self::byzantine_invalid_kernel_command(),
            ScenarioType::CorruptionBitFlip => Self::corruption_bit_flip(),
            ScenarioType::CorruptionChecksumValidation => Self::corruption_checksum_validation(),
            ScenarioType::CorruptionSilentDiskFailure => Self::corruption_silent_disk_failure(),
            ScenarioType::CrashDuringCommit => Self::crash_during_commit(),
            ScenarioType::CrashDuringViewChange => Self::crash_during_view_change(),
            ScenarioType::RecoveryCorruptLog => Self::recovery_corrupt_log(),
            ScenarioType::GrayFailureSlowDisk => Self::gray_failure_slow_disk(),
            ScenarioType::GrayFailureIntermittentNetwork => {
                Self::gray_failure_intermittent_network()
            }
            ScenarioType::RaceConcurrentViewChanges => Self::race_concurrent_view_changes(),
            ScenarioType::RaceCommitDuringDvc => Self::race_commit_during_dvc(),
            ScenarioType::ClockDrift => Self::clock_drift(),
            ScenarioType::ClockOffsetExceeded => Self::clock_offset_exceeded(),
            ScenarioType::ClockNtpFailure => Self::clock_ntp_failure(),
            ScenarioType::ClockBackwardJump => Self::clock_backward_jump(),
            ScenarioType::ClientSessionCrash => Self::client_session_crash(),
            ScenarioType::ClientSessionViewChangeLockout => {
                Self::client_session_view_change_lockout()
            }
            ScenarioType::ClientSessionEviction => Self::client_session_eviction(),
            ScenarioType::RepairBudgetPreventsStorm => Self::repair_budget_prevents_storm(&mut rng),
            ScenarioType::RepairEwmaSelection => Self::repair_ewma_selection(&mut rng),
            ScenarioType::RepairSyncTimeout => Self::repair_sync_timeout(),
            ScenarioType::PrimaryAbdicatePartition => Self::primary_abdicate_partition(&mut rng),
            ScenarioType::CommitStallDetection => Self::commit_stall_detection(),
            ScenarioType::PingHeartbeat => Self::ping_heartbeat(),
            ScenarioType::CommitMessageFallback => Self::commit_message_fallback(&mut rng),
            ScenarioType::StartViewChangeWindow => Self::start_view_change_window(),
            ScenarioType::TimeoutComprehensive => Self::timeout_comprehensive(&mut rng),
            ScenarioType::ScrubDetectsCorruption => Self::scrub_detects_corruption(),
            ScenarioType::ScrubCompletesTour => Self::scrub_completes_tour(),
            ScenarioType::ScrubRateLimited => Self::scrub_rate_limited(),
            ScenarioType::ScrubTriggersRepair => Self::scrub_triggers_repair(),
            ScenarioType::ReconfigAddReplicas => Self::reconfig_add_replicas(),
            ScenarioType::ReconfigRemoveReplicas => Self::reconfig_remove_replicas(),
            ScenarioType::ReconfigDuringPartition => Self::reconfig_during_partition(&mut rng),
            ScenarioType::ReconfigDuringViewChange => Self::reconfig_during_view_change(&mut rng),
            ScenarioType::ReconfigConcurrentRequests => Self::reconfig_concurrent_requests(),
            ScenarioType::ReconfigJointQuorumValidation => Self::reconfig_joint_quorum_validation(),
            ScenarioType::UpgradeGradualRollout => Self::upgrade_gradual_rollout(),
            ScenarioType::UpgradeWithFailure => Self::upgrade_with_failure(&mut rng),
            ScenarioType::UpgradeRollback => Self::upgrade_rollback(),
            ScenarioType::UpgradeFeatureActivation => Self::upgrade_feature_activation(),
            ScenarioType::StandbyFollowsLog => Self::standby_follows_log(),
            ScenarioType::StandbyPromotion => Self::standby_promotion(),
            ScenarioType::StandbyReadScaling => Self::standby_read_scaling(),
            ScenarioType::RbacUnauthorizedColumnAccess => Self::rbac_unauthorized_column_access(),
            ScenarioType::RbacRoleEscalationAttack => Self::rbac_role_escalation_attack(),
            ScenarioType::RbacRowLevelSecurity => Self::rbac_row_level_security(&mut rng),
            ScenarioType::RbacAuditTrailComplete => Self::rbac_audit_trail_complete(),
        }
    }

    /// Baseline scenario: no faults.
    fn baseline() -> Self {
        Self {
            scenario_type: ScenarioType::Baseline,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000, // 1ms
                max_delay_ns: 5_000_000, // 5ms
                drop_probability: 0.0,
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000, // 10 seconds
            max_events: 10_000,
        }
    }

    /// Swizzle-clogging scenario: intermittent network congestion.
    fn swizzle_clogging(rng: &mut SimRng) -> Self {
        // Choose aggressive or mild clogging randomly
        let clogger = if rng.next_bool() {
            SwizzleClogger::aggressive()
        } else {
            SwizzleClogger::mild()
        };

        Self {
            scenario_type: ScenarioType::SwizzleClogging,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 10_000_000,
                drop_probability: 0.05, // 5% base drop rate
                duplicate_probability: 0.02,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: Some(clogger),
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 15_000, // More events to observe clogging effects
        }
    }

    /// Gray failures scenario: partial node failures.
    fn gray_failures() -> Self {
        let gray_injector = GrayFailureInjector::new(
            0.1, // 10% chance of entering gray failure
            0.3, // 30% chance of recovery
        );

        Self {
            scenario_type: ScenarioType::GrayFailures,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 20_000_000, // Higher latency for slow nodes
                drop_probability: 0.02,
                duplicate_probability: 0.01,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: Some(gray_injector),
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 15_000,
        }
    }

    /// Multi-tenant isolation scenario: multiple tenants with faults.
    fn multi_tenant_isolation(rng: &mut SimRng) -> Self {
        Self {
            scenario_type: ScenarioType::MultiTenantIsolation,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 10_000_000,
                drop_probability: rng.next_f64() * 0.05, // 0-5%
                duplicate_probability: rng.next_f64() * 0.02,
                max_in_flight: 2000, // More capacity for multiple tenants
            },
            storage_config: StorageConfig {
                min_write_latency_ns: 500_000,
                max_write_latency_ns: 2_000_000,
                min_read_latency_ns: 50_000,
                max_read_latency_ns: 200_000,
                write_failure_probability: rng.next_f64() * 0.01,
                read_corruption_probability: rng.next_f64() * 0.001,
                fsync_failure_probability: rng.next_f64() * 0.01,
                partial_write_probability: rng.next_f64() * 0.01,
                ..Default::default()
            },
            swizzle_clogger: Some(SwizzleClogger::mild()),
            gray_failure_injector: Some(GrayFailureInjector::new(0.05, 0.4)),
            byzantine_injector: None,
            num_tenants: 5, // Test with 5 concurrent tenants
            time_compression_factor: 1.0,
            max_time_ns: 15_000_000_000, // 15 seconds (more work)
            max_events: 25_000,          // More events for multiple tenants
        }
    }

    /// Time compression scenario: 10x accelerated time.
    fn time_compression() -> Self {
        Self {
            scenario_type: ScenarioType::TimeCompression,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.01,
                duplicate_probability: 0.005,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 10.0, // 10x faster
            max_time_ns: 100_000_000_000,  // 100 seconds simulated (10s real)
            max_events: 50_000,            // More events in compressed time
        }
    }

    /// Combined scenario: all fault types enabled.
    fn combined(rng: &mut SimRng) -> Self {
        let clogger = if rng.next_bool() {
            SwizzleClogger::aggressive()
        } else {
            SwizzleClogger::mild()
        };

        let gray_injector = GrayFailureInjector::new(0.15, 0.25);

        Self {
            scenario_type: ScenarioType::Combined,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 50_000_000,               // High variability
                drop_probability: rng.next_f64() * 0.1, // 0-10%
                duplicate_probability: rng.next_f64() * 0.05,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig {
                min_write_latency_ns: 500_000,
                max_write_latency_ns: 5_000_000,
                min_read_latency_ns: 50_000,
                max_read_latency_ns: 500_000,
                write_failure_probability: rng.next_f64() * 0.02,
                read_corruption_probability: rng.next_f64() * 0.002,
                fsync_failure_probability: rng.next_f64() * 0.02,
                partial_write_probability: rng.next_f64() * 0.02,
                ..Default::default()
            },
            swizzle_clogger: Some(clogger),
            gray_failure_injector: Some(gray_injector),
            byzantine_injector: None,
            num_tenants: 3,               // Multiple tenants
            time_compression_factor: 5.0, // 5x compression
            max_time_ns: 50_000_000_000,  // 50 seconds simulated
            max_events: 30_000,
        }
    }

    /// Byzantine scenario: View change log merge (Bug #1).
    fn byzantine_view_change_merge() -> Self {
        use crate::AttackPattern;
        Self {
            scenario_type: ScenarioType::ByzantineViewChangeMerge,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 10_000_000,
                drop_probability: 0.1, // Force view changes
                duplicate_probability: 0.02,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: Some(AttackPattern::ViewChangeMergeOverwrite.injector()),
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 20_000, // More events to trigger view changes
        }
    }

    /// Byzantine scenario: Commit number desync (Bug #2).
    fn byzantine_commit_desync() -> Self {
        use crate::AttackPattern;
        Self {
            scenario_type: ScenarioType::ByzantineCommitDesync,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 10_000_000,
                drop_probability: 0.1,
                duplicate_probability: 0.01,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: Some(AttackPattern::CommitNumberDesync.injector()),
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 20_000,
        }
    }

    /// Byzantine scenario: Inflated commit number (Bug #3).
    fn byzantine_inflated_commit() -> Self {
        use crate::AttackPattern;
        Self {
            scenario_type: ScenarioType::ByzantineInflatedCommit,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 10_000_000,
                drop_probability: 0.1,
                duplicate_probability: 0.01,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: Some(AttackPattern::InflatedCommitNumber.injector()),
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 20_000,
        }
    }

    /// Byzantine scenario: Invalid entry metadata (Bug #4).
    fn byzantine_invalid_metadata() -> Self {
        use crate::AttackPattern;
        Self {
            scenario_type: ScenarioType::ByzantineInvalidMetadata,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.05,
                duplicate_probability: 0.01,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: Some(AttackPattern::InvalidEntryMetadata.injector()),
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 15_000,
        }
    }

    /// Byzantine scenario: Malicious view change selection (Bug #5).
    fn byzantine_malicious_view_change() -> Self {
        use crate::AttackPattern;
        Self {
            scenario_type: ScenarioType::ByzantineMaliciousViewChange,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 10_000_000,
                drop_probability: 0.1,
                duplicate_probability: 0.02,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: Some(AttackPattern::MaliciousViewChangeSelection.injector()),
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 20_000,
        }
    }

    /// Byzantine scenario: Leader selection race (Bug #6).
    fn byzantine_leader_race() -> Self {
        use crate::AttackPattern;
        Self {
            scenario_type: ScenarioType::ByzantineLeaderRace,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 15_000_000, // High variance for races
                drop_probability: 0.15,   // More partitions
                duplicate_probability: 0.03,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: Some(AttackPattern::LeaderSelectionRace.injector()),
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 25_000, // More events for race conditions
        }
    }

    /// Byzantine scenario: Replay old view messages (AUDIT-2026-03 H-1).
    fn byzantine_replay_old_view() -> Self {
        use crate::ProtocolAttack;
        Self {
            scenario_type: ScenarioType::ByzantineReplayOldView,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 10_000_000,
                drop_probability: 0.05,
                duplicate_probability: 0.02,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: Some(ByzantineInjector::from_protocol_attack(ProtocolAttack::ReplayOldView {
                old_view: 2, // Replay messages from 2 views ago
            })),
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 15_000,
        }
    }

    /// Byzantine scenario: Corrupt message checksums (AUDIT-2026-03 H-1).
    fn byzantine_corrupt_checksums() -> Self {
        use crate::ProtocolAttack;
        Self {
            scenario_type: ScenarioType::ByzantineCorruptChecksums,
            network_config: NetworkConfig::default(),
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: Some(ByzantineInjector::from_protocol_attack(
                ProtocolAttack::CorruptChecksums {
                    corruption_rate: 0.1, // 10% checksum corruption rate
                },
            )),
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 15_000,
        }
    }

    /// Byzantine scenario: Block DoViewChange messages (AUDIT-2026-03 H-1).
    fn byzantine_view_change_blocking() -> Self {
        use crate::ProtocolAttack;
        use kimberlite_vsr::ReplicaId;
        Self {
            scenario_type: ScenarioType::ByzantineViewChangeBlocking,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 10_000_000,
                drop_probability: 0.1,
                duplicate_probability: 0.02,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: Some(ByzantineInjector::from_protocol_attack(
                ProtocolAttack::ViewChangeBlocking {
                    blocked_replicas: vec![ReplicaId::new(2), ReplicaId::new(3)], // Block 2 replicas
                },
            )),
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 20_000, // More events to observe liveness impact
        }
    }

    /// Byzantine scenario: Flood with excessive Prepare messages (AUDIT-2026-03 H-1).
    fn byzantine_prepare_flood() -> Self {
        use crate::ProtocolAttack;
        Self {
            scenario_type: ScenarioType::ByzantinePrepareFlood,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.05,
                duplicate_probability: 0.02,
                max_in_flight: 2000, // Higher to allow flood
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: Some(ByzantineInjector::from_protocol_attack(ProtocolAttack::PrepareFlood {
                rate_multiplier: 10, // Send 10x normal Prepare messages
            })),
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 30_000, // More events for flooding scenario
        }
    }

    /// Byzantine scenario: Selectively ignore messages from specific replicas (AUDIT-2026-03 H-1).
    fn byzantine_selective_silence() -> Self {
        use crate::ProtocolAttack;
        use kimberlite_vsr::ReplicaId;
        Self {
            scenario_type: ScenarioType::ByzantineSelectiveSilence,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 10_000_000,
                drop_probability: 0.1,
                duplicate_probability: 0.02,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: Some(ByzantineInjector::from_protocol_attack(
                ProtocolAttack::SelectiveSilence {
                    ignored_replicas: vec![ReplicaId::new(1), ReplicaId::new(4)], // Ignore 2 replicas
                },
            )),
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 20_000,
        }
    }

    /// Byzantine scenario: DoViewChange log_tail length mismatch (Bug 3.1).
    fn byzantine_dvc_tail_length_mismatch() -> Self {
        use crate::AttackPattern;
        Self {
            scenario_type: ScenarioType::ByzantineDvcTailLengthMismatch,
            network_config: NetworkConfig::default(),
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: Some(AttackPattern::InflatedCommitNumber.injector()),
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 20_000,
        }
    }

    /// Byzantine scenario: DoViewChange with identical claims (Bug 3.3).
    fn byzantine_dvc_identical_claims() -> Self {
        use crate::AttackPattern;
        Self {
            scenario_type: ScenarioType::ByzantineDvcIdenticalClaims,
            network_config: NetworkConfig::default(),
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: Some(AttackPattern::MaliciousViewChangeSelection.injector()),
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 20_000,
        }
    }

    /// Byzantine scenario: Oversized StartView log_tail (Bug 3.4 - DoS).
    fn byzantine_oversized_start_view() -> Self {
        use crate::AttackPattern;
        Self {
            scenario_type: ScenarioType::ByzantineOversizedStartView,
            network_config: NetworkConfig::default(),
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: Some(AttackPattern::ViewChangeMergeOverwrite.injector()),
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 15_000,
        }
    }

    /// Byzantine scenario: Invalid repair range (Bug 3.5).
    fn byzantine_invalid_repair_range() -> Self {
        Self {
            scenario_type: ScenarioType::ByzantineInvalidRepairRange,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.1, // Some drops to trigger repair
                ..Default::default()
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: Some(ByzantineInjector::new()),
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 20_000,
        }
    }

    /// Byzantine scenario: Invalid kernel command (Bug 3.2).
    fn byzantine_invalid_kernel_command() -> Self {
        use crate::AttackPattern;
        Self {
            scenario_type: ScenarioType::ByzantineInvalidKernelCommand,
            network_config: NetworkConfig::default(),
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: Some(AttackPattern::InflatedCommitNumber.injector()),
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 20_000,
        }
    }

    /// Corruption scenario: Random bit flip in log entry.
    fn corruption_bit_flip() -> Self {
        Self {
            scenario_type: ScenarioType::CorruptionBitFlip,
            network_config: NetworkConfig::default(),
            storage_config: StorageConfig {
                read_corruption_probability: 0.01, // 1% corruption rate
                ..Default::default()
            },
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 15_000,
        }
    }

    /// Corruption scenario: Checksum validation test.
    fn corruption_checksum_validation() -> Self {
        Self {
            scenario_type: ScenarioType::CorruptionChecksumValidation,
            network_config: NetworkConfig::default(),
            storage_config: StorageConfig {
                read_corruption_probability: 0.05, // 5% corruption rate
                ..Default::default()
            },
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 15_000,
        }
    }

    /// Corruption scenario: Silent disk failure.
    fn corruption_silent_disk_failure() -> Self {
        Self {
            scenario_type: ScenarioType::CorruptionSilentDiskFailure,
            network_config: NetworkConfig::default(),
            storage_config: StorageConfig {
                read_corruption_probability: 0.02,
                write_failure_probability: 0.01,
                ..Default::default()
            },
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 15_000,
        }
    }

    /// Crash scenario: During commit application.
    fn crash_during_commit() -> Self {
        Self {
            scenario_type: ScenarioType::CrashDuringCommit,
            network_config: NetworkConfig::default(),
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: Some(GrayFailureInjector::new(0.05, 0.1)),
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 20_000,
        }
    }

    /// Crash scenario: During view change.
    fn crash_during_view_change() -> Self {
        Self {
            scenario_type: ScenarioType::CrashDuringViewChange,
            network_config: NetworkConfig {
                drop_probability: 0.1, // Cause view changes
                ..Default::default()
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: Some(GrayFailureInjector::new(0.1, 0.1)),
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 25_000,
        }
    }

    /// Recovery scenario: Corrupt log.
    fn recovery_corrupt_log() -> Self {
        Self {
            scenario_type: ScenarioType::RecoveryCorruptLog,
            network_config: NetworkConfig::default(),
            storage_config: StorageConfig {
                read_corruption_probability: 0.03,
                ..Default::default()
            },
            swizzle_clogger: None,
            gray_failure_injector: Some(GrayFailureInjector::new(0.02, 0.05)),
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 20_000,
        }
    }

    /// Gray failure scenario: Slow disk I/O.
    fn gray_failure_slow_disk() -> Self {
        Self {
            scenario_type: ScenarioType::GrayFailureSlowDisk,
            network_config: NetworkConfig::default(),
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: Some(GrayFailureInjector::new(0.3, 0.1)),
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 15_000,
        }
    }

    /// Gray failure scenario: Intermittent network.
    fn gray_failure_intermittent_network() -> Self {
        Self {
            scenario_type: ScenarioType::GrayFailureIntermittentNetwork,
            network_config: NetworkConfig {
                drop_probability: 0.2,
                duplicate_probability: 0.05,
                min_delay_ns: 1_000_000,
                max_delay_ns: 20_000_000,
                ..Default::default()
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: Some(SwizzleClogger::mild()),
            gray_failure_injector: Some(GrayFailureInjector::new(0.2, 0.1)),
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 20_000,
        }
    }

    /// Race scenario: Concurrent view changes.
    fn race_concurrent_view_changes() -> Self {
        Self {
            scenario_type: ScenarioType::RaceConcurrentViewChanges,
            network_config: NetworkConfig {
                drop_probability: 0.15,
                min_delay_ns: 1_000_000,
                max_delay_ns: 15_000_000,
                ..Default::default()
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 30_000,
        }
    }

    /// Race scenario: Commit during DoViewChange.
    fn race_commit_during_dvc() -> Self {
        Self {
            scenario_type: ScenarioType::RaceCommitDuringDvc,
            network_config: NetworkConfig {
                drop_probability: 0.1,
                min_delay_ns: 1_000_000,
                max_delay_ns: 10_000_000,
                ..Default::default()
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000,
            max_events: 25_000,
        }
    }

    // ========================================================================
    // Phase 1: Clock Synchronization Scenarios
    // ========================================================================

    /// Clock scenario: Gradual drift detection.
    ///
    /// Tests that replicas detect and handle gradual clock drift within
    /// tolerance bounds (CLOCK_OFFSET_TOLERANCE_MS = 500ms).
    fn clock_drift() -> Self {
        Self {
            scenario_type: ScenarioType::ClockDrift,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.02,
                duplicate_probability: 0.01,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: Some(GrayFailureInjector::new(0.05, 0.1)),
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 30_000_000_000, // 30 seconds for drift to accumulate
            max_events: 50_000,
        }
    }

    /// Clock scenario: Offset exceeds tolerance.
    ///
    /// Tests that replicas reject clock samples when offset exceeds
    /// CLOCK_OFFSET_TOLERANCE_MS (500ms).
    fn clock_offset_exceeded() -> Self {
        Self {
            scenario_type: ScenarioType::ClockOffsetExceeded,
            network_config: NetworkConfig {
                min_delay_ns: 5_000_000,   // Higher delay to cause offset
                max_delay_ns: 100_000_000, // Very high delay (100ms)
                drop_probability: 0.1,
                duplicate_probability: 0.02,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: Some(SwizzleClogger::aggressive()),
            gray_failure_injector: Some(GrayFailureInjector::new(0.15, 0.05)),
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 20_000_000_000, // 20 seconds
            max_events: 30_000,
        }
    }

    /// Clock scenario: NTP failure simulation.
    ///
    /// Tests graceful degradation when clock samples are unavailable
    /// (simulating NTP server failure).
    fn clock_ntp_failure() -> Self {
        Self {
            scenario_type: ScenarioType::ClockNtpFailure,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 10_000_000,
                drop_probability: 0.3, // High drop rate to prevent clock samples
                duplicate_probability: 0.01,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: Some(SwizzleClogger::aggressive()),
            gray_failure_injector: Some(GrayFailureInjector::new(0.2, 0.05)),
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 20_000_000_000, // 20 seconds
            max_events: 25_000,
        }
    }

    /// Clock scenario: Backward jump during partition.
    ///
    /// Tests that clock monotonicity is preserved when:
    /// 1. Primary gets partitioned from cluster
    /// 2. System clock jumps backward (simulating NTP adjustment)
    /// 3. View change occurs and new primary takes over
    /// 4. Original primary rejoins with stale clock
    ///
    /// **Critical test:** Ensures HIPAA/GDPR audit timestamp monotonicity
    /// even under extreme clock conditions (backward jumps).
    fn clock_backward_jump() -> Self {
        Self {
            scenario_type: ScenarioType::ClockBackwardJump,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.15, // Moderate drops to trigger partition
                duplicate_probability: 0.01,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: Some(SwizzleClogger::mild()), // Network partition simulation
            gray_failure_injector: Some(GrayFailureInjector::new(0.1, 0.2)), // Intermittent failures
            byzantine_injector: None, // No Byzantine attacks, just clock issues
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 25_000_000_000, // 25 seconds for partition + view change + recovery
            max_events: 40_000,
        }
    }

    // ========================================================================
    // Phase 1: Client Session Scenarios
    // ========================================================================

    /// Client session scenario: Crash recovery (VRR Bug #1).
    ///
    /// Tests that successive client crashes with request number reset
    /// don't cause request collisions (wrong cached replies returned).
    fn client_session_crash() -> Self {
        Self {
            scenario_type: ScenarioType::ClientSessionCrash,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.05,
                duplicate_probability: 0.02,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: Some(GrayFailureInjector::new(0.1, 0.2)), // Simulate client crashes
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 15_000_000_000, // 15 seconds
            max_events: 40_000,
        }
    }

    /// Client session scenario: View change lockout prevention (VRR Bug #2).
    ///
    /// Tests that uncommitted client sessions are discarded during view
    /// change to prevent client lockout (new leader rejects client).
    fn client_session_view_change_lockout() -> Self {
        Self {
            scenario_type: ScenarioType::ClientSessionViewChangeLockout,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 10_000_000,
                drop_probability: 0.15, // Trigger view changes
                duplicate_probability: 0.03,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: Some(SwizzleClogger::mild()),
            gray_failure_injector: Some(GrayFailureInjector::new(0.1, 0.1)),
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 20_000_000_000, // 20 seconds
            max_events: 35_000,
        }
    }

    /// Client session scenario: Deterministic eviction.
    ///
    /// Tests that session eviction is deterministic across all replicas
    /// when max_sessions limit is exceeded (evicts by oldest commit_timestamp).
    fn client_session_eviction() -> Self {
        Self {
            scenario_type: ScenarioType::ClientSessionEviction,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.02,
                duplicate_probability: 0.01,
                max_in_flight: 2000, // Higher capacity for many sessions
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 3, // Multiple tenants to create many sessions
            time_compression_factor: 1.0,
            max_time_ns: 15_000_000_000, // 15 seconds
            max_events: 100_000,         // High event count to trigger eviction
        }
    }

    /// Phase 2 Scenario: Repair budget prevents repair storm.
    ///
    /// Tests that when multiple replicas fall behind, the repair budget
    /// system prevents message queue overflow by rate-limiting repair requests.
    fn repair_budget_prevents_storm(_rng: &mut SimRng) -> Self {
        Self {
            scenario_type: ScenarioType::RepairBudgetPreventsStorm,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 10_000_000, // Higher latency
                drop_probability: 0.15,   // High drop rate to create lag
                duplicate_probability: 0.0,
                max_in_flight: 100, // Limited capacity to test overflow
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: Some(SwizzleClogger::aggressive()),
            gray_failure_injector: Some(GrayFailureInjector::new(
                0.3, // 30% failure probability
                0.1, // 10% recovery probability (stay slow)
            )),
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 30_000_000_000, // 30 seconds
            max_events: 50_000,
        }
    }

    /// Phase 2 Scenario: EWMA-based smart replica selection.
    ///
    /// Tests that repair requests are intelligently routed to fast replicas
    /// based on EWMA latency tracking.
    fn repair_ewma_selection(_rng: &mut SimRng) -> Self {
        Self {
            scenario_type: ScenarioType::RepairEwmaSelection,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 50_000_000, // Wide latency variance
                drop_probability: 0.05,
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: Some(GrayFailureInjector::new(
                0.4,  // 40% failure probability (slow replicas)
                0.05, // 5% recovery (persistent slowness)
            )),
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 20_000_000_000, // 20 seconds
            max_events: 40_000,
        }
    }

    /// Phase 2 Scenario: Repair sync timeout escalates to state transfer.
    ///
    /// Tests that when repair is stuck for a large gap (>100 ops), the
    /// repair sync timeout triggers escalation to state transfer.
    fn repair_sync_timeout() -> Self {
        Self {
            scenario_type: ScenarioType::RepairSyncTimeout,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.30, // Very high drop rate to stall repair
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 25_000_000_000, // 25 seconds (long enough for timeout)
            max_events: 50_000,
        }
    }

    /// Phase 2 Scenario: Primary abdicate when partitioned from quorum.
    ///
    /// Tests that when the leader is partitioned from a quorum of replicas,
    /// the primary abdicate timeout causes it to step down, preventing deadlock.
    fn primary_abdicate_partition(_rng: &mut SimRng) -> Self {
        Self {
            scenario_type: ScenarioType::PrimaryAbdicatePartition,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.0, // Controlled via swizzle
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: Some(SwizzleClogger::new(
                0.5, // 50% clog probability
                0.3, // 30% unclog probability
                3.0, // 3x delay multiplier
                0.8, // 80% drop when clogged
            )),
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 20_000_000_000, // 20 seconds
            max_events: 30_000,
        }
    }

    /// Phase 2 Scenario: Commit stall detection and backpressure.
    ///
    /// Tests that when the pipeline grows without commit progress, the
    /// commit stall timeout detects the condition and applies backpressure.
    fn commit_stall_detection() -> Self {
        Self {
            scenario_type: ScenarioType::CommitStallDetection,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.10, // Moderate drop rate
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 15_000_000_000, // 15 seconds
            max_events: 60_000,          // High load to create pipeline pressure
        }
    }

    /// Phase 2.2 Scenario: Ping heartbeat health checks.
    ///
    /// Tests that ping timeout triggers regular heartbeats from the leader,
    /// ensuring continuous network health monitoring and early failure detection.
    fn ping_heartbeat() -> Self {
        Self {
            scenario_type: ScenarioType::PingHeartbeat,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 10_000_000,
                drop_probability: 0.05, // Some drops to test heartbeat resilience
                duplicate_probability: 0.02,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000, // 10 seconds
            max_events: 10_000,
        }
    }

    /// Phase 2.2 Scenario: Commit message fallback via heartbeat.
    ///
    /// Tests that when commit messages are delayed or dropped, the commit message
    /// timeout triggers heartbeat fallback to notify backups of commit progress.
    fn commit_message_fallback(rng: &mut SimRng) -> Self {
        Self {
            scenario_type: ScenarioType::CommitMessageFallback,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 20_000_000, // Higher latency to cause delays
                drop_probability: rng.next_f64() * 0.15, // 0-15% drop rate
                duplicate_probability: 0.01,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: Some(SwizzleClogger::mild()),
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 15_000_000_000, // 15 seconds
            max_events: 20_000,
        }
    }

    /// Phase 2.2 Scenario: Start view change window timeout.
    ///
    /// Tests that the view change window timeout prevents premature view change
    /// completion, ensuring split-brain prevention through delayed installation.
    fn start_view_change_window() -> Self {
        Self {
            scenario_type: ScenarioType::StartViewChangeWindow,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 15_000_000, // Moderate latency
                drop_probability: 0.10,   // Trigger view changes
                duplicate_probability: 0.02,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: Some(SwizzleClogger::aggressive()), // Trigger view changes
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 20_000_000_000, // 20 seconds
            max_events: 30_000,
        }
    }

    /// Phase 2.2 Scenario: Comprehensive timeout testing.
    ///
    /// Tests all timeout types (heartbeat, prepare, view change, recovery,
    /// clock sync, ping, primary abdicate, repair sync, commit stall, commit
    /// message, start view change window) under various fault conditions.
    fn timeout_comprehensive(rng: &mut SimRng) -> Self {
        let gray_injector = GrayFailureInjector::new(
            0.15, // 15% chance of gray failure
            0.25, // 25% recovery chance
        );

        Self {
            scenario_type: ScenarioType::TimeoutComprehensive,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 30_000_000, // High latency to trigger timeouts
                drop_probability: rng.next_f64() * 0.20, // 0-20% drops
                duplicate_probability: rng.next_f64() * 0.05,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig {
                min_write_latency_ns: 500_000,
                max_write_latency_ns: 5_000_000, // Slow writes
                min_read_latency_ns: 50_000,
                max_read_latency_ns: 500_000,
                write_failure_probability: rng.next_f64() * 0.05,
                read_corruption_probability: rng.next_f64() * 0.01,
                fsync_failure_probability: rng.next_f64() * 0.05,
                partial_write_probability: rng.next_f64() * 0.02,
                ..Default::default()
            },
            swizzle_clogger: Some(SwizzleClogger::aggressive()),
            gray_failure_injector: Some(gray_injector),
            byzantine_injector: None,
            num_tenants: 3, // Multiple tenants to increase load
            time_compression_factor: 1.0,
            max_time_ns: 30_000_000_000, // 30 seconds
            max_events: 50_000,          // High event count to exercise all timeouts
        }
    }

    // ========================================================================
    // Phase 3: Storage Integrity Scenarios
    // ========================================================================

    /// Phase 3 Scenario: Scrubber detects corruption.
    ///
    /// Tests that background scrubbing detects corrupted entries via checksum
    /// validation and triggers appropriate repair.
    fn scrub_detects_corruption() -> Self {
        Self {
            scenario_type: ScenarioType::ScrubDetectsCorruption,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.0, // Clean network for deterministic corruption
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None, // Note: Would use CorruptionInjector if available
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000, // 10 seconds (enough for tour)
            max_events: 10_000,
        }
    }

    /// Phase 3 Scenario: Scrubber completes tour.
    ///
    /// Tests that the scrubber successfully tours the entire log within
    /// a reasonable time window, validating all entries.
    fn scrub_completes_tour() -> Self {
        Self {
            scenario_type: ScenarioType::ScrubCompletesTour,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.0,
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 30_000_000_000, // 30 seconds (long enough for full tour)
            max_events: 50_000,          // Large log to test tour completion
        }
    }

    /// Phase 3 Scenario: Scrubber respects rate limits.
    ///
    /// Tests that scrubbing respects the IOPS budget (max 10 reads/tick)
    /// and doesn't impact production traffic.
    fn scrub_rate_limited() -> Self {
        Self {
            scenario_type: ScenarioType::ScrubRateLimited,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.0,
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 20_000_000_000, // 20 seconds
            max_events: 100_000,         // High load to test rate limiting under pressure
        }
    }

    /// Phase 3 Scenario: Scrubber triggers repair on corruption.
    ///
    /// Tests that when the scrubber detects corruption, it automatically
    /// triggers repair to restore data integrity.
    fn scrub_triggers_repair() -> Self {
        Self {
            scenario_type: ScenarioType::ScrubTriggersRepair,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 10_000_000,
                drop_probability: 0.02, // Some loss to test repair under stress
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 15_000_000_000, // 15 seconds
            max_events: 20_000,
        }
    }

    // ========================================================================
    // Phase 4: Cluster Reconfiguration Scenarios
    // ========================================================================

    /// Phase 4 scenario: Add replicas (3 → 5).
    ///
    /// Tests joint consensus protocol for adding replicas safely.
    fn reconfig_add_replicas() -> Self {
        Self {
            scenario_type: ScenarioType::ReconfigAddReplicas,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.0, // No loss initially
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 20_000_000_000, // 20 seconds for reconfiguration
            max_events: 10_000,
        }
    }

    /// Phase 4 scenario: Remove replicas (5 → 3).
    ///
    /// Tests joint consensus protocol for removing replicas safely.
    fn reconfig_remove_replicas() -> Self {
        Self {
            scenario_type: ScenarioType::ReconfigRemoveReplicas,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.0,
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 20_000_000_000,
            max_events: 10_000,
        }
    }

    /// Phase 4 scenario: Reconfiguration during network partition.
    ///
    /// Tests that reconfigurations survive network partitions and view changes.
    fn reconfig_during_partition(_rng: &mut crate::rng::SimRng) -> Self {
        Self {
            scenario_type: ScenarioType::ReconfigDuringPartition,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 10_000_000,
                drop_probability: 0.1, // 10% loss to create partitions
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: Some(SwizzleClogger::aggressive()),
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 30_000_000_000, // 30 seconds (needs time for recovery)
            max_events: 15_000,
        }
    }

    /// Phase 4 scenario: View change during joint consensus.
    ///
    /// Tests that view changes during reconfiguration preserve the joint consensus state.
    /// Leader fails during joint consensus, new leader must recover reconfiguration state.
    fn reconfig_during_view_change(_rng: &mut crate::rng::SimRng) -> Self {
        Self {
            scenario_type: ScenarioType::ReconfigDuringViewChange,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.05, // 5% loss to trigger view changes
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: Some(SwizzleClogger::mild()),
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 25_000_000_000, // 25 seconds (time for view change + reconfig)
            max_events: 12_000,
        }
    }

    /// Phase 4 scenario: Concurrent reconfiguration requests.
    ///
    /// Tests that multiple concurrent reconfiguration requests are rejected.
    /// Only one reconfiguration can be in progress at a time.
    fn reconfig_concurrent_requests() -> Self {
        Self {
            scenario_type: ScenarioType::ReconfigConcurrentRequests,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.0,
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 20_000_000_000, // 20 seconds
            max_events: 10_000,
        }
    }

    /// Phase 4 scenario: Joint quorum validation.
    ///
    /// Tests that joint consensus correctly requires quorum in BOTH old and new configs.
    /// Attempts to commit with quorum in only one config should fail.
    fn reconfig_joint_quorum_validation() -> Self {
        Self {
            scenario_type: ScenarioType::ReconfigJointQuorumValidation,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.0,
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 20_000_000_000, // 20 seconds
            max_events: 10_000,
        }
    }

    // ========================================================================
    // Phase 4.2: Rolling Upgrade Scenarios
    // ========================================================================

    /// Phase 4.2 scenario: Gradual rollout (sequential upgrade).
    ///
    /// Tests upgrading replicas one-by-one from v0.3.0 → v0.4.0 without service disruption.
    /// Cluster version should increase as each replica upgrades.
    fn upgrade_gradual_rollout() -> Self {
        Self {
            scenario_type: ScenarioType::UpgradeGradualRollout,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.0, // No packet loss during upgrade
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 30_000_000_000, // 30 seconds (time for sequential upgrades)
            max_events: 15_000,
        }
    }

    /// Phase 4.2 scenario: Replica failure during upgrade.
    ///
    /// Tests that cluster remains operational when a replica fails mid-upgrade.
    /// Verifies: ongoing operations complete, new leader elected if needed.
    fn upgrade_with_failure(_rng: &mut crate::rng::SimRng) -> Self {
        Self {
            scenario_type: ScenarioType::UpgradeWithFailure,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 10_000_000,
                drop_probability: 0.05, // 5% loss to simulate instability
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: Some(SwizzleClogger::mild()),
            gray_failure_injector: Some(GrayFailureInjector::new(0.1, 0.3)),
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 35_000_000_000, // 35 seconds (longer for recovery)
            max_events: 18_000,
        }
    }

    /// Phase 4.2 scenario: Rollback to previous version.
    ///
    /// Tests rolling back from v0.4.0 → v0.3.0 when issues detected.
    /// Cluster version should decrease as replicas roll back.
    fn upgrade_rollback() -> Self {
        Self {
            scenario_type: ScenarioType::UpgradeRollback,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.0,
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 25_000_000_000, // 25 seconds
            max_events: 12_000,
        }
    }

    /// Phase 4.2 scenario: Feature flag activation.
    ///
    /// Tests that new features (e.g., ClusterReconfig) activate only when
    /// all replicas reach the required version (v0.4.0).
    fn upgrade_feature_activation() -> Self {
        Self {
            scenario_type: ScenarioType::UpgradeFeatureActivation,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.0,
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 20_000_000_000, // 20 seconds
            max_events: 10_000,
        }
    }

    // ========================================================================
    // Phase 4.3: Standby Replica Scenarios
    // ========================================================================

    /// Phase 4.3 scenario: Standby follows log without participating in quorum.
    ///
    /// Tests that standby replicas:
    /// - Receive Prepare messages from active replicas
    /// - Append entries to log but DON'T send PrepareOK
    /// - Track commit_number from Commit messages
    /// - Never affect quorum decisions
    ///
    /// Verification (Kani Proof #68): Standby NEVER sends PrepareOK.
    fn standby_follows_log() -> Self {
        Self {
            scenario_type: ScenarioType::StandbyFollowsLog,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.02, // 2% loss (normal network conditions)
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 25_000_000_000, // 25 seconds
            max_events: 12_000,
        }
    }

    /// Phase 4.3 scenario: Standby promotion to active replica.
    ///
    /// Tests that standby replicas can be safely promoted to active status:
    /// - Standby must be up-to-date (log matches active primary)
    /// - Promotion requires cluster reconfiguration (joint consensus)
    /// - Promoted replica begins participating in quorum
    /// - Log consistency is preserved (no divergence)
    ///
    /// Verification (Kani Proof #69): Promotion preserves log consistency.
    fn standby_promotion() -> Self {
        Self {
            scenario_type: ScenarioType::StandbyPromotion,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.0, // No loss during promotion
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 30_000_000_000, // 30 seconds
            max_events: 15_000,
        }
    }

    /// Phase 4.3 scenario: Read scaling with multiple standby replicas.
    ///
    /// Tests that multiple standby replicas can serve read-only queries:
    /// - Standby replicas serve eventually consistent reads
    /// - Reads may lag behind committed operations
    /// - No impact on active cluster performance (quorum)
    /// - Load distributed across multiple standbys
    ///
    /// Use case: Geographic DR + read scaling (offload queries from active replicas).
    fn standby_read_scaling() -> Self {
        Self {
            scenario_type: ScenarioType::StandbyReadScaling,
            network_config: NetworkConfig {
                min_delay_ns: 2_000_000, // Higher latency (geographic distribution)
                max_delay_ns: 10_000_000,
                drop_probability: 0.03, // 3% loss (cross-datacenter)
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: Some(GrayFailureInjector::new(0.05, 0.2)), // Slow reads
            byzantine_injector: None,
            num_tenants: 3, // Multi-tenant read workload
            time_compression_factor: 1.0,
            max_time_ns: 40_000_000_000, // 40 seconds
            max_events: 20_000,
        }
    }

    // ========================================================================
    // Phase 3.2: RBAC (Role-Based Access Control) Scenarios
    // ========================================================================

    /// Phase 3.2 scenario: Unauthorized column access attempt.
    ///
    /// Tests that users cannot access columns denied by their role policy.
    fn rbac_unauthorized_column_access() -> Self {
        Self {
            scenario_type: ScenarioType::RbacUnauthorizedColumnAccess,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.0,
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000, // 10 seconds
            max_events: 5_000,
        }
    }

    /// Phase 3.2 scenario: Role escalation attack prevention.
    ///
    /// Tests that users cannot escalate their role privileges.
    fn rbac_role_escalation_attack() -> Self {
        Self {
            scenario_type: ScenarioType::RbacRoleEscalationAttack,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.0,
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000, // 10 seconds
            max_events: 5_000,
        }
    }

    /// Phase 3.2 scenario: Row-level security enforcement.
    ///
    /// Tests that multi-tenant queries are automatically filtered by tenant_id.
    fn rbac_row_level_security(_rng: &mut crate::rng::SimRng) -> Self {
        Self {
            scenario_type: ScenarioType::RbacRowLevelSecurity,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.0,
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 5, // Multiple tenants for RLS testing
            time_compression_factor: 1.0,
            max_time_ns: 15_000_000_000, // 15 seconds
            max_events: 10_000,
        }
    }

    /// Phase 3.2 scenario: Audit trail completeness.
    ///
    /// Tests that all access attempts (allowed and denied) are logged.
    fn rbac_audit_trail_complete() -> Self {
        Self {
            scenario_type: ScenarioType::RbacAuditTrailComplete,
            network_config: NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.0,
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            },
            storage_config: StorageConfig::default(),
            swizzle_clogger: None,
            gray_failure_injector: None,
            byzantine_injector: None,
            num_tenants: 1,
            time_compression_factor: 1.0,
            max_time_ns: 10_000_000_000, // 10 seconds
            max_events: 5_000,
        }
    }

    /// Applies time compression to a duration.
    #[allow(clippy::cast_sign_loss, clippy::cast_precision_loss)]
    pub fn compress_time(&self, duration_ns: u64) -> u64 {
        if self.time_compression_factor <= 1.0 {
            duration_ns
        } else {
            (duration_ns as f64 / self.time_compression_factor) as u64
        }
    }

    /// Decompresses time for display purposes.
    #[allow(clippy::cast_sign_loss, clippy::cast_precision_loss)]
    pub fn decompress_time(&self, compressed_ns: u64) -> u64 {
        if self.time_compression_factor <= 1.0 {
            compressed_ns
        } else {
            (compressed_ns as f64 * self.time_compression_factor) as u64
        }
    }
}

// ============================================================================
// Tenant Workload Generator
// ============================================================================

/// Generates tenant-specific workloads for multi-tenant scenarios.
#[derive(Debug)]
pub struct TenantWorkloadGenerator {
    /// Number of tenants.
    num_tenants: usize,
    /// Key space per tenant (non-overlapping).
    keys_per_tenant: u64,
}

impl TenantWorkloadGenerator {
    /// Creates a new tenant workload generator.
    pub fn new(num_tenants: usize) -> Self {
        Self {
            num_tenants,
            keys_per_tenant: 100, // Each tenant has 100 keys
        }
    }

    /// Gets the key range for a tenant.
    ///
    /// Returns (`start_key`, `end_key`) exclusive.
    pub fn tenant_key_range(&self, tenant_id: usize) -> (u64, u64) {
        let start = (tenant_id as u64) * self.keys_per_tenant;
        let end = start + self.keys_per_tenant;
        (start, end)
    }

    /// Generates a random key for a tenant.
    pub fn random_key(&self, tenant_id: usize, rng: &mut SimRng) -> u64 {
        let (start, end) = self.tenant_key_range(tenant_id);
        start + (rng.next_u64() % (end - start))
    }

    /// Verifies that a key belongs to a tenant.
    pub fn verify_tenant_isolation(&self, key: u64, expected_tenant: usize) -> bool {
        let (start, end) = self.tenant_key_range(expected_tenant);
        key >= start && key < end
    }

    /// Returns the total number of tenants.
    pub fn num_tenants(&self) -> usize {
        self.num_tenants
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scenario_names() {
        for scenario in ScenarioType::all() {
            assert!(!scenario.name().is_empty());
            assert!(!scenario.description().is_empty());
        }
    }

    #[test]
    fn test_baseline_scenario() {
        let config = ScenarioConfig::new(ScenarioType::Baseline, 12345);
        assert_eq!(config.scenario_type, ScenarioType::Baseline);
        assert!(config.swizzle_clogger.is_none());
        assert!(config.gray_failure_injector.is_none());
        assert_eq!(config.num_tenants, 1);
        assert_eq!(config.time_compression_factor, 1.0);
    }

    #[test]
    fn test_swizzle_clogging_scenario() {
        let config = ScenarioConfig::new(ScenarioType::SwizzleClogging, 12345);
        assert!(config.swizzle_clogger.is_some());
        assert!(config.gray_failure_injector.is_none());
    }

    #[test]
    fn test_gray_failures_scenario() {
        let config = ScenarioConfig::new(ScenarioType::GrayFailures, 12345);
        assert!(config.swizzle_clogger.is_none());
        assert!(config.gray_failure_injector.is_some());
    }

    #[test]
    fn test_multi_tenant_scenario() {
        let config = ScenarioConfig::new(ScenarioType::MultiTenantIsolation, 12345);
        assert_eq!(config.num_tenants, 5);
        assert!(config.swizzle_clogger.is_some());
        assert!(config.gray_failure_injector.is_some());
    }

    #[test]
    fn test_time_compression() {
        let config = ScenarioConfig::new(ScenarioType::TimeCompression, 12345);
        assert_eq!(config.time_compression_factor, 10.0);

        // 10 seconds compressed = 1 second
        let compressed = config.compress_time(10_000_000_000);
        assert_eq!(compressed, 1_000_000_000);

        // Decompression should reverse it
        let decompressed = config.decompress_time(compressed);
        assert_eq!(decompressed, 10_000_000_000);
    }

    #[test]
    fn test_combined_scenario() {
        let config = ScenarioConfig::new(ScenarioType::Combined, 12345);
        assert!(config.swizzle_clogger.is_some());
        assert!(config.gray_failure_injector.is_some());
        assert_eq!(config.num_tenants, 3);
        assert_eq!(config.time_compression_factor, 5.0);
    }

    #[test]
    fn test_tenant_key_isolation() {
        let generator = TenantWorkloadGenerator::new(3);

        // Tenant 0: keys 0-99
        assert_eq!(generator.tenant_key_range(0), (0, 100));
        // Tenant 1: keys 100-199
        assert_eq!(generator.tenant_key_range(1), (100, 200));
        // Tenant 2: keys 200-299
        assert_eq!(generator.tenant_key_range(2), (200, 300));

        // Verify isolation
        assert!(generator.verify_tenant_isolation(50, 0));
        assert!(!generator.verify_tenant_isolation(50, 1));
        assert!(generator.verify_tenant_isolation(150, 1));
        assert!(!generator.verify_tenant_isolation(150, 0));
    }

    #[test]
    fn test_tenant_random_keys() {
        let generator = TenantWorkloadGenerator::new(2);
        let mut rng = SimRng::new(12345);

        // Generate 100 random keys for tenant 0
        for _ in 0..100 {
            let key = generator.random_key(0, &mut rng);
            assert!(generator.verify_tenant_isolation(key, 0));
        }

        // Generate 100 random keys for tenant 1
        for _ in 0..100 {
            let key = generator.random_key(1, &mut rng);
            assert!(generator.verify_tenant_isolation(key, 1));
        }
    }
}
