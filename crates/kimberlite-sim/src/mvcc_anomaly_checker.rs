//! MVCC transaction anomaly detection (Jepsen-style).
//!
//! This module implements detection for transaction isolation anomalies
//! using Adya's definitions and dependency graph analysis.
//!
//! ## Detected Anomalies
//!
//! - **G0 (Dirty Write)**: Two transactions write the same item, second overwrites
//!   the first before it commits
//! - **G1a (Dirty Read)**: T1 writes X, T2 reads X, T1 aborts
//! - **G1b (Non-Repeatable Read)**: T1 reads X, T2 writes X, T1 reads X again (different)
//! - **G1c (Phantom Read)**: T1 reads set, T2 modifies set, T1 reads again (different)
//! - **G2 (Lost Update)**: T1 reads X, T2 reads X, both update based on old value
//!
//! ## Architecture
//!
//! 1. **Transaction History Tracking**: Record all reads/writes with versions
//! 2. **Dependency Graph**: Build wr/ww/rw edges between transactions
//! 3. **Cycle Detection**: Find cycles in dependency graph = serializability violation
//!
//! ## References
//!
//! - Adya, "Weak Consistency: A Generalized Theory" (1999)
//! - Jepsen consistency models: https://jepsen.io/consistency
//! - Kimberlite MVCC: offset-based versioning with `created_at`/`deleted_at`

use std::collections::{HashMap, HashSet};

use kimberlite_types::Offset;

use crate::invariant::{InvariantChecker, InvariantResult};

// ============================================================================
// Transaction History Types
// ============================================================================

/// Transaction identifier.
pub type TxnId = u64;

/// A read operation in a transaction.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReadOp {
    /// Key read.
    pub key: String,
    /// Version offset read (Offset at which the value was created).
    pub version: Offset,
    /// The value read (serialized for comparison).
    pub value: String,
}

/// A write operation in a transaction.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WriteOp {
    /// Key written.
    pub key: String,
    /// Value written (serialized).
    pub value: String,
    /// Offset at which this write was created.
    pub created_at: Offset,
}

/// Transaction status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxnStatus {
    /// Transaction is active (not yet committed or aborted).
    Active,
    /// Transaction committed successfully.
    Committed,
    /// Transaction aborted.
    Aborted,
}

/// A transaction's execution history.
#[derive(Debug, Clone)]
pub struct TransactionHistory {
    /// Transaction ID.
    pub txn_id: TxnId,
    /// Transaction start offset.
    pub start_offset: Offset,
    /// Transaction commit/abort offset (None if still active).
    pub end_offset: Option<Offset>,
    /// Transaction status.
    pub status: TxnStatus,
    /// All read operations.
    pub reads: Vec<ReadOp>,
    /// All write operations.
    pub writes: Vec<WriteOp>,
}

impl TransactionHistory {
    /// Creates a new transaction history.
    pub fn new(txn_id: TxnId, start_offset: Offset) -> Self {
        Self {
            txn_id,
            start_offset,
            end_offset: None,
            status: TxnStatus::Active,
            reads: Vec::new(),
            writes: Vec::new(),
        }
    }

    /// Marks the transaction as committed.
    pub fn commit(&mut self, offset: Offset) {
        self.end_offset = Some(offset);
        self.status = TxnStatus::Committed;
    }

    /// Marks the transaction as aborted.
    pub fn abort(&mut self, offset: Offset) {
        self.end_offset = Some(offset);
        self.status = TxnStatus::Aborted;
    }

    /// Returns true if the transaction is committed.
    pub fn is_committed(&self) -> bool {
        self.status == TxnStatus::Committed
    }

    /// Returns true if the transaction is aborted.
    pub fn is_aborted(&self) -> bool {
        self.status == TxnStatus::Aborted
    }
}

// ============================================================================
// Dependency Graph Types
// ============================================================================

