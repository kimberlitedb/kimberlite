//! Advanced fault injection for simulation testing.
//!
//! This module provides sophisticated fault injection patterns inspired by
//! `FoundationDB` and `TigerBeetle`'s testing approaches.
//!
//! # Fault Types
//!
//! ## Network Faults
//!
//! - **Swizzle-clogging**: Randomly clog/unclog network connections to simulate
//!   intermittent connectivity issues.
//! - **Asymmetric partitions**: One-way network failures where A can reach B
//!   but B cannot reach A.
//!
//! ## Gray Failures
//!
//! Gray failures are partial failures that are harder to detect than complete
//! failures:
//!
//! - **Slow responses**: Node responds but with high latency
//! - **Intermittent failures**: Node fails some requests but not others
//! - **Partial functionality**: Node can read but not write, or vice versa
//!
//! ## Storage Faults
//!
//! - **Seen but corrupt**: Data was written but corrupted on disk
//! - **Not seen**: Write was lost (e.g., crashed before fsync)
//! - **Phantom writes**: Write appears successful but data is wrong

use crate::rng::SimRng;

// ============================================================================
// Swizzle-Clogging (Network)
// ============================================================================

/// Controls swizzle-clogging behavior for a network link.
///
/// Swizzle-clogging randomly clogs and unclogs network connections,
/// simulating intermittent network issues like congestion or flaky links.
#[derive(Debug, Clone)]
pub struct SwizzleClogger {
    /// Probability of transitioning from unclogged to clogged (per check).
    pub clog_probability: f64,
    /// Probability of transitioning from clogged to unclogged (per check).
    pub unclog_probability: f64,
    /// Current clogged state per link: (from, to) -> clogged flag
    clogged_links: std::collections::HashMap<(u64, u64), bool>,
    /// When clogged, messages are delayed by this factor (1.0 = no change).
    pub delay_factor: f64,
    /// When clogged, probability of dropping messages.
    pub clogged_drop_probability: f64,
}

impl SwizzleClogger {
    /// Creates a new swizzle-clogger with the given parameters.
    pub fn new(
        clog_probability: f64,
        unclog_probability: f64,
        delay_factor: f64,
        clogged_drop_probability: f64,
    ) -> Self {
        debug_assert!(
            (0.0..=1.0).contains(&clog_probability),
            "clog_probability must be 0.0 to 1.0"
        );
        debug_assert!(
            (0.0..=1.0).contains(&unclog_probability),
            "unclog_probability must be 0.0 to 1.0"
        );
        debug_assert!(delay_factor >= 1.0, "delay_factor must be >= 1.0");
        debug_assert!(
            (0.0..=1.0).contains(&clogged_drop_probability),
            "clogged_drop_probability must be 0.0 to 1.0"
        );

        Self {
            clog_probability,
            unclog_probability,
            clogged_links: std::collections::HashMap::new(),
            delay_factor,
            clogged_drop_probability,
        }
    }

    /// Creates a mild swizzle-clogger (10% clog, 50% unclog, 2x delay).
    pub fn mild() -> Self {
        Self::new(0.1, 0.5, 2.0, 0.1)
    }

    /// Creates an aggressive swizzle-clogger (30% clog, 20% unclog, 10x delay).
    pub fn aggressive() -> Self {
        Self::new(0.3, 0.2, 10.0, 0.5)
    }

    /// Checks if a link is currently clogged.
    pub fn is_clogged(&self, from: u64, to: u64) -> bool {
        *self.clogged_links.get(&(from, to)).unwrap_or(&false)
    }

    /// Updates the clog state for a link and returns whether it changed.
    pub fn update(&mut self, from: u64, to: u64, rng: &mut SimRng) -> bool {
        let currently_clogged = self.is_clogged(from, to);
        let new_state = if currently_clogged {
            // Currently clogged - maybe unclog
            !rng.next_bool_with_probability(self.unclog_probability)
        } else {
            // Currently unclogged - maybe clog
            rng.next_bool_with_probability(self.clog_probability)
        };

        let changed = currently_clogged != new_state;
        self.clogged_links.insert((from, to), new_state);
        changed
    }

    /// Applies clogging effects to a message delay.
    ///
    /// Returns the adjusted delay and whether to drop the message.
    #[allow(clippy::cast_sign_loss, clippy::cast_precision_loss)]
    pub fn apply(&self, from: u64, to: u64, delay_ns: u64, rng: &mut SimRng) -> (u64, bool) {
        if self.is_clogged(from, to) {
            let adjusted_delay = (delay_ns as f64 * self.delay_factor) as u64;
            let should_drop = rng.next_bool_with_probability(self.clogged_drop_probability);
            (adjusted_delay, should_drop)
        } else {
            (delay_ns, false)
        }
    }

    /// Returns the number of currently clogged links.
    pub fn clogged_count(&self) -> usize {
        self.clogged_links.values().filter(|&&v| v).count()
    }

    /// Resets all clog states.
    pub fn reset(&mut self) {
        self.clogged_links.clear();
    }
}

