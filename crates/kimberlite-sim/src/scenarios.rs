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
        }
    }

    /// Returns a description of what this scenario tests.
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