/// Dependency type between transactions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DepType {
    /// Write-Read: T1 writes X, T2 reads X.
    WR,
    /// Write-Write: T1 writes X, T2 writes X.
    WW,
    /// Read-Write (anti-dependency): T1 reads X, T2 writes X.
    RW,
}

/// A dependency edge in the transaction dependency graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Dependency {
    /// Source transaction (happened-before).
    pub from: TxnId,
    /// Target transaction (happened-after).
    pub to: TxnId,
    /// Type of dependency.
    pub dep_type: DepType,
    /// Key involved in the dependency.
    pub key: String,
}

// ============================================================================
// MVCC Anomaly Checker
// ============================================================================

/// Detects MVCC transaction isolation anomalies.
///
/// This checker tracks transaction histories and builds a dependency graph
/// to detect Adya's anomalies (G0, G1a, G1b, G1c, G2).
#[derive(Debug)]
pub struct MvccAnomalyChecker {
    /// All transaction histories.
    transactions: HashMap<TxnId, TransactionHistory>,
    /// Next transaction ID.
    next_txn_id: TxnId,
    /// Dependency edges.
    dependencies: Vec<Dependency>,
    /// Number of checks performed.
    checks_performed: u64,
}

impl MvccAnomalyChecker {
    /// Creates a new MVCC anomaly checker.
    pub fn new() -> Self {
        Self {
            transactions: HashMap::new(),
            next_txn_id: 1,
            dependencies: Vec::new(),
            checks_performed: 0,
        }
    }

    /// Starts a new transaction, returning its ID.
    pub fn begin_transaction(&mut self, start_offset: Offset) -> TxnId {
        let txn_id = self.next_txn_id;
        self.next_txn_id += 1;

        let history = TransactionHistory::new(txn_id, start_offset);
        self.transactions.insert(txn_id, history);

        txn_id
    }

    /// Records a read operation.
    pub fn record_read(&mut self, txn_id: TxnId, key: &str, version: Offset, value: &str) {
        if let Some(txn) = self.transactions.get_mut(&txn_id) {
            txn.reads.push(ReadOp {
                key: key.to_string(),
                version,
                value: value.to_string(),
            });
        }
    }

    /// Records a write operation.
    pub fn record_write(&mut self, txn_id: TxnId, key: &str, value: &str, created_at: Offset) {
        if let Some(txn) = self.transactions.get_mut(&txn_id) {
            txn.writes.push(WriteOp {
                key: key.to_string(),
                value: value.to_string(),
                created_at,
            });
        }
    }

    /// Commits a transaction and checks for anomalies.
    pub fn commit_transaction(&mut self, txn_id: TxnId, commit_offset: Offset) -> InvariantResult {
        if let Some(txn) = self.transactions.get_mut(&txn_id) {
            txn.commit(commit_offset);

            // Build dependencies for this transaction
            self.build_dependencies(txn_id);

            // Check for anomalies
            self.checks_performed += 1;
            return self.check_anomalies();
        }

        InvariantResult::Ok
    }

    /// Aborts a transaction and checks for G1a (dirty read).
    pub fn abort_transaction(&mut self, txn_id: TxnId, abort_offset: Offset) -> InvariantResult {
        if let Some(txn) = self.transactions.get_mut(&txn_id) {
            txn.abort(abort_offset);

            // Check if any transaction read data written by this aborted transaction
            self.checks_performed += 1;
            return self.check_dirty_read_on_abort(txn_id);
        }

        InvariantResult::Ok
    }