impl Default for SwizzleClogger {
    fn default() -> Self {
        Self::mild()
    }
}

// ============================================================================
// Gray Failure Injection
// ============================================================================

/// Mode of gray failure for a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrayFailureMode {
    /// Node is healthy (no gray failure).
    Healthy,
    /// Node responds slowly (high latency).
    Slow {
        /// Latency multiplier (e.g., 10.0 = 10x normal latency).
        latency_multiplier: u32,
    },
    /// Node fails intermittently.
    Intermittent {
        /// Probability of failure per operation.
        failure_probability_percent: u8,
    },
    /// Node can only perform certain operations.
    PartialFunction {
        /// Can the node perform reads?
        can_read: bool,
        /// Can the node perform writes?
        can_write: bool,
    },
    /// Node is completely unresponsive (but not crashed).
    Unresponsive,
}

/// Gray failure injector for nodes.
///
/// Gray failures are partial failures that are harder to detect than
/// complete failures. They include slow responses, intermittent failures,
/// and partial functionality.
#[derive(Debug, Clone)]
pub struct GrayFailureInjector {
    /// Failure mode per node.
    node_modes: std::collections::HashMap<u64, GrayFailureMode>,
    /// Probability of transitioning to a gray failure mode.
    pub failure_probability: f64,
    /// Probability of recovering from a gray failure.
    pub recovery_probability: f64,
}

impl GrayFailureInjector {
    /// Creates a new gray failure injector.
    pub fn new(failure_probability: f64, recovery_probability: f64) -> Self {
        debug_assert!(
            (0.0..=1.0).contains(&failure_probability),
            "failure_probability must be 0.0 to 1.0"
        );
        debug_assert!(
            (0.0..=1.0).contains(&recovery_probability),
            "recovery_probability must be 0.0 to 1.0"
        );

        Self {
            node_modes: std::collections::HashMap::new(),
            failure_probability,
            recovery_probability,
        }
    }

    /// Gets the current failure mode for a node.
    pub fn get_mode(&self, node_id: u64) -> GrayFailureMode {
        self.node_modes
            .get(&node_id)
            .copied()
            .unwrap_or(GrayFailureMode::Healthy)
    }

    /// Sets the failure mode for a node.
    pub fn set_mode(&mut self, node_id: u64, mode: GrayFailureMode) {
        self.node_modes.insert(node_id, mode);
    }

    /// Updates failure states for all nodes.
    ///
    /// Returns list of nodes whose state changed.
    pub fn update_all(
        &mut self,
        node_ids: &[u64],
        rng: &mut SimRng,
    ) -> Vec<(u64, GrayFailureMode, GrayFailureMode)> {
        let mut changes = Vec::new();

        for &node_id in node_ids {
            let old_mode = self.get_mode(node_id);
            let new_mode = if old_mode == GrayFailureMode::Healthy {
                // Maybe enter a failure mode
                if rng.next_bool_with_probability(self.failure_probability) {
                    Self::random_failure_mode(rng)
                } else {
                    GrayFailureMode::Healthy
                }
            } else {
                // Maybe recover
                if rng.next_bool_with_probability(self.recovery_probability) {
                    GrayFailureMode::Healthy
                } else {
                    old_mode
                }
            };

            if old_mode != new_mode {
                self.set_mode(node_id, new_mode);
                changes.push((node_id, old_mode, new_mode));
            }
        }

        changes
    }

    /// Generates a random failure mode.
    fn random_failure_mode(rng: &mut SimRng) -> GrayFailureMode {
        match rng.next_usize(4) {
            0 => GrayFailureMode::Slow {
                latency_multiplier: (rng.next_usize(10) + 2) as u32, // 2x to 11x
            },
            1 => GrayFailureMode::Intermittent {
                failure_probability_percent: (rng.next_usize(50) + 10) as u8, // 10% to 59%
            },
            2 => GrayFailureMode::PartialFunction {
                can_read: rng.next_bool(),
                can_write: rng.next_bool(),
            },
            3 => GrayFailureMode::Unresponsive,
            _ => unreachable!(),
        }
    }

    /// Checks if an operation should succeed for a node.
    ///
    /// Returns whether to proceed and the latency multiplier.
    pub fn check_operation(&self, node_id: u64, is_write: bool, rng: &mut SimRng) -> (bool, u32) {
        match self.get_mode(node_id) {
            GrayFailureMode::Healthy => (true, 1),
            GrayFailureMode::Slow { latency_multiplier } => (true, latency_multiplier),
            GrayFailureMode::Intermittent {
                failure_probability_percent,
            } => {
                let should_fail =
                    rng.next_bool_with_probability(f64::from(failure_probability_percent) / 100.0);
                (!should_fail, 1)
            }
            GrayFailureMode::PartialFunction {
                can_read,
                can_write,
            } => {
                let can_proceed = if is_write { can_write } else { can_read };
                (can_proceed, 1)
            }
            GrayFailureMode::Unresponsive => (false, 1),
        }
    }

