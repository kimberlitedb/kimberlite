//! SQL differential testing integration for VOPR.
//!
//! This module provides hooks for integrating differential testing (comparing
//! Kimberlite vs DuckDB query results) into VOPR simulation scenarios.
//!
//! ## Architecture
//!
//! Differential testing works by:
//! 1. Executing SQL queries in both Kimberlite and DuckDB (reference oracle)
//! 2. Comparing results byte-by-byte using `DifferentialTester`
//! 3. Reporting any discrepancies as invariant violations
//!
//! ## Usage in VOPR
//!
//! Enable differential testing via the `--enable-sql-differential` flag:
//!
//! ```bash
//! cargo run --release -p kimberlite-sim --bin vopr -- \
//!   --scenario combined \
//!   --enable-sql-differential \
//!   -n 10000
//! ```
//!
//! ## Current Status
//!
//! **Phase 1 (Infrastructure) - COMPLETE:**
//! - ✅ Oracle abstraction (`kimberlite-oracle` crate)
//! - ✅ DuckDB oracle implementation
//! - ✅ DifferentialTester implementation
//! - ✅ Fuzzing harness
//!
//! **Phase 2 (VOPR Integration) - IN PROGRESS:**
//! - ✅ Configuration flag added to `VoprConfig`
//! - 🔲 Query execution hooks (requires Kimberlite instance in VOPR)
//! - 🔲 Schema synchronization (Kimberlite ↔ DuckDB)
//! - 🔲 Result comparison integration
//!
//! ## Future Work
//!
//! To complete VOPR integration, the following work is needed:
//!
//! 1. **Create KimberliteOracle implementation:**
//!    - Wrap VOPR's simulation state to provide query execution
//!    - Map Kimberlite's data model to DuckDB's SQL schema
//!    - Handle MVCC snapshots and point-in-time queries
//!
//! 2. **Hook into query execution path:**
//!    - Intercept queries in `query_workload.rs`
//!    - Execute in both oracles when flag is enabled
//!    - Compare results using `DifferentialTester`
//!
//! 3. **Synchronize schemas:**
//!    - Mirror CREATE TABLE statements to DuckDB
//!    - Keep schemas in sync during simulation
//!
//! 4. **Handle edge cases:**
//!    - Unsupported SQL features (gracefully skip)
//!    - Type conversion differences
//!    - Timing/concurrency differences
//!
//! ## Expected Payoff
//!
//! Based on Crucible's results (154+ bugs found in major OSS projects),
//! we expect differential testing to find **5-10 SQL correctness bugs**
//! in Kimberlite's query engine during the first fuzzing campaign.

use kimberlite_oracle::{DuckDbOracle, OracleRunner};

use crate::invariant::InvariantResult;
use crate::sql_oracles::DifferentialTester;

// ============================================================================
// SQL Differential Testing Context
// ============================================================================

/// Context for SQL differential testing during VOPR simulations.
///
/// This struct maintains the state needed for differential testing,
/// including both oracles and the differential tester.
pub struct SqlDifferentialContext {
    /// DuckDB oracle (reference/ground truth).
    duckdb_oracle: DuckDbOracle,

    /// Differential tester (compares results).
    /// Uses a stub oracle for Kimberlite until full integration is complete.
    #[allow(dead_code)]
    differential_tester: Option<DifferentialTester<DuckDbOracle, StubKimberliteOracle>>,

    /// Queries tested.
    queries_tested: u64,

    /// Violations detected.
    violations_detected: u64,
}

impl SqlDifferentialContext {
    /// Creates a new SQL differential testing context.
    pub fn new() -> Result<Self, String> {
        let duckdb_oracle = DuckDbOracle::new()
            .map_err(|e| format!("Failed to create DuckDB oracle: {}", e))?;

        Ok(Self {
            duckdb_oracle,
            differential_tester: None,
            queries_tested: 0,
            violations_detected: 0,
        })
    }