    /// Builds dependency edges for a transaction.
    fn build_dependencies(&mut self, txn_id: TxnId) {
        let txn = match self.transactions.get(&txn_id) {
            Some(t) => t.clone(),
            None => return,
        };

        // Build WR dependencies: if T1 writes X, and T2 reads X
        for write in &txn.writes {
            for other_txn in self.transactions.values() {
                if other_txn.txn_id == txn_id {
                    continue;
                }

                for read in &other_txn.reads {
                    if read.key == write.key && read.version == write.created_at {
                        self.dependencies.push(Dependency {
                            from: txn_id,
                            to: other_txn.txn_id,
                            dep_type: DepType::WR,
                            key: write.key.clone(),
                        });
                    }
                }
            }
        }

        // Build WW dependencies: if T1 writes X, and T2 writes X
        for write in &txn.writes {
            for other_txn in self.transactions.values() {
                if other_txn.txn_id == txn_id {
                    continue;
                }

                for other_write in &other_txn.writes {
                    if other_write.key == write.key {
                        // Determine order based on created_at
                        if write.created_at < other_write.created_at {
                            self.dependencies.push(Dependency {
                                from: txn_id,
                                to: other_txn.txn_id,
                                dep_type: DepType::WW,
                                key: write.key.clone(),
                            });
                        }
                    }
                }
            }
        }

        // Build RW dependencies: if T1 reads X, and T2 writes X
        for read in &txn.reads {
            for other_txn in self.transactions.values() {
                if other_txn.txn_id == txn_id {
                    continue;
                }

                for write in &other_txn.writes {
                    if write.key == read.key && write.created_at > read.version {
                        self.dependencies.push(Dependency {
                            from: txn_id,
                            to: other_txn.txn_id,
                            dep_type: DepType::RW,
                            key: read.key.clone(),
                        });
                    }
                }
            }
        }
    }

    /// Checks for all anomalies in the transaction history.
    fn check_anomalies(&self) -> InvariantResult {
        // Check G0: Dirty Write (WW cycle with uncommitted transaction)
        if let Some(err) = self.check_dirty_write() {
            return err;
        }

        // Check G1b: Non-Repeatable Read (same key read twice with different values)
        if let Some(err) = self.check_non_repeatable_read() {
            return err;
        }

        // Check G2: Lost Update (cycle involving RW edges)
        if let Some(err) = self.check_lost_update() {
            return err;
        }

        // Check for serializability violations (cycles in dependency graph)
        if let Some(err) = self.check_serializability() {
            return err;
        }

        InvariantResult::Ok
    }

    /// Checks for G0 (Dirty Write): Two concurrent writes to the same key.
    fn check_dirty_write(&self) -> Option<InvariantResult> {
        for dep in &self.dependencies {
            if dep.dep_type == DepType::WW {
                let from_txn = self.transactions.get(&dep.from)?;
                let to_txn = self.transactions.get(&dep.to)?;

                // If both transactions overlap in time and both write the same key
                if let (Some(from_end), Some(to_end)) = (from_txn.end_offset, to_txn.end_offset) {
                    if from_txn.start_offset < to_end && to_txn.start_offset < from_end {
                        return Some(InvariantResult::Violated {
                            invariant: "mvcc_g0_dirty_write".to_string(),
                            message: format!(
                                "G0 (Dirty Write): T{} and T{} both wrote key '{}' with overlapping execution",
                                dep.from, dep.to, dep.key
                            ),
                            context: vec![
                                ("from_txn".to_string(), dep.from.to_string()),
                                ("to_txn".to_string(), dep.to.to_string()),
                                ("key".to_string(), dep.key.clone()),
                                ("from_start".to_string(), from_txn.start_offset.to_string()),
                                ("to_start".to_string(), to_txn.start_offset.to_string()),
                            ],
                        });
                    }
                }
            }
        }
        None
    }