    /// Returns the number of nodes in a gray failure state.
    pub fn failing_count(&self) -> usize {
        self.node_modes
            .values()
            .filter(|&&m| m != GrayFailureMode::Healthy)
            .count()
    }

    /// Resets all nodes to healthy state.
    pub fn reset(&mut self) {
        self.node_modes.clear();
    }
}

impl Default for GrayFailureInjector {
    fn default() -> Self {
        Self::new(0.05, 0.2) // 5% chance of failure, 20% chance of recovery
    }
}

// ============================================================================
// Enhanced Storage Faults
// ============================================================================

/// Type of storage fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageFaultType {
    /// Write was never persisted (crashed before fsync).
    NotSeen,
    /// Write was persisted but data is corrupted.
    SeenButCorrupt,
    /// Write appears successful but contains wrong data.
    PhantomWrite,
    /// Read returns stale data (from before a write).
    StaleRead,
}

/// Fault state for a storage block.
#[derive(Debug, Clone)]
pub struct BlockFaultState {
    /// Type of fault affecting this block.
    pub fault_type: Option<StorageFaultType>,
    /// Original data before corruption (for debugging).
    pub original_checksum: Option<u32>,
    /// Time when fault was injected.
    pub injected_at_ns: u64,
}

/// Enhanced storage fault injector.
///
/// This injector distinguishes between different types of storage faults,
/// which is critical for Protocol-Aware Recovery (PAR).
#[derive(Debug)]
pub struct StorageFaultInjector {
    /// Fault states per block.
    block_faults: std::collections::HashMap<u64, BlockFaultState>,
    /// Probability of "not seen" fault (lost write).
    pub not_seen_probability: f64,
    /// Probability of "seen but corrupt" fault.
    pub corrupt_probability: f64,
    /// Probability of phantom write fault.
    pub phantom_probability: f64,
    /// Probability of stale read fault.
    pub stale_read_probability: f64,
}

impl StorageFaultInjector {
    /// Creates a new storage fault injector.
    pub fn new(
        not_seen_probability: f64,
        corrupt_probability: f64,
        phantom_probability: f64,
        stale_read_probability: f64,
    ) -> Self {
        debug_assert!(
            (0.0..=1.0).contains(&not_seen_probability),
            "not_seen_probability must be 0.0 to 1.0"
        );
        debug_assert!(
            (0.0..=1.0).contains(&corrupt_probability),
            "corrupt_probability must be 0.0 to 1.0"
        );
        debug_assert!(
            (0.0..=1.0).contains(&phantom_probability),
            "phantom_probability must be 0.0 to 1.0"
        );
        debug_assert!(
            (0.0..=1.0).contains(&stale_read_probability),
            "stale_read_probability must be 0.0 to 1.0"
        );

        Self {
            block_faults: std::collections::HashMap::new(),
            not_seen_probability,
            corrupt_probability,
            phantom_probability,
            stale_read_probability,
        }
    }

    /// Creates a conservative fault injector (low fault rates).
    pub fn conservative() -> Self {
        Self::new(0.001, 0.001, 0.0001, 0.001)
    }

    /// Creates an aggressive fault injector for stress testing.
    pub fn aggressive() -> Self {
        Self::new(0.01, 0.01, 0.001, 0.01)
    }

    /// Decides if a write should be affected by a fault.
    ///
    /// Returns the fault type if one should be injected, or None for success.
    pub fn check_write(
        &mut self,
        block_id: u64,
        time_ns: u64,
        rng: &mut SimRng,
    ) -> Option<StorageFaultType> {
        // Check each fault type in order
        if rng.next_bool_with_probability(self.not_seen_probability) {
            self.inject_fault(block_id, StorageFaultType::NotSeen, time_ns);
            return Some(StorageFaultType::NotSeen);
        }

        if rng.next_bool_with_probability(self.corrupt_probability) {
            self.inject_fault(block_id, StorageFaultType::SeenButCorrupt, time_ns);
            return Some(StorageFaultType::SeenButCorrupt);
        }

        if rng.next_bool_with_probability(self.phantom_probability) {
            self.inject_fault(block_id, StorageFaultType::PhantomWrite, time_ns);
            return Some(StorageFaultType::PhantomWrite);
        }

        // Clear any previous fault
        self.block_faults.remove(&block_id);
        None
    }

    /// Decides if a read should be affected by a fault.
    pub fn check_read(&self, block_id: u64, rng: &mut SimRng) -> Option<StorageFaultType> {
        // Check if block has an existing fault
        if let Some(state) = self.block_faults.get(&block_id) {
            return state.fault_type;
        }

        // Random stale read
        if rng.next_bool_with_probability(self.stale_read_probability) {
            return Some(StorageFaultType::StaleRead);
        }

        None
    }

    /// Injects a fault for a specific block.
    pub fn inject_fault(&mut self, block_id: u64, fault_type: StorageFaultType, time_ns: u64) {
        self.block_faults.insert(
            block_id,
            BlockFaultState {
                fault_type: Some(fault_type),
                original_checksum: None,
                injected_at_ns: time_ns,
            },
        );
    }