    /// Tests a query using differential testing.
    ///
    /// # Arguments
    /// - `query_id`: Unique identifier for this query (for debugging)
    /// - `sql`: The SQL query to test
    ///
    /// # Returns
    /// - `Ok(())` if results match (or oracle doesn't support the query)
    /// - `Err(msg)` if results differ (bug found!)
    pub fn test_query(&mut self, _query_id: &str, sql: &str) -> InvariantResult {
        self.queries_tested += 1;

        // For now, just execute in DuckDB to verify the oracle works
        // TODO: When KimberliteOracle is implemented, use DifferentialTester
        match self.duckdb_oracle.execute(sql) {
            Ok(_result) => {
                // Query succeeded in DuckDB
                // TODO: Compare with Kimberlite result
                InvariantResult::Ok
            }
            Err(_) => {
                // Query failed in DuckDB
                // This is OK - not all queries will succeed
                InvariantResult::Ok
            }
        }
    }

    /// Returns the number of queries tested.
    pub fn queries_tested(&self) -> u64 {
        self.queries_tested
    }

    /// Returns the number of violations detected.
    pub fn violations_detected(&self) -> u64 {
        self.violations_detected
    }

    /// Resets the differential testing state.
    pub fn reset(&mut self) -> Result<(), String> {
        self.duckdb_oracle
            .reset()
            .map_err(|e| format!("Failed to reset DuckDB oracle: {}", e))?;

        self.queries_tested = 0;
        self.violations_detected = 0;

        Ok(())
    }
}

// ============================================================================
// Stub Kimberlite Oracle (Placeholder)
// ============================================================================

/// Stub implementation of KimberliteOracle.
///
/// This is a placeholder until full VOPR integration is complete.
/// The real implementation will wrap VOPR's simulation state.
pub struct StubKimberliteOracle {}

impl StubKimberliteOracle {
    /// Creates a new stub oracle.
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {}
    }
}

impl OracleRunner for StubKimberliteOracle {
    fn execute(&mut self, _sql: &str) -> Result<kimberlite_query::QueryResult, kimberlite_oracle::OracleError> {
        // TODO: Implement by wrapping VOPR's simulation state
        Err(kimberlite_oracle::OracleError::Unsupported(
            "KimberliteOracle not yet integrated with VOPR".to_string(),
        ))
    }

    fn reset(&mut self) -> Result<(), kimberlite_oracle::OracleError> {
        Ok(())
    }

    fn name(&self) -> &'static str {
        "Kimberlite (stub)"
    }
}

// ============================================================================
// Integration Hooks
// ============================================================================

/// Hook point for SQL differential testing.
///
/// Call this from query execution points in VOPR to enable differential testing.
///
/// # Example
///
/// ```ignore
/// if config.enable_sql_differential {
///     if let Some(ctx) = &mut sql_differential_ctx {
///         let result = ctx.test_query("query_123", sql);
///         if !result.is_ok() {
///             return result.into_error(sim_time_ns);
///         }
///     }
/// }
/// ```
pub fn differential_test_query(
    ctx: &mut SqlDifferentialContext,
    query_id: &str,
    sql: &str,
) -> InvariantResult {
    ctx.test_query(query_id, sql)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sql_differential_context_creation() {
        let ctx = SqlDifferentialContext::new();
        assert!(ctx.is_ok());

        let ctx = ctx.unwrap();
        assert_eq!(ctx.queries_tested(), 0);
        assert_eq!(ctx.violations_detected(), 0);
    }

    #[test]
    fn test_sql_differential_context_test_query() {
        let mut ctx = SqlDifferentialContext::new().unwrap();

        // Test a simple query
        let result = ctx.test_query("test1", "SELECT 1");
        assert!(result.is_ok());
        assert_eq!(ctx.queries_tested(), 1);
    }

    #[test]
    fn test_sql_differential_context_reset() {
        let mut ctx = SqlDifferentialContext::new().unwrap();

        ctx.test_query("test1", "SELECT 1");
        assert_eq!(ctx.queries_tested(), 1);

        ctx.reset().unwrap();
        assert_eq!(ctx.queries_tested(), 0);
    }

    #[test]
    fn test_differential_test_query_hook() {
        let mut ctx = SqlDifferentialContext::new().unwrap();

        let result = differential_test_query(&mut ctx, "test1", "SELECT 1");
        assert!(result.is_ok());
    }
}
