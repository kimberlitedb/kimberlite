//! Realistic workload generators for simulation testing.
//!
//! This module provides workload generators that mimic real-world application
//! patterns, enabling more thorough testing than simple synthetic workloads.
//!
//! ## Workload Types
//!
//! - **Multi-tenant**: Hot tenants, bursty spikes, varied traffic patterns
//! - **Read-Modify-Write**: Atomic RMW chains with conflicts
//! - **Long transactions**: Multi-operation transactions with rollback
//! - **Scan-heavy**: Range queries over large datasets
//! - **Hotspot contention**: 20% keys get 80% traffic (Pareto distribution)

use crate::SimRng;
use serde::{Deserialize, Serialize};

// ============================================================================
// Workload Configuration
// ============================================================================

/// Configuration for a realistic workload.
#[derive(Debug, Clone)]
pub struct WorkloadConfig {
    /// Number of tenants.
    pub num_tenants: usize,

    /// Number of operations to generate.
    pub num_operations: usize,

    /// Workload pattern.
    pub pattern: WorkloadPattern,

    /// Key space size.
    pub key_space_size: usize,

    /// Average transaction size (operations per transaction).
    pub avg_transaction_size: usize,
}

impl Default for WorkloadConfig {
    fn default() -> Self {
        Self {
            num_tenants: 1,
            num_operations: 1000,
            pattern: WorkloadPattern::Uniform,
            key_space_size: 10000,
            avg_transaction_size: 1,
        }
    }
}

/// Workload access patterns.
#[derive(Debug, Clone, Copy)]
pub enum WorkloadPattern {
    /// Uniform random access.
    Uniform,

    /// Hotspot: 20% keys get 80% traffic (Pareto distribution).
    Hotspot,

    /// Sequential scan pattern.
    Sequential,

    /// Multi-tenant with hot tenant (80% traffic to 1 tenant).
    MultiTenantHot,

    /// Bursty spikes (10x traffic for 100ms bursts).
    Bursty,

    /// Read-modify-write chains (read, modify, write).
    ReadModifyWrite,
}

// ============================================================================
// Operation Types
// ============================================================================

/// A generated workload operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadOp {
    /// Tenant ID.
    pub tenant_id: u64,

    /// Operation type.
    pub op_type: OpType,

    /// Key being accessed.
    pub key: String,

    /// Value (for writes).
    pub value: Option<Vec<u8>>,

    /// Transaction ID (operations in same transaction share ID).
    pub transaction_id: u64,

    /// Sequence number within transaction.
    pub seq_in_transaction: usize,
}

/// Types of operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpType {
    /// Read operation.
    Read,

    /// Write operation.
    Write,

    /// Read-modify-write (atomic).
    ReadModifyWrite,

    /// Range scan.
    Scan { limit: usize },

    /// Begin transaction.
    BeginTx,

    /// Commit transaction.
    CommitTx,

    /// Rollback transaction.
    RollbackTx,
}

// ============================================================================
// Workload Generator
// ============================================================================

/// Generates realistic workloads for testing.
pub struct WorkloadGenerator {
    /// Configuration.
    config: WorkloadConfig,

    /// Next transaction ID.
    next_tx_id: u64,
}

impl WorkloadGenerator {
    /// Creates a new workload generator.
    pub fn new(config: WorkloadConfig) -> Self {
        Self {
            config,
            next_tx_id: 0,
        }
    }

    /// Generates a complete workload.
    pub fn generate(&mut self, rng: &mut SimRng) -> Vec<WorkloadOp> {
        match self.config.pattern {
            WorkloadPattern::Uniform => self.generate_uniform(rng),
            WorkloadPattern::Hotspot => self.generate_hotspot(rng),
            WorkloadPattern::Sequential => self.generate_sequential(rng),
            WorkloadPattern::MultiTenantHot => self.generate_multi_tenant_hot(rng),
            WorkloadPattern::Bursty => self.generate_bursty(rng),
            WorkloadPattern::ReadModifyWrite => self.generate_rmw(rng),
        }
    }