    /// Clears faults for a specific block.
    pub fn clear_fault(&mut self, block_id: u64) {
        self.block_faults.remove(&block_id);
    }

    /// Gets the fault state for a block.
    pub fn get_fault(&self, block_id: u64) -> Option<&BlockFaultState> {
        self.block_faults.get(&block_id)
    }

    /// Returns the count of blocks with each fault type.
    pub fn fault_counts(&self) -> FaultCounts {
        let mut counts = FaultCounts::default();
        for state in self.block_faults.values() {
            if let Some(fault_type) = state.fault_type {
                match fault_type {
                    StorageFaultType::NotSeen => counts.not_seen += 1,
                    StorageFaultType::SeenButCorrupt => counts.corrupt += 1,
                    StorageFaultType::PhantomWrite => counts.phantom += 1,
                    StorageFaultType::StaleRead => counts.stale += 1,
                }
            }
        }
        counts
    }

    /// Resets all fault states.
    pub fn reset(&mut self) {
        self.block_faults.clear();
    }
}

impl Default for StorageFaultInjector {
    fn default() -> Self {
        Self::conservative()
    }
}

/// Counts of each fault type.
#[derive(Debug, Clone, Default)]
pub struct FaultCounts {
    /// Number of "not seen" faults.
    pub not_seen: usize,
    /// Number of "seen but corrupt" faults.
    pub corrupt: usize,
    /// Number of phantom write faults.
    pub phantom: usize,
    /// Number of stale read faults.
    pub stale: usize,
}

// ============================================================================
// Timed (Duration-Bounded) Faults
// ============================================================================

/// A fault with an explicit activation window `[scheduled_at, until_ns)`.
///
/// Unlike the probability-driven injectors above (which re-roll per tick), these
/// faults are scheduled once and auto-expire after their duration elapses. This
/// mirrors the Antithesis taxonomy of node hangs, throttling, multi-group
/// partitions, and quiet periods.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimedFault {
    /// Node is alive but makes no progress (equivalent to `SIGSTOP` on a
    /// process). Distinct from a crash because the node never reboots: it
    /// simply stops responding until the fault expires.
    NodeHang {
        /// Replica ID being hung.
        node_id: u64,
        /// Absolute time (ns) at which the hang ends.
        until_ns: u64,
    },
    /// Node continues to run but with a reduced CPU budget per tick, modelling
    /// overloaded or throttled hardware. Budget is advisory — callers decide
    /// how to enforce it (e.g., cap events processed per tick).
    NodeThrottle {
        /// Replica ID being throttled.
        node_id: u64,
        /// Nanoseconds of CPU budget per simulation tick.
        cpu_budget_ns_per_tick: u64,
        /// Absolute time (ns) at which throttling ends.
        until_ns: u64,
    },
    /// N-way network partition. Nodes in different groups cannot communicate
    /// for the duration. Nodes not present in any group behave as if isolated.
    MultiGroupPartition {
        /// Disjoint groups of replica IDs.
        groups: Vec<Vec<u64>>,
        /// Absolute time (ns) at which the partition heals.
        until_ns: u64,
    },
    /// Global fault-free recovery window. While active:
    /// - All currently-active timed faults are terminated early.
    /// - Callers should suppress *new* fault injection (swizzle, gray failures,
    ///   storage faults) by checking [`FaultInjector::is_quiet`].
    ///
    /// Mirrors Antithesis's `ANTITHESIS_STOP_FAULTS` API and is used to test
    /// liveness / eventual-availability after faults cease.
    QuietPeriod {
        /// Absolute time (ns) at which the quiet period ends.
        until_ns: u64,
    },
}

impl TimedFault {
    /// Absolute expiry time (ns) for this fault.
    pub fn until_ns(&self) -> u64 {
        match self {
            Self::NodeHang { until_ns, .. }
            | Self::NodeThrottle { until_ns, .. }
            | Self::MultiGroupPartition { until_ns, .. }
            | Self::QuietPeriod { until_ns } => *until_ns,
        }
    }
}

/// Injector for scheduled, duration-bounded faults.
///
/// Callers schedule faults via [`Self::schedule`] and call [`Self::tick`] each
/// simulation step to expire elapsed faults. Query methods
/// ([`Self::is_node_hung`], [`Self::can_communicate`], [`Self::is_quiet`], etc.)
/// report the effective state at a given time.
#[derive(Debug, Default, Clone)]
pub struct TimedFaultInjector {
    active: Vec<TimedFault>,
}

impl TimedFaultInjector {
    /// Creates an empty injector.
    pub fn new() -> Self {
        Self::default()
    }

    /// Schedules a new fault. If the fault is a [`TimedFault::QuietPeriod`],
    /// all other currently-active faults are terminated immediately (partitions
    /// heal, hung nodes resume) so the system has a clean window to recover.
    pub fn schedule(&mut self, fault: TimedFault) {
        if matches!(fault, TimedFault::QuietPeriod { .. }) {
            self.active
                .retain(|f| matches!(f, TimedFault::QuietPeriod { .. }));
        }
        self.active.push(fault);
    }