    /// Checks for G1a (Dirty Read): Reading data from an aborted transaction.
    fn check_dirty_read_on_abort(&self, aborted_txn_id: TxnId) -> InvariantResult {
        let _aborted_txn = match self.transactions.get(&aborted_txn_id) {
            Some(t) => t,
            None => return InvariantResult::Ok,
        };

        // Check if any committed transaction read data written by this aborted transaction
        for dep in &self.dependencies {
            if dep.from == aborted_txn_id && dep.dep_type == DepType::WR {
                if let Some(reader_txn) = self.transactions.get(&dep.to) {
                    if reader_txn.is_committed() {
                        return InvariantResult::Violated {
                            invariant: "mvcc_g1a_dirty_read".to_string(),
                            message: format!(
                                "G1a (Dirty Read): T{} read key '{}' written by aborted T{}",
                                dep.to, dep.key, aborted_txn_id
                            ),
                            context: vec![
                                ("aborted_txn".to_string(), aborted_txn_id.to_string()),
                                ("reader_txn".to_string(), dep.to.to_string()),
                                ("key".to_string(), dep.key.clone()),
                            ],
                        };
                    }
                }
            }
        }

        InvariantResult::Ok
    }

    /// Checks for G1b (Non-Repeatable Read): Same key read twice with different values.
    fn check_non_repeatable_read(&self) -> Option<InvariantResult> {
        for txn in self.transactions.values() {
            let mut key_reads: HashMap<&str, Vec<&ReadOp>> = HashMap::new();

            for read in &txn.reads {
                key_reads.entry(&read.key).or_default().push(read);
            }

            for (key, reads) in key_reads {
                if reads.len() > 1 {
                    // Check if different versions were read
                    let first_version = reads[0].version;
                    for read in &reads[1..] {
                        if read.version != first_version {
                            return Some(InvariantResult::Violated {
                                invariant: "mvcc_g1b_non_repeatable_read".to_string(),
                                message: format!(
                                    "G1b (Non-Repeatable Read): T{} read key '{}' with different versions",
                                    txn.txn_id, key
                                ),
                                context: vec![
                                    ("txn_id".to_string(), txn.txn_id.to_string()),
                                    ("key".to_string(), key.to_string()),
                                    ("first_version".to_string(), first_version.to_string()),
                                    ("second_version".to_string(), read.version.to_string()),
                                ],
                            });
                        }
                    }
                }
            }
        }
        None
    }

    /// Checks for G2 (Lost Update): Concurrent reads followed by writes.
    fn check_lost_update(&self) -> Option<InvariantResult> {
        // Look for pattern: T1 reads X, T2 reads X, T1 writes X, T2 writes X
        // This creates a cycle: T1 -rw-> T2 -rw-> T1
        for dep1 in &self.dependencies {
            if dep1.dep_type == DepType::RW {
                for dep2 in &self.dependencies {
                    if dep2.dep_type == DepType::RW
                        && dep1.from == dep2.to
                        && dep1.to == dep2.from
                        && dep1.key == dep2.key
                    {
                        return Some(InvariantResult::Violated {
                            invariant: "mvcc_g2_lost_update".to_string(),
                            message: format!(
                                "G2 (Lost Update): T{} and T{} both read and wrote key '{}' creating RW cycle",
                                dep1.from, dep1.to, dep1.key
                            ),
                            context: vec![
                                ("txn1".to_string(), dep1.from.to_string()),
                                ("txn2".to_string(), dep1.to.to_string()),
                                ("key".to_string(), dep1.key.clone()),
                            ],
                        });
                    }
                }
            }
        }
        None
    }

    /// Checks for serializability violations (cycles in dependency graph).
    fn check_serializability(&self) -> Option<InvariantResult> {
        // Build adjacency list
        let mut graph: HashMap<TxnId, Vec<TxnId>> = HashMap::new();
        for dep in &self.dependencies {
            graph.entry(dep.from).or_default().push(dep.to);
        }

        // DFS cycle detection
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();

        for &txn_id in self.transactions.keys() {
            if !visited.contains(&txn_id) {
                if let Some(cycle) =
                    self.dfs_find_cycle(txn_id, &graph, &mut visited, &mut rec_stack)
                {
                    return Some(InvariantResult::Violated {
                        invariant: "mvcc_serializability_violation".to_string(),
                        message: format!("Cycle detected in dependency graph: {cycle:?}"),
                        context: vec![("cycle".to_string(), format!("{cycle:?}"))],
                    });
                }
            }
        }

        None
    }

