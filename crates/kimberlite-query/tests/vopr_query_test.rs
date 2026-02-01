//! VOPR integration tests for the query engine.
//!
//! These tests demonstrate the use of VOPR simulation infrastructure with
//! query-specific invariant checkers. They verify correctness properties:
//! - Query determinism (same inputs → same outputs)
//! - Read-your-writes consistency
//! - Type safety across operations
//! - ORDER BY + LIMIT correctness
//! - Aggregate function correctness
//!
//! To run these tests:
//! ```bash
//! cargo test --package kmb-query --test vopr_query_test
//! ```
//!
//! To run with specific seeds:
//! ```bash
//! cargo test --package kmb-query --test vopr_query_test -- --ignored
//! ```

use kimberlite_sim::query_workload::{QueryWorkloadGenerator, TableSchema};
use kimberlite_sim::{
    AggregateCorrectnessChecker, OrderByLimitChecker, QueryDeterminismChecker,
    ReadYourWritesChecker, TypeSafetyChecker,
};

/// Demonstrate query determinism checker.
///
/// Verifies that running the same queries multiple times produces identical results.
#[test]
fn test_query_determinism_checker() {
    let mut checker = QueryDeterminismChecker::new();

    let query = "SELECT * FROM users WHERE age > $1 ORDER BY id";
    let params = vec!["25".to_string()];
    let result1 = "[row1, row2, row3]";

    // First execution - establishes baseline
    let check1 = checker.check_query(query, &params, result1);
    assert!(check1.is_ok());

    // Second execution - same result should pass
    let check2 = checker.check_query(query, &params, result1);
    assert!(check2.is_ok());

    // Third execution - different result should fail
    let result2 = "[row1, row4, row5]";
    let check3 = checker.check_query(query, &params, result2);
    assert!(
        !check3.is_ok(),
        "Different results should violate determinism"
    );

    // Only 2 queries were checked because the third one failed early
    assert_eq!(checker.queries_checked(), 2);
    assert_eq!(checker.unique_queries(), 1);
}

/// Demonstrate read-your-writes checker.
///
/// Verifies that writes are immediately visible to subsequent reads.
#[test]
fn test_read_your_writes_checker() {
    let mut checker = ReadYourWritesChecker::new();

    // Record a write
    checker.record_write("users", "1", Some("Alice"));

    // Verify read sees the write
    let check1 = checker.verify_read("users", "1", Some("Alice"));
    assert!(check1.is_ok());

    // Verify read with wrong value fails
    let check2 = checker.verify_read("users", "1", Some("Bob"));
    assert!(
        !check2.is_ok(),
        "Wrong value should violate read-your-writes"
    );

    // Record a delete (write with None)
    checker.record_write("users", "2", None);

    // Verify read sees the delete
    let check3 = checker.verify_read("users", "2", None);
    assert!(check3.is_ok());

    // Verify read with value when deleted fails
    let check4 = checker.verify_read("users", "2", Some("Charlie"));
    assert!(!check4.is_ok(), "Reading value after delete should fail");

    assert_eq!(checker.writes_tracked(), 2);
    // Only 2 reads verified (check1 and check3) - check2 and check4 failed early
    assert_eq!(checker.reads_verified(), 2);
}

/// Demonstrate type safety checker.
#[test]
fn test_type_safety_checker() {
    let mut checker = TypeSafetyChecker::new();

    // Register schema
    checker.register_table(
        "users",
        &[
            ("id".to_string(), "BIGINT".to_string()),
            ("name".to_string(), "TEXT".to_string()),
            ("age".to_string(), "INTEGER".to_string()),
        ],
    );

    // Verify correct types
    assert!(checker.verify_type("users", "id", "BIGINT").is_ok());
    assert!(checker.verify_type("users", "name", "TEXT").is_ok());

    // Numeric coercion should work
    assert!(checker.verify_type("users", "age", "BIGINT").is_ok());

    // Type mismatch should fail
    assert!(!checker.verify_type("users", "name", "BIGINT").is_ok());

    // Only 3 checks performed - the 4th one failed early
    assert_eq!(checker.checks_performed(), 3);
}

/// Demonstrate ORDER BY + LIMIT correctness checker.
#[test]
fn test_order_by_limit_checker() {
    let mut checker = OrderByLimitChecker::new();

    // Simulate full sorted result
    let full_rows = vec![
        "row1".to_string(),
        "row2".to_string(),
        "row3".to_string(),
        "row4".to_string(),
        "row5".to_string(),
    ];
    checker.record_full_result("test_query", full_rows);

    // Verify correct limited result (first 3 rows)
    let correct_limited = vec!["row1".to_string(), "row2".to_string(), "row3".to_string()];
    let check1 = checker.verify_limited_result("test_query", &correct_limited, 3);
    assert!(check1.is_ok());

    // Verify incorrect limited result (wrong rows)
    let wrong_limited = vec!["row2".to_string(), "row3".to_string(), "row4".to_string()];
    let check2 = checker.verify_limited_result("test_query", &wrong_limited, 3);
    assert!(
        !check2.is_ok(),
        "Wrong rows should violate ORDER BY + LIMIT correctness"
    );

    // Only 1 check performed - the 2nd one failed early
    assert_eq!(checker.checks_performed(), 1);
}