    /// Removes any fault whose `until_ns` has been reached.
    pub fn tick(&mut self, current_time_ns: u64) {
        self.active.retain(|f| f.until_ns() > current_time_ns);
    }

    /// Total active faults, including a quiet period if one is in effect.
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    /// All currently-scheduled faults (for observability / reporting).
    pub fn active(&self) -> &[TimedFault] {
        &self.active
    }

    /// Clears all scheduled faults. Use sparingly — callers are expected to
    /// rely on [`Self::tick`] for normal expiry.
    pub fn reset(&mut self) {
        self.active.clear();
    }

    /// True if a quiet period is in effect. Callers (including other injectors)
    /// should skip new fault injection while this is true.
    pub fn is_quiet(&self, current_time_ns: u64) -> bool {
        self.active
            .iter()
            .any(|f| matches!(f, TimedFault::QuietPeriod { .. }) && f.until_ns() > current_time_ns)
    }

    /// True if the given node is currently hung.
    pub fn is_node_hung(&self, node_id: u64, current_time_ns: u64) -> bool {
        self.active.iter().any(|f| match f {
            TimedFault::NodeHang {
                node_id: n,
                until_ns,
            } => *n == node_id && *until_ns > current_time_ns,
            _ => false,
        })
    }

    /// CPU budget (ns/tick) for a throttled node, or `None` if unthrottled.
    /// If multiple throttles are active for the same node, the smallest budget
    /// (most restrictive) wins.
    pub fn node_cpu_budget(&self, node_id: u64, current_time_ns: u64) -> Option<u64> {
        self.active
            .iter()
            .filter_map(|f| match f {
                TimedFault::NodeThrottle {
                    node_id: n,
                    cpu_budget_ns_per_tick,
                    until_ns,
                } if *n == node_id && *until_ns > current_time_ns => Some(*cpu_budget_ns_per_tick),
                _ => None,
            })
            .min()
    }

    /// Returns the group index of `node_id` in the *earliest-scheduled* active
    /// partition, or `None` if no partition is active. When multiple partitions
    /// overlap, the first wins (partitions should generally not overlap).
    pub fn partition_group(&self, node_id: u64, current_time_ns: u64) -> Option<usize> {
        self.active.iter().find_map(|f| match f {
            TimedFault::MultiGroupPartition { groups, until_ns } if *until_ns > current_time_ns => {
                groups.iter().position(|g| g.contains(&node_id))
            }
            _ => None,
        })
    }

    /// True if `from` can deliver a message to `to` at `current_time_ns`.
    ///
    /// Returns false when:
    /// - Either endpoint is currently hung (hung nodes drop all traffic), or
    /// - A partition is active and the two nodes fall in different groups, or
    /// - A partition is active and at least one node is outside all groups
    ///   (modelled as fully isolated).
    pub fn can_communicate(&self, from: u64, to: u64, current_time_ns: u64) -> bool {
        if self.is_node_hung(from, current_time_ns) || self.is_node_hung(to, current_time_ns) {
            return false;
        }
        match (
            self.partition_group(from, current_time_ns),
            self.partition_group(to, current_time_ns),
        ) {
            (Some(a), Some(b)) => a == b,
            (None, None) => true,
            _ => {
                // At least one node is in an active partition but not in any
                // group — treat as isolated.
                false
            }
        }
    }

    /// Reconcile this injector's active network faults (NodeHang,
    /// MultiGroupPartition) with a [`crate::network::SimNetwork`] by blocking
    /// the corresponding directed links. Callers should:
    ///
    /// 1. Call [`Self::tick`] to expire elapsed faults.
    /// 2. Call this method to rebuild the network's timed-block set.
    ///
    /// This is an authoritative reconciler — it clears any previous
    /// timed-blocks before applying the current state, so it's safe to call
    /// every tick.
    ///
    /// `NodeThrottle` and `QuietPeriod` are not reflected here (they affect
    /// scheduling and fault-injection rate respectively, not connectivity).
    pub fn apply_to_sim_network(&self, net: &mut crate::network::SimNetwork, current_time_ns: u64) {
        net.clear_timed_blocked_links();

        let nodes: Vec<u64> = net.nodes().iter().copied().collect();

        for fault in &self.active {
            if fault.until_ns() <= current_time_ns {
                continue;
            }
            match fault {
                TimedFault::NodeHang { node_id, .. } => {
                    for other in &nodes {
                        if other == node_id {
                            continue;
                        }
                        net.block_link(*node_id, *other);
                        net.block_link(*other, *node_id);
                    }
                }
                TimedFault::MultiGroupPartition { groups, .. } => {
                    for (i, group_a) in groups.iter().enumerate() {
                        for (j, group_b) in groups.iter().enumerate() {
                            if i == j {
                                continue;
                            }
                            for &a in group_a {
                                for &b in group_b {
                                    net.block_link(a, b);
                                }
                            }
                        }
                    }
                }
                TimedFault::NodeThrottle { .. } | TimedFault::QuietPeriod { .. } => {}
            }
        }
    }
}