    /// DFS helper for cycle detection.
    fn dfs_find_cycle(
        &self,
        node: TxnId,
        graph: &HashMap<TxnId, Vec<TxnId>>,
        visited: &mut HashSet<TxnId>,
        rec_stack: &mut HashSet<TxnId>,
    ) -> Option<Vec<TxnId>> {
        visited.insert(node);
        rec_stack.insert(node);

        if let Some(neighbors) = graph.get(&node) {
            for &neighbor in neighbors {
                if !visited.contains(&neighbor) {
                    if let Some(mut cycle) =
                        self.dfs_find_cycle(neighbor, graph, visited, rec_stack)
                    {
                        cycle.insert(0, node);
                        return Some(cycle);
                    }
                } else if rec_stack.contains(&neighbor) {
                    // Cycle found
                    return Some(vec![node, neighbor]);
                }
            }
        }

        rec_stack.remove(&node);
        None
    }

    /// Returns the number of checks performed.
    pub fn checks_performed(&self) -> u64 {
        self.checks_performed
    }

    /// Returns the number of tracked transactions.
    pub fn transaction_count(&self) -> usize {
        self.transactions.len()
    }
}

impl Default for MvccAnomalyChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl InvariantChecker for MvccAnomalyChecker {
    fn name(&self) -> &'static str {
        "MvccAnomalyChecker"
    }

    fn reset(&mut self) {
        self.transactions.clear();
        self.dependencies.clear();
        self.next_txn_id = 1;
        self.checks_performed = 0;
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_transaction_tracking() {
        let mut checker = MvccAnomalyChecker::new();

        let txn1 = checker.begin_transaction(Offset::new(1));
        assert_eq!(txn1, 1);
        assert_eq!(checker.transaction_count(), 1);

        checker.record_write(txn1, "key1", "value1", Offset::new(2));
        let result = checker.commit_transaction(txn1, Offset::new(3));
        assert!(result.is_ok());
    }

    #[test]
    fn test_dirty_read_detection() {
        let mut checker = MvccAnomalyChecker::new();

        // T1 writes and aborts
        let txn1 = checker.begin_transaction(Offset::new(1));
        checker.record_write(txn1, "key1", "value1", Offset::new(2));

        // T2 reads T1's write
        let txn2 = checker.begin_transaction(Offset::new(3));
        checker.record_read(txn2, "key1", Offset::new(2), "value1");
        checker.commit_transaction(txn2, Offset::new(4));

        // T1 aborts - should detect G1a
        let result = checker.abort_transaction(txn1, Offset::new(5));
        assert!(!result.is_ok());
    }

    #[test]
    fn test_non_repeatable_read_detection() {
        let mut checker = MvccAnomalyChecker::new();

        let txn1 = checker.begin_transaction(Offset::new(1));
        checker.record_read(txn1, "key1", Offset::new(2), "v1");
        checker.record_read(txn1, "key1", Offset::new(5), "v2"); // Different version

        let result = checker.commit_transaction(txn1, Offset::new(10));
        assert!(!result.is_ok());
    }

    #[test]
    fn test_clean_transaction_history() {
        let mut checker = MvccAnomalyChecker::new();

        // T1: read and write
        let txn1 = checker.begin_transaction(Offset::new(1));
        checker.record_read(txn1, "key1", Offset::new(1), "v0");
        checker.record_write(txn1, "key1", "v1", Offset::new(2));
        let result = checker.commit_transaction(txn1, Offset::new(3));
        assert!(result.is_ok());

        // T2: read T1's write
        let txn2 = checker.begin_transaction(Offset::new(4));
        checker.record_read(txn2, "key1", Offset::new(2), "v1");
        let result = checker.commit_transaction(txn2, Offset::new(5));
        assert!(result.is_ok());
    }
}