/// Demonstrate aggregate correctness checker.
#[test]
fn test_aggregate_correctness_checker() {
    let mut checker = AggregateCorrectnessChecker::new();

    // Record table data
    let rows = vec!["row1".to_string(), "row2".to_string(), "row3".to_string()];
    checker.record_table_data("users", rows);

    // Verify correct COUNT(*)
    let check1 = checker.verify_count("users", 3);
    assert!(check1.is_ok());

    // Verify incorrect COUNT(*)
    let check2 = checker.verify_count("users", 5);
    assert!(
        !check2.is_ok(),
        "Wrong count should violate aggregate correctness"
    );

    // Verify SUM correctness
    let check3 = checker.verify_sum("sum_query", 15, 15);
    assert!(check3.is_ok());

    let check4 = checker.verify_sum("sum_query", 20, 15);
    assert!(
        !check4.is_ok(),
        "Wrong sum should violate aggregate correctness"
    );

    // Only 2 checks performed (check1 and check3) - check2 and check4 failed early
    assert_eq!(checker.checks_performed(), 2);
}

/// Demonstrate workload generator.
///
/// Shows how to use the workload generator to create deterministic SQL queries.
#[test]
fn test_workload_generator() {
    let mut generator = QueryWorkloadGenerator::new(42);

    // Add schema
    generator.add_schema(TableSchema::new(
        "users",
        vec![("id", "BIGINT"), ("name", "TEXT"), ("age", "INTEGER")],
        vec!["id"],
    ));

    // Generate specific query types
    let insert = generator.generate_insert().expect("Should generate INSERT");
    assert!(insert.starts_with("INSERT INTO users VALUES ("));

    let select = generator.generate_select().expect("Should generate SELECT");
    assert!(select.starts_with("SELECT"));
    assert!(select.contains("FROM users"));

    let update = generator.generate_update().expect("Should generate UPDATE");
    assert!(update.starts_with("UPDATE users SET"));

    let delete = generator.generate_delete().expect("Should generate DELETE");
    assert!(delete.starts_with("DELETE FROM users"));

    let aggregate = generator
        .generate_aggregate()
        .expect("Should generate aggregate");
    assert!(aggregate.starts_with("SELECT"));
    assert!(
        aggregate.contains("COUNT")
            || aggregate.contains("SUM")
            || aggregate.contains("MIN")
            || aggregate.contains("MAX")
    );

    // Generate mixed workload
    let queries = generator.generate_mixed_workload(20);
    assert_eq!(queries.len(), 20);

    // Verify determinism - use fresh generators with same seed
    let mut fresh_generator1 = QueryWorkloadGenerator::new(99);
    fresh_generator1.add_schema(TableSchema::new(
        "users",
        vec![("id", "BIGINT"), ("name", "TEXT"), ("age", "INTEGER")],
        vec!["id"],
    ));
    let queries_a = fresh_generator1.generate_mixed_workload(20);

    let mut fresh_generator2 = QueryWorkloadGenerator::new(99);
    fresh_generator2.add_schema(TableSchema::new(
        "users",
        vec![("id", "BIGINT"), ("name", "TEXT"), ("age", "INTEGER")],
        vec!["id"],
    ));
    let queries_b = fresh_generator2.generate_mixed_workload(20);

    assert_eq!(
        queries_a, queries_b,
        "Same seed should produce same queries"
    );
}

/// Example of how VOPR seed-based regression tests would work.
///
/// When simulation testing finds a bug with a specific seed, add a regression
/// test that replays that exact seed to prevent regressions.
#[test]
#[ignore = "This is a template for future regression tests"]
fn example_vopr_regression_test_template() {
    const SEED: u64 = 12345; // Replace with actual seed that found a bug

    // In a real test, you would:
    // 1. Create query engine with specific schema
    // 2. Create workload generator with seed
    // 3. Run queries and track state
    // 4. Apply invariant checkers to verify correctness
    // 5. Assert no violations occur

    // Example pattern:
    let mut generator = QueryWorkloadGenerator::new(SEED);
    let _determinism_checker = QueryDeterminismChecker::new();

    // Add schema matching the bug scenario
    generator.add_schema(TableSchema::new(
        "test_table",
        vec![("id", "BIGINT"), ("value", "TEXT")],
        vec!["id"],
    ));

    // Generate and execute queries
    for _query in generator.generate_mixed_workload(100) {
        // In real test: execute query and verify with checkers
        // let result = engine.query(&mut store, &query, &[]);
        // assert!(determinism_checker.check_query(...).is_ok());
    }
}

/// Full integration example showing all components working together.
#[test]
#[ignore = "Integration test template"]
fn example_full_vopr_integration() {
    // This demonstrates the complete pattern for VOPR testing:
    //
    // 1. Set up schema
    // 2. Create invariant checkers
    // 3. Generate workload
    // 4. Execute queries
    // 5. Verify invariants
    //
    // For a real implementation, this would:
    // - Use actual QueryEngine and store
    // - Execute generated SQL
    // - Track all state changes
    // - Apply all invariant checkers
    // - Report any violations with full context

    let seed = 99;
    let mut generator = QueryWorkloadGenerator::new(seed);

    // Initialize checkers
    let determinism = QueryDeterminismChecker::new();
    let ryw = ReadYourWritesChecker::new();
    let mut type_safety = TypeSafetyChecker::new();

    // Register schema
    type_safety.register_table(
        "users",
        &[
            ("id".to_string(), "BIGINT".to_string()),
            ("name".to_string(), "TEXT".to_string()),
        ],
    );

    generator.add_schema(TableSchema::new(
        "users",
        vec![("id", "BIGINT"), ("name", "TEXT")],
        vec!["id"],
    ));

    // In real test: execute queries and verify invariants
    let _queries = generator.generate_mixed_workload(100);

    // Verify all checkers initialized (counters exist)
    let _ = determinism.queries_checked();
    let _ = ryw.writes_tracked();
    let _ = type_safety.checks_performed();
}