// ============================================================================
// Combined Fault Injector
// ============================================================================

/// Comprehensive fault injection configuration.
///
/// Combines all fault injection capabilities into a single configurable interface.
#[derive(Debug)]
pub struct FaultInjector {
    /// Network swizzle-clogging.
    pub swizzle: SwizzleClogger,
    /// Gray failure injection.
    pub gray_failures: GrayFailureInjector,
    /// Storage fault injection.
    pub storage_faults: StorageFaultInjector,
    /// Scheduled, duration-bounded faults (hangs, throttling, N-way partitions,
    /// quiet periods).
    pub timed: TimedFaultInjector,
    /// Whether fault injection is enabled.
    pub enabled: bool,
}

impl FaultInjector {
    /// Creates a new fault injector with the given components.
    pub fn new(
        swizzle: SwizzleClogger,
        gray_failures: GrayFailureInjector,
        storage_faults: StorageFaultInjector,
    ) -> Self {
        Self {
            swizzle,
            gray_failures,
            storage_faults,
            timed: TimedFaultInjector::new(),
            enabled: true,
        }
    }

    /// Creates a disabled fault injector.
    pub fn disabled() -> Self {
        Self {
            swizzle: SwizzleClogger::default(),
            gray_failures: GrayFailureInjector::default(),
            storage_faults: StorageFaultInjector::default(),
            timed: TimedFaultInjector::new(),
            enabled: false,
        }
    }

    /// Creates a mild fault injector suitable for basic testing.
    pub fn mild() -> Self {
        Self {
            swizzle: SwizzleClogger::mild(),
            gray_failures: GrayFailureInjector::new(0.02, 0.3),
            storage_faults: StorageFaultInjector::conservative(),
            timed: TimedFaultInjector::new(),
            enabled: true,
        }
    }

    /// Creates an aggressive fault injector for stress testing.
    pub fn aggressive() -> Self {
        Self {
            swizzle: SwizzleClogger::aggressive(),
            gray_failures: GrayFailureInjector::new(0.1, 0.1),
            storage_faults: StorageFaultInjector::aggressive(),
            timed: TimedFaultInjector::new(),
            enabled: true,
        }
    }

    /// Enables or disables fault injection.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// True if a quiet period is active at `current_time_ns`. When true, the
    /// probability-driven injectors (swizzle, gray failures, storage faults)
    /// should be skipped so the system has a fault-free recovery window.
    pub fn is_quiet(&self, current_time_ns: u64) -> bool {
        self.timed.is_quiet(current_time_ns)
    }

    /// Advances time and expires any elapsed timed faults.
    pub fn tick(&mut self, current_time_ns: u64) {
        self.timed.tick(current_time_ns);
    }

    /// Resets all fault states.
    pub fn reset(&mut self) {
        self.swizzle.reset();
        self.gray_failures.reset();
        self.storage_faults.reset();
        self.timed.reset();
    }
}