    /// Generates uniform random access pattern.
    fn generate_uniform(&mut self, rng: &mut SimRng) -> Vec<WorkloadOp> {
        let mut ops = Vec::new();

        for _ in 0..self.config.num_operations {
            let tenant_id = rng.next_usize(self.config.num_tenants) as u64;
            let key = format!("key_{}", rng.next_usize(self.config.key_space_size));
            let op_type = if rng.next_bool() {
                OpType::Read
            } else {
                OpType::Write
            };

            ops.push(WorkloadOp {
                tenant_id,
                op_type,
                key,
                value: Some(self.generate_value(rng)),
                transaction_id: self.next_tx_id,
                seq_in_transaction: 0,
            });

            self.next_tx_id += 1;
        }

        ops
    }

    /// Generates hotspot pattern (80/20 rule).
    fn generate_hotspot(&mut self, rng: &mut SimRng) -> Vec<WorkloadOp> {
        let mut ops = Vec::new();
        let hot_keys = (self.config.key_space_size as f64 * 0.2) as usize;

        for _ in 0..self.config.num_operations {
            let tenant_id = rng.next_usize(self.config.num_tenants) as u64;

            // 80% access to 20% of keys
            let key = if rng.next_f64() < 0.8 {
                format!("key_{}", rng.next_usize(hot_keys))
            } else {
                format!("key_{}", rng.next_usize(self.config.key_space_size))
            };

            let op_type = if rng.next_bool() {
                OpType::Read
            } else {
                OpType::Write
            };

            ops.push(WorkloadOp {
                tenant_id,
                op_type,
                key,
                value: Some(self.generate_value(rng)),
                transaction_id: self.next_tx_id,
                seq_in_transaction: 0,
            });

            self.next_tx_id += 1;
        }

        ops
    }

    /// Generates sequential scan pattern.
    fn generate_sequential(&mut self, rng: &mut SimRng) -> Vec<WorkloadOp> {
        let mut ops = Vec::new();
        let mut current_key = 0;

        for _ in 0..self.config.num_operations {
            let tenant_id = rng.next_usize(self.config.num_tenants) as u64;
            let key = format!("key_{:08}", current_key);
            current_key = (current_key + 1) % self.config.key_space_size;

            // Mix of reads and scans
            let op_type = if rng.next_bool() {
                OpType::Read
            } else {
                OpType::Scan {
                    limit: rng.next_usize(100) + 1,
                }
            };

            ops.push(WorkloadOp {
                tenant_id,
                op_type,
                key,
                value: None,
                transaction_id: self.next_tx_id,
                seq_in_transaction: 0,
            });

            self.next_tx_id += 1;
        }

        ops
    }

    /// Generates multi-tenant workload with hot tenant.
    fn generate_multi_tenant_hot(&mut self, rng: &mut SimRng) -> Vec<WorkloadOp> {
        let mut ops = Vec::new();
        let hot_tenant = 0u64;

        for _ in 0..self.config.num_operations {
            // 80% traffic to hot tenant
            let tenant_id = if rng.next_f64() < 0.8 {
                hot_tenant
            } else {
                rng.next_usize(self.config.num_tenants) as u64
            };

            let key = format!("key_{}", rng.next_usize(self.config.key_space_size));
            let op_type = if rng.next_bool() {
                OpType::Read
            } else {
                OpType::Write
            };

            ops.push(WorkloadOp {
                tenant_id,
                op_type,
                key,
                value: Some(self.generate_value(rng)),
                transaction_id: self.next_tx_id,
                seq_in_transaction: 0,
            });

            self.next_tx_id += 1;
        }

        ops
    }

    /// Generates bursty traffic with 10x spikes.
    fn generate_bursty(&mut self, rng: &mut SimRng) -> Vec<WorkloadOp> {
        let mut ops = Vec::new();
        let mut in_burst = false;
        let mut burst_ops_remaining = 0;

        for i in 0..self.config.num_operations {
            // Start burst every ~1000 ops
            if i % 1000 == 0 && rng.next_bool_with_probability(0.3) {
                in_burst = true;
                burst_ops_remaining = 100; // 10x for 100ms
            }

            if in_burst {
                burst_ops_remaining -= 1;
                if burst_ops_remaining == 0 {
                    in_burst = false;
                }
            }

            let tenant_id = rng.next_usize(self.config.num_tenants) as u64;
            let key = format!("key_{}", rng.next_usize(self.config.key_space_size));
            let op_type = OpType::Write;

            ops.push(WorkloadOp {
                tenant_id,
                op_type,
                key,
                value: Some(self.generate_value(rng)),
                transaction_id: self.next_tx_id,
                seq_in_transaction: 0,
            });

            self.next_tx_id += 1;

            // In burst mode, generate 10x ops
            if in_burst {
                for _ in 0..9 {
                    ops.push(WorkloadOp {
                        tenant_id,
                        op_type,
                        key: format!("key_{}", rng.next_usize(self.config.key_space_size)),
                        value: Some(self.generate_value(rng)),
                        transaction_id: self.next_tx_id,
                        seq_in_transaction: 0,
                    });
                    self.next_tx_id += 1;
                }
            }
        }

        ops
    }

