//! Kimberlite oracle implementation for differential testing.
//!
//! This module provides a wrapper around Kimberlite's query engine for
//! differential testing against DuckDB.

use kimberlite_query::QueryResult;

use crate::{OracleError, OracleRunner};

/// Kimberlite oracle for differential testing.
///
/// This is a placeholder implementation. When wired into VOPR, it will
/// execute queries against an actual Kimberlite instance.
pub struct KimberliteOracle {
    // TODO: Add Kimberlite instance handle when integrating with VOPR
}

impl KimberliteOracle {
    /// Creates a new Kimberlite oracle.
    ///
    /// This is a placeholder. When implementing VOPR integration, this will
    /// initialize a real Kimberlite instance.
    #[allow(dead_code)]
    pub fn new() -> Result<Self, OracleError> {
        Ok(Self {})
    }
}

impl OracleRunner for KimberliteOracle {
    fn execute(&mut self, _sql: &str) -> Result<QueryResult, OracleError> {
        // TODO: Implement when integrating with VOPR
        // This will call into kimberlite_query::executor
        Err(OracleError::Unsupported(
            "KimberliteOracle is a placeholder - implement when wiring into VOPR".to_string(),
        ))
    }

    fn reset(&mut self) -> Result<(), OracleError> {
        // TODO: Implement when integrating with VOPR
        // This will reset Kimberlite's state (clear tables, etc.)
        Ok(())
    }

    fn name(&self) -> &'static str {
        "Kimberlite"
    }
}