impl Default for FaultInjector {
    fn default() -> Self {
        Self::mild()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swizzle_clogger_basic() {
        let clogger = SwizzleClogger::new(0.5, 0.5, 2.0, 0.3);
        assert!(!clogger.is_clogged(1, 2));
        assert_eq!(clogger.clogged_count(), 0);
    }

    #[test]
    fn swizzle_clogger_update() {
        let mut clogger = SwizzleClogger::new(1.0, 0.0, 2.0, 0.0); // Always clog
        let mut rng = SimRng::new(42);

        clogger.update(1, 2, &mut rng);
        assert!(clogger.is_clogged(1, 2));
    }

    #[test]
    fn swizzle_clogger_apply() {
        let mut clogger = SwizzleClogger::new(1.0, 0.0, 2.0, 0.0);
        let mut rng = SimRng::new(42);

        // Clog the link
        clogger.update(1, 2, &mut rng);

        // Apply to delay
        let (delay, drop) = clogger.apply(1, 2, 1000, &mut rng);
        assert_eq!(delay, 2000); // 2x delay
        assert!(!drop); // 0% drop probability
    }

    #[test]
    fn gray_failure_healthy_by_default() {
        let injector = GrayFailureInjector::default();
        assert_eq!(injector.get_mode(1), GrayFailureMode::Healthy);
    }

    #[test]
    fn gray_failure_set_mode() {
        let mut injector = GrayFailureInjector::default();
        injector.set_mode(
            1,
            GrayFailureMode::Slow {
                latency_multiplier: 5,
            },
        );

        assert_eq!(
            injector.get_mode(1),
            GrayFailureMode::Slow {
                latency_multiplier: 5
            }
        );
    }

    #[test]
    fn gray_failure_check_operation() {
        let mut injector = GrayFailureInjector::default();
        let mut rng = SimRng::new(42);

        // Healthy node
        let (proceed, mult) = injector.check_operation(1, false, &mut rng);
        assert!(proceed);
        assert_eq!(mult, 1);

        // Slow node
        injector.set_mode(
            1,
            GrayFailureMode::Slow {
                latency_multiplier: 10,
            },
        );
        let (proceed, mult) = injector.check_operation(1, false, &mut rng);
        assert!(proceed);
        assert_eq!(mult, 10);

        // Unresponsive node
        injector.set_mode(1, GrayFailureMode::Unresponsive);
        let (proceed, _) = injector.check_operation(1, false, &mut rng);
        assert!(!proceed);
    }

    #[test]
    fn gray_failure_partial_function() {
        let mut injector = GrayFailureInjector::default();
        let mut rng = SimRng::new(42);

        injector.set_mode(
            1,
            GrayFailureMode::PartialFunction {
                can_read: true,
                can_write: false,
            },
        );

        let (can_read, _) = injector.check_operation(1, false, &mut rng);
        let (can_write, _) = injector.check_operation(1, true, &mut rng);

        assert!(can_read);
        assert!(!can_write);
    }

    #[test]
    fn storage_fault_conservative() {
        let injector = StorageFaultInjector::conservative();
        assert!(injector.not_seen_probability < 0.01);
        assert!(injector.corrupt_probability < 0.01);
    }

    #[test]
    fn storage_fault_injection() {
        let mut injector = StorageFaultInjector::new(1.0, 0.0, 0.0, 0.0); // Always not_seen
        let mut rng = SimRng::new(42);

        let fault = injector.check_write(1, 1000, &mut rng);
        assert_eq!(fault, Some(StorageFaultType::NotSeen));

        let state = injector.get_fault(1).unwrap();
        assert_eq!(state.fault_type, Some(StorageFaultType::NotSeen));
        assert_eq!(state.injected_at_ns, 1000);
    }

    #[test]
    fn storage_fault_counts() {
        let mut injector = StorageFaultInjector::default();
        injector.inject_fault(1, StorageFaultType::NotSeen, 1000);
        injector.inject_fault(2, StorageFaultType::SeenButCorrupt, 2000);
        injector.inject_fault(3, StorageFaultType::NotSeen, 3000);

        let counts = injector.fault_counts();
        assert_eq!(counts.not_seen, 2);
        assert_eq!(counts.corrupt, 1);
        assert_eq!(counts.phantom, 0);
    }

    #[test]
    fn fault_injector_disabled() {
        let injector = FaultInjector::disabled();
        assert!(!injector.enabled);
    }

    #[test]
    fn fault_injector_reset() {
        let mut injector = FaultInjector::aggressive();
        let mut rng = SimRng::new(42);

        // Create some faults
        injector.swizzle.update(1, 2, &mut rng);
        injector
            .gray_failures
            .set_mode(1, GrayFailureMode::Unresponsive);
        injector
            .storage_faults
            .inject_fault(1, StorageFaultType::NotSeen, 1000);
        injector.timed.schedule(TimedFault::NodeHang {
            node_id: 3,
            until_ns: 5_000,
        });

        // Reset
        injector.reset();

        assert_eq!(injector.swizzle.clogged_count(), 0);
        assert_eq!(injector.gray_failures.failing_count(), 0);
        assert_eq!(injector.storage_faults.fault_counts().not_seen, 0);
        assert_eq!(injector.timed.active_count(), 0);
    }

    #[test]
    fn timed_node_hang_expires() {
        let mut timed = TimedFaultInjector::new();
        timed.schedule(TimedFault::NodeHang {
            node_id: 1,
            until_ns: 1_000,
        });

        assert!(timed.is_node_hung(1, 500));
        assert!(!timed.is_node_hung(2, 500));

        // At/after expiry, tick drops it.
        timed.tick(1_000);
        assert!(!timed.is_node_hung(1, 1_000));
        assert_eq!(timed.active_count(), 0);
    }

    #[test]
    fn timed_node_hang_blocks_communication() {
        let mut timed = TimedFaultInjector::new();
        timed.schedule(TimedFault::NodeHang {
            node_id: 2,
            until_ns: 1_000,
        });

        assert!(!timed.can_communicate(1, 2, 500)); // to-hung
        assert!(!timed.can_communicate(2, 1, 500)); // from-hung
        assert!(timed.can_communicate(1, 3, 500)); // neither hung
    }

    #[test]
    fn timed_throttle_reports_smallest_budget() {
        let mut timed = TimedFaultInjector::new();
        timed.schedule(TimedFault::NodeThrottle {
            node_id: 1,
            cpu_budget_ns_per_tick: 500,
            until_ns: 10_000,
        });
        timed.schedule(TimedFault::NodeThrottle {
            node_id: 1,
            cpu_budget_ns_per_tick: 100,
            until_ns: 5_000,
        });

        // Both active — pick most restrictive.
        assert_eq!(timed.node_cpu_budget(1, 1_000), Some(100));

        // After first expires, only the 500-ns budget remains.
        timed.tick(5_000);
        assert_eq!(timed.node_cpu_budget(1, 5_000), Some(500));

        // Unthrottled node.
        assert_eq!(timed.node_cpu_budget(99, 1_000), None);
    }

    #[test]
    fn timed_multi_group_partition() {
        let mut timed = TimedFaultInjector::new();
        timed.schedule(TimedFault::MultiGroupPartition {
            groups: vec![vec![1, 2], vec![3, 4], vec![5]],
            until_ns: 10_000,
        });

        // Same group: OK.
        assert!(timed.can_communicate(1, 2, 1_000));
        assert!(timed.can_communicate(3, 4, 1_000));
        // Different groups: blocked.
        assert!(!timed.can_communicate(1, 3, 1_000));
        assert!(!timed.can_communicate(4, 5, 1_000));
        // Node not in any group: isolated.
        assert!(!timed.can_communicate(1, 99, 1_000));

        // After expiry, communication resumes.
        timed.tick(10_000);
        assert!(timed.can_communicate(1, 3, 10_000));
    }

    #[test]
    fn timed_quiet_period_terminates_existing_faults() {
        let mut timed = TimedFaultInjector::new();
        timed.schedule(TimedFault::NodeHang {
            node_id: 1,
            until_ns: 10_000,
        });
        timed.schedule(TimedFault::MultiGroupPartition {
            groups: vec![vec![1], vec![2]],
            until_ns: 10_000,
        });
        assert_eq!(timed.active_count(), 2);

        // Scheduling a QuietPeriod clears all other active faults.
        timed.schedule(TimedFault::QuietPeriod { until_ns: 5_000 });
        assert_eq!(timed.active_count(), 1);
        assert!(timed.is_quiet(1_000));
        assert!(!timed.is_node_hung(1, 1_000));
        assert!(timed.can_communicate(1, 2, 1_000));

        // Quiet period expires.
        timed.tick(5_000);
        assert!(!timed.is_quiet(5_000));
        assert_eq!(timed.active_count(), 0);
    }

    #[test]
    fn timed_injector_applies_node_hang_to_network() {
        use crate::network::{RejectReason, SendResult, SimNetwork};

        let mut net = SimNetwork::reliable();
        net.register_node(1);
        net.register_node(2);
        net.register_node(3);

        let mut timed = TimedFaultInjector::new();
        timed.schedule(TimedFault::NodeHang {
            node_id: 2,
            until_ns: 1_000,
        });

        timed.apply_to_sim_network(&mut net, 500);
        assert!(net.is_link_timed_blocked(1, 2));
        assert!(net.is_link_timed_blocked(2, 1));
        assert!(!net.is_link_timed_blocked(1, 3));

        // Send through the network — hung link is rejected.
        let mut rng = SimRng::new(7);
        let res = net.send(1, 2, vec![0xAA], 500, &mut rng);
        assert!(matches!(
            res,
            SendResult::Rejected {
                reason: RejectReason::Partitioned
            }
        ));

        // After expiry and re-reconcile, link is restored.
        timed.tick(1_000);
        timed.apply_to_sim_network(&mut net, 1_000);
        assert!(!net.is_link_timed_blocked(1, 2));
        let res = net.send(1, 2, vec![0xAA], 1_000, &mut rng);
        assert!(matches!(res, SendResult::Queued { .. }));
    }

    #[test]
    fn timed_injector_applies_multi_group_partition_to_network() {
        use crate::network::{RejectReason, SendResult, SimNetwork};

        let mut net = SimNetwork::reliable();
        for id in 1..=5 {
            net.register_node(id);
        }

        let mut timed = TimedFaultInjector::new();
        timed.schedule(TimedFault::MultiGroupPartition {
            groups: vec![vec![1, 2], vec![3, 4], vec![5]],
            until_ns: 10_000,
        });
        timed.apply_to_sim_network(&mut net, 1_000);

        // Same group: allowed.
        assert!(!net.is_link_timed_blocked(1, 2));
        assert!(!net.is_link_timed_blocked(3, 4));
        // Across groups: blocked both directions.
        assert!(net.is_link_timed_blocked(1, 3));
        assert!(net.is_link_timed_blocked(3, 1));
        assert!(net.is_link_timed_blocked(5, 2));
        assert!(net.is_link_timed_blocked(2, 5));

        let mut rng = SimRng::new(7);
        let res = net.send(1, 3, vec![0xAA], 1_000, &mut rng);
        assert!(matches!(
            res,
            SendResult::Rejected {
                reason: RejectReason::Partitioned
            }
        ));
        let res = net.send(1, 2, vec![0xAA], 1_000, &mut rng);
        assert!(matches!(res, SendResult::Queued { .. }));
    }

    #[test]
    fn fault_injector_is_quiet_delegates() {
        let mut injector = FaultInjector::mild();
        assert!(!injector.is_quiet(0));

        injector
            .timed
            .schedule(TimedFault::QuietPeriod { until_ns: 1_000 });
        assert!(injector.is_quiet(500));

        injector.tick(1_000);
        assert!(!injector.is_quiet(1_000));
    }
}