    /// Generates read-modify-write chains.
    fn generate_rmw(&mut self, rng: &mut SimRng) -> Vec<WorkloadOp> {
        let mut ops = Vec::new();

        while ops.len() < self.config.num_operations {
            let tenant_id = rng.next_usize(self.config.num_tenants) as u64;
            let key = format!("key_{}", rng.next_usize(self.config.key_space_size));
            let tx_id = self.next_tx_id;
            self.next_tx_id += 1;

            // Begin transaction
            ops.push(WorkloadOp {
                tenant_id,
                op_type: OpType::BeginTx,
                key: String::new(),
                value: None,
                transaction_id: tx_id,
                seq_in_transaction: 0,
            });

            // Read
            ops.push(WorkloadOp {
                tenant_id,
                op_type: OpType::Read,
                key: key.clone(),
                value: None,
                transaction_id: tx_id,
                seq_in_transaction: 1,
            });

            // Modify (write)
            ops.push(WorkloadOp {
                tenant_id,
                op_type: OpType::Write,
                key: key.clone(),
                value: Some(self.generate_value(rng)),
                transaction_id: tx_id,
                seq_in_transaction: 2,
            });

            // Commit (or sometimes rollback)
            let commit = rng.next_bool_with_probability(0.9);
            ops.push(WorkloadOp {
                tenant_id,
                op_type: if commit {
                    OpType::CommitTx
                } else {
                    OpType::RollbackTx
                },
                key: String::new(),
                value: None,
                transaction_id: tx_id,
                seq_in_transaction: 3,
            });
        }

        ops
    }

    /// Generates a random value.
    fn generate_value(&self, rng: &mut SimRng) -> Vec<u8> {
        let size = rng.next_usize(100) + 10; // 10-110 bytes
        let mut value = vec![0u8; size];
        for byte in &mut value {
            *byte = rng.next_u32() as u8;
        }
        value
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_uniform_workload() {
        let config = WorkloadConfig {
            num_operations: 100,
            ..Default::default()
        };
        let mut generator = WorkloadGenerator::new(config);
        let mut rng = SimRng::new(42);

        let ops = generator.generate(&mut rng);

        assert_eq!(ops.len(), 100);
    }

    #[test]
    fn generate_hotspot_workload() {
        let config = WorkloadConfig {
            num_operations: 100,
            pattern: WorkloadPattern::Hotspot,
            ..Default::default()
        };
        let mut generator = WorkloadGenerator::new(config);
        let mut rng = SimRng::new(42);

        let ops = generator.generate(&mut rng);

        assert_eq!(ops.len(), 100);
    }

    #[test]
    fn generate_rmw_workload() {
        let config = WorkloadConfig {
            num_operations: 100,
            pattern: WorkloadPattern::ReadModifyWrite,
            ..Default::default()
        };
        let mut generator = WorkloadGenerator::new(config);
        let mut rng = SimRng::new(42);

        let ops = generator.generate(&mut rng);

        // RMW generates 4 ops per transaction (begin, read, write, commit/rollback)
        assert!(ops.len() >= 100);

        // Check that transactions are structured correctly
        let has_begin = ops.iter().any(|op| matches!(op.op_type, OpType::BeginTx));
        assert!(has_begin);
    }

    #[test]
    fn multi_tenant_hot_tenant() {
        let config = WorkloadConfig {
            num_tenants: 5,
            num_operations: 1000,
            pattern: WorkloadPattern::MultiTenantHot,
            ..Default::default()
        };
        let mut generator = WorkloadGenerator::new(config);
        let mut rng = SimRng::new(42);

        let ops = generator.generate(&mut rng);

        // Count operations per tenant
        let mut tenant_counts = [0; 5];
        for op in &ops {
            tenant_counts[op.tenant_id as usize] += 1;
        }

        // Hot tenant (0) should have significantly more traffic
        assert!(tenant_counts[0] > tenant_counts[1] * 2);
    }
}
