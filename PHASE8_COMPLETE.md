# Phase 8 Complete: SQL Metamorphic Testing

## Summary

Phase 8 has been successfully implemented. Kimberlite now has SQLancer-inspired metamorphic testing oracles for detecting SQL logic bugs through automated test case generation and result comparison. These oracles verify query correctness without requiring manual test case creation.

---

## Deliverables ✅

### Task #1: SQL Testing Oracles Module ✅

**Module Created**: `/crates/kimberlite-sim/src/sql_oracles.rs` (630+ lines)

**Purpose**: Automated SQL correctness testing using metamorphic relations

**Oracles Implemented** (2 testing approaches + coverage tracking):

1. **TLP (Ternary Logic Partitioning) Oracle**
   - **Principle**: Partition WHERE clause into TRUE/FALSE/NULL partitions
   - **Invariant**: `COUNT(TRUE) + COUNT(FALSE) + COUNT(NULL) = COUNT(ALL)`
   - **Example**:
     ```sql
     -- Original query
     SELECT * FROM users WHERE age > 30  -- Returns 100 rows

     -- Partitioned queries (must sum to 100)
     SELECT * FROM users WHERE age > 30                -- TRUE: 80 rows
     SELECT * FROM users WHERE NOT (age > 30)          -- FALSE: 15 rows
     SELECT * FROM users WHERE (age > 30) IS NULL      -- NULL: 5 rows
     -- Total: 80 + 15 + 5 = 100 ✓
     ```
   - **Catches**: NULL handling bugs, boolean logic errors, type coercion bugs
   - **Inspired by**: SQLancer (Rigger & Su, 2020)

2. **NoREC (Non-optimizing Reference Engine Comparison) Oracle**
   - **Principle**: Execute same query with and without optimizations, compare results
   - **Invariant**: Optimized execution ≡ Unoptimized execution (same results)
   - **Example**:
     ```sql
     -- Optimized: uses index scan on age_idx
     SELECT * FROM users WHERE age > 30 ORDER BY age LIMIT 10
     -- Results: [user1, user2, ..., user10]

     -- Unoptimized: forces table scan, no index
     SELECT * FROM users WHERE age > 30 ORDER BY age LIMIT 10
     -- Results: [user1, user2, ..., user10]
     -- Must match optimized results!
     ```
   - **Catches**: Optimizer bugs (wrong index usage, incorrect predicate pushdown, bad join reordering)
   - **Inspired by**: NoREC (Chen et al.)

3. **Query Plan Coverage Tracker**
   - **Principle**: Track unique query plans executed, detect coverage plateaus
   - **Coverage Metric**: Number of unique plan signatures seen
   - **Plan Signature**: Hash of (plan_type, table, index, has_filter, has_limit, has_order_by, aggregate)
   - **Plateau Detection**: If no new plans in N queries, trigger database mutation
   - **Guides**: When to INSERT/DELETE/CREATE INDEX/UPDATE to increase coverage

---

### Task #2: Database State Mutators ✅

**Purpose**: Automatically generate database mutations to increase test coverage

**Mutation Actions** (inspired by SQLancer):

```rust
pub enum DatabaseAction {
    /// Insert random row
    InsertRandomRow { table: String },

    /// Insert special values (NULL, min, max, empty, duplicate)
    InsertSpecialRow { table: String, value_type: SpecialValueType },

    /// Update random rows
    UpdateRandomRows { table: String, count: usize },

    /// Delete random rows
    DeleteRandomRows { table: String, count: usize },

    /// Create secondary index
    CreateIndex { table: String, column: String },

    /// Drop index
    DropIndex { table: String, index: String },

    /// Analyze table statistics
    AnalyzeTable { table: String },
}
```

**Special Value Types**:
```rust
pub enum SpecialValueType {
    AllNulls,      // Test NULL handling
    MinValues,     // Test boundary conditions
    MaxValues,     // Test boundary conditions
    Empty,         // Test empty strings/zero values
    Duplicate,     // Test uniqueness constraints
}
```

**Mutation Strategy**:
```rust
pub fn select_next_action(
    unique_plan_count: usize,
    row_count: usize,
    index_count: usize,
    queries_without_new_plan: u64,
) -> Option<DatabaseAction>
```

**Strategy Rules**:
1. If coverage not plateaued (< 100 queries without new plan) → keep querying
2. If plateaued + low row count (< 10) → INSERT rows
3. If plateaued + no indexes → CREATE INDEX
4. If plateaued + many rows (> 100) → DELETE rows
5. Default → UPDATE rows to create variety

---

### Task #3: API Design ✅

**TLP Oracle API**:
```rust
impl TlpOracle {
    pub fn new() -> Self;

    pub fn verify_partitioning(
        &mut self,
        query_id: &str,
        original_count: usize,
        true_partition_count: usize,
        false_partition_count: usize,
        null_partition_count: usize,
    ) -> InvariantResult;

    pub fn queries_checked(&self) -> u64;
    pub fn violations_detected(&self) -> u64;
    pub fn reset(&mut self);
}
```

**NoREC Oracle API**:
```rust
impl NoRecOracle {
    pub fn new() -> Self;

    pub fn verify_optimization(
        &mut self,
        query_id: &str,
        optimized_result_hash: u64,
        unoptimized_result_hash: u64,
    ) -> InvariantResult;

    pub fn comparisons_performed(&self) -> u64;
    pub fn violations_detected(&self) -> u64;
    pub fn reset(&mut self);
}
```

**Coverage Tracker API**:
```rust
impl QueryPlanCoverageTracker {
    pub fn new(plateau_threshold: u64) -> Self;

    pub fn record_plan(&mut self, plan_signature: u64) -> bool;
    pub fn is_coverage_plateaued(&self) -> bool;
    pub fn unique_plan_count(&self) -> usize;
    pub fn queries_executed(&self) -> u64;
    pub fn reset_plateau_counter(&mut self);
    pub fn reset(&mut self);
}
```

**Plan Signature Computation**:
```rust
pub fn compute_plan_signature(
    plan_type: &str,
    table_name: &str,
    index_name: Option<&str>,
    has_filter: bool,
    has_limit: bool,
    has_order_by: bool,
    aggregate_function: Option<&str>,
) -> u64
```

**Mutation Selection**:
```rust
pub fn select_next_action(
    unique_plan_count: usize,
    row_count: usize,
    index_count: usize,
    queries_without_new_plan: u64,
) -> Option<DatabaseAction>
```

---

### Task #4: Integration with Existing Infrastructure ✅

**Invariant Tracking**:
- Both oracles call `invariant_tracker::record_invariant_execution()`
- Tracked names:
  - `sql_tlp_partitioning`
  - `sql_norec_consistency`
- Enables coverage reporting in VOPR

**Uses Standard Types**:
- `InvariantResult` from `invariant.rs`
- `HashSet` for unique plan tracking
- `DefaultHasher` for deterministic plan signatures

---

### Task #5: Comprehensive Testing ✅

**Tests Added** (11 total in `sql_oracles.rs`):

1. **test_tlp_oracle_ok**
   - Original: 100 rows, Partitions: 60+35+5 = 100 → OK
   - Verifies normal TLP case

2. **test_tlp_oracle_violation**
   - Original: 100 rows, Partitions: 60+35+3 = 98 → VIOLATION
   - Catches partition sum mismatch

3. **test_norec_oracle_ok**
   - Optimized and unoptimized have same hash → OK
   - Normal case

4. **test_norec_oracle_violation**
   - Different hashes → VIOLATION
   - Catches optimizer bugs

5. **test_query_plan_coverage_tracker**
   - Records new plans, tracks duplicates
   - Detects plateau after threshold

6. **test_compute_plan_signature**
   - Different plan types → different signatures
   - Same plan → same signature
   - Verifies deterministic hashing

7. **test_select_next_action_low_rows**
   - Low row count → suggests INSERT
   - Verifies mutation strategy

8. **test_select_next_action_no_index**
   - No indexes → suggests CREATE INDEX
   - Coverage-driven action selection

9. **test_select_next_action_many_rows**
   - Many rows → suggests DELETE
   - Balances row count

10. **test_select_next_action_not_plateaued**
    - Coverage still improving → no action
    - Don't mutate prematurely

11. **test_all_sql_oracles_track_execution**
    - Verifies invariant_tracker integration
    - Both oracles tracked

**All tests pass** (225/225 in kimberlite-sim, up from 214)

---

## Architecture

```
┌────────────────────────────────────────────────────────────┐
│  SQL Query Engine (kimberlite-query)                       │
│  - Parser → AST                                            │
│  - Planner → QueryPlan (PointLookup, RangeScan, etc.)     │
│  - Executor → Results                                      │
└────────────────┬───────────────────────────────────────────┘
                 │ Execute queries, record plans
                 ▼
┌────────────────────────────────────────────────────────────┐
│  SQL Metamorphic Testing Oracles (kimberlite-sim)         │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ TlpOracle                                            │  │
│  │  - verify_partitioning(orig, true, false, null)     │  │
│  │  - Detects: NULL bugs, boolean logic errors         │  │
│  └──────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ NoRecOracle                                          │  │
│  │  - verify_optimization(optimized_hash, unopt_hash)  │  │
│  │  - Detects: optimizer bugs, wrong index usage       │  │
│  └──────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ QueryPlanCoverageTracker                             │  │
│  │  - record_plan(signature)                           │  │
│  │  - is_coverage_plateaued() → trigger mutation       │  │
│  └──────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ Database State Mutators                              │  │
│  │  - select_next_action() → DatabaseAction            │  │
│  │  - INSERT/UPDATE/DELETE/CREATE INDEX                │  │
│  └──────────────────────────────────────────────────────┘  │
└────────────────┬───────────────────────────────────────────┘
                 │ Reports violations, guides mutations
                 ▼
┌────────────────────────────────────────────────────────────┐
│  VOPR (Simulation Harness)                                 │
│  - Generates random queries                                │
│  - Executes with TLP/NoREC oracles                         │
│  - Tracks coverage, mutates when plateaued                 │
│  - Fails on violation with reproducible seed               │
└────────────────────────────────────────────────────────────┘
```

---

## Testing & Verification ✅

### Unit Tests (11 new)
- **kimberlite-sim**: 225/225 passing (up from 214)
  - `sql_oracles::tests`: 11 tests covering both oracles and coverage tracking
  - Each oracle has violation and success test cases
  - Mutation strategy tested with different database states
  - Invariant tracking verified

### Coverage Tracking
Both SQL oracles integrated with `invariant_tracker`:
```bash
# After running VOPR with SQL testing
invariant_tracker.get_run_count("sql_tlp_partitioning")   // > 0
invariant_tracker.get_run_count("sql_norec_consistency")  // > 0
```

---

## Usage Examples

### TLP Oracle

```rust
use kimberlite_sim::{TlpOracle, InvariantResult};

let mut oracle = TlpOracle::new();

// Execute original query
let original_count = execute_query("SELECT * FROM users WHERE age > 30").count();  // 100

// Execute partitioned queries
let true_count = execute_query("SELECT * FROM users WHERE age > 30").count();  // 80
let false_count = execute_query("SELECT * FROM users WHERE NOT (age > 30)").count();  // 15
let null_count = execute_query("SELECT * FROM users WHERE (age > 30) IS NULL").count();  // 5

// Verify partitioning
let result = oracle.verify_partitioning(
    "query1",
    original_count,
    true_count,
    false_count,
    null_count,
);

assert!(matches!(result, InvariantResult::Ok));  // 80 + 15 + 5 = 100 ✓
```

### NoREC Oracle

```rust
use kimberlite_sim::NoRecOracle;

let mut oracle = NoRecOracle::new();

// Execute with optimizations enabled
let optimized_results = execute_query_optimized("SELECT * FROM users WHERE age > 30");
let optimized_hash = hash_results(&optimized_results);

// Execute with optimizations disabled (force table scan)
let unoptimized_results = execute_query_unoptimized("SELECT * FROM users WHERE age > 30");
let unoptimized_hash = hash_results(&unoptimized_results);

// Verify consistency
let result = oracle.verify_optimization("query1", optimized_hash, unoptimized_hash);

assert!(matches!(result, InvariantResult::Ok));  // Hashes match ✓
```

### Query Plan Coverage + Mutations

```rust
use kimberlite_sim::{QueryPlanCoverageTracker, compute_plan_signature, select_next_action};

let mut coverage = QueryPlanCoverageTracker::new(100);

// Execute queries, track coverage
loop {
    let query = generate_random_query();
    let plan = plan_query(&query);

    let signature = compute_plan_signature(
        plan.plan_type(),
        plan.table_name(),
        plan.index_name(),
        plan.has_filter(),
        plan.has_limit(),
        plan.has_order_by(),
        plan.aggregate_function(),
    );

    let is_new = coverage.record_plan(signature);
    if is_new {
        println!("New plan discovered! Coverage: {}", coverage.unique_plan_count());
    }

    // Check if coverage has plateaued
    if coverage.is_coverage_plateaued() {
        // Select mutation to increase coverage
        let action = select_next_action(
            coverage.unique_plan_count(),
            get_row_count(),
            get_index_count(),
            coverage.steps_since_new_plan,
        );

        if let Some(action) = action {
            execute_mutation(action);
            coverage.reset_plateau_counter();
        }
    }
}
```

---

## Integration with VOPR (Future)

When SQL testing is added to VOPR:

```rust
// In VOPR simulation loop (future integration)
let mut tlp = TlpOracle::new();
let mut norec = NoRecOracle::new();
let mut coverage = QueryPlanCoverageTracker::new(100);

loop {
    // Generate random query
    let query = generate_random_query(&mut rng);

    // TLP: Execute partitioned queries
    let original_count = execute(&query).count();
    let (true_count, false_count, null_count) = execute_partitioned(&query);

    if let InvariantResult::Violated { .. } =
        tlp.verify_partitioning(&query, original_count, true_count, false_count, null_count) {
        return Err(SimError::InvariantViolation(/* ... */));
    }

    // NoREC: Compare optimized vs unoptimized
    let optimized_hash = execute_optimized(&query);
    let unoptimized_hash = execute_unoptimized(&query);

    if let InvariantResult::Violated { .. } =
        norec.verify_optimization(&query, optimized_hash, unoptimized_hash) {
        return Err(SimError::InvariantViolation(/* ... */));
    }

    // Track coverage
    let plan = plan_query(&query);
    let signature = compute_plan_signature(/* ... */);
    coverage.record_plan(signature);

    // Mutate when plateaued
    if coverage.is_coverage_plateaued() {
        let action = select_next_action(/* ... */);
        if let Some(action) = action {
            execute_mutation(action);
            coverage.reset_plateau_counter();
        }
    }
}
```

---

## Key Design Decisions

### 1. Hash-Based Result Comparison (NoREC)
- **Decision**: Compare result hashes instead of full result sets
- **Why**: Efficient, deterministic, works with large result sets
- **Alternative**: Deep equality check (expensive for large results)
- **Benefit**: O(1) space, O(n) time to compute hash

### 2. Separate Partitioning Counts (TLP)
- **Decision**: Pass 4 separate counts (original, true, false, null)
- **Why**: Makes violation messages clearer (can see which partition is wrong)
- **Alternative**: Single partition count array (less clear)
- **Benefit**: Rich error context for debugging

### 3. Plan Signature vs Full Plan Comparison
- **Decision**: Use hash signature for plan coverage
- **Why**: Efficient set membership test, deterministic
- **Alternative**: Store full QueryPlan structs (more memory, harder equality)
- **Benefit**: HashSet lookups, compact storage

### 4. Configurable Plateau Threshold
- **Decision**: `QueryPlanCoverageTracker::new(plateau_threshold)`
- **Why**: Different workloads have different coverage dynamics
- **Alternative**: Hardcoded threshold (less flexible)
- **Example**: Short runs = 50, long runs = 1000

### 5. Action Selection Heuristics (Not ML)
- **Decision**: Simple rule-based mutation strategy
- **Why**: Deterministic, easy to understand, no training needed
- **Alternative**: Reinforcement learning or genetic algorithms (complex)
- **Trade-off**: Simple but effective for initial coverage

---

## Known Limitations

1. **TLP doesn't handle complex expressions**
   - Current: Works best with simple WHERE clauses
   - Missing: Automatic partitioning of complex boolean expressions (nested AND/OR)
   - Reason: Requires query rewrite infrastructure
   - Impact: Manual partition construction needed for complex queries

2. **NoREC requires execution mode flag**
   - Current: Assumes query engine has "unoptimized" mode
   - Missing: Automatic optimizer disabling
   - Workaround: Could force table scans by hiding indexes
   - Impact: Integration requires query engine support

3. **Mutation strategy is simplistic**
   - Current: Rule-based (if X then Y)
   - Alternative: Reinforcement learning to optimize for coverage
   - Reason: Simplicity first, ML later if needed
   - Impact: May not find optimal mutations

4. **No actual query execution**
   - Current: Oracle infrastructure only, no query runner
   - Missing: Integration with kimberlite-query executor
   - Reason: Phase 8 is oracle design, Phase 9+ will integrate
   - Impact: Oracles are tested but not yet used in VOPR

5. **Plan signatures might collide**
   - Current: Hash-based signatures (u64)
   - Risk: Different plans could hash to same value
   - Mitigation: Good hash function (DefaultHasher)
   - Impact: Low probability, acceptable for coverage tracking

---

## Next Steps

### Immediate (Remaining Phase 8 Work)
1. None - Phase 8 SQL oracles are complete ✅

### Integration Work (Future Phases)
1. **VOPR Integration**:
   - Add SQL query generation to scenarios
   - Hook TLP/NoREC oracles into query execution
   - Implement actual mutation execution (INSERT/DELETE/etc.)
   - Create SQL-focused VOPR scenarios

2. **Query Engine Integration**:
   - Add optimizer enable/disable flag to kimberlite-query
   - Implement query result hashing
   - Add TLP partition query generation
   - Integrate plan signature computation

3. **Advanced Mutations**:
   - Implement more sophisticated mutation strategies
   - Add transaction-based mutations
   - Create mutations for constraints (UNIQUE, FOREIGN KEY)
   - Generate schema mutations (ALTER TABLE)

### Phase 9: LLM Integration (Safe Architecture)
1. LLM-generated SQL queries (offline validation)
2. Failure analysis with LLM (explain violations)
3. Mutation suggestions from LLM
4. Test case shrinking assistance

---

## Files Created/Modified

### New Files (1)
1. `/crates/kimberlite-sim/src/sql_oracles.rs` (630+ lines) - SQL metamorphic testing oracles

### Modified Files (2)
1. `/crates/kimberlite-sim/src/lib.rs` - Added `pub mod sql_oracles` and exports
2. `/PHASE8_COMPLETE.md` - This documentation

---

## Metrics

### New in Phase 8
- **New Files**: 1 (sql_oracles.rs)
- **Modified Files**: 1 (lib.rs)
- **New Tests**: 11 (SQL oracle tests)
- **Lines of Code**: ~630 (sql_oracles.rs)
- **Testing Oracles**: 2 (TLP, NoREC)
- **Coverage Tools**: 1 (QueryPlanCoverageTracker)
- **Mutation Actions**: 7 (INSERT, UPDATE, DELETE, CREATE INDEX, DROP INDEX, ANALYZE, special values)
- **Invariants Tracked**: 2 (sql_tlp_partitioning, sql_norec_consistency)

### Cumulative (Phases 1-8)
- **Total Tests**: 225 (all passing)
- **Instrumentation Tests**: 26
- **Proc Macros**: 6 (fault_point!, fault!, phase!, sometimes_assert!, assert_after!, assert_within_steps!)
- **Fault Points**: 5
- **Invariants Tracked**: 18 (8 original + 4 VSR + 4 projection + 2 SQL)
- **Phase Markers**: 1 (storage:fsync_complete)
- **Deferred Assertions**: Infrastructure complete
- **Canaries**: 5 defined, 1 applied
- **VSR Invariants**: 4
- **Projection Invariants**: 4
- **SQL Oracles**: 2 (TLP, NoREC)
- **Coverage Trackers**: 1 (QueryPlanCoverage)

---

## References

- **SQLancer**: "Detecting Logic Bugs in DBMS" (Rigger & Su, 2020) - TLP, NoREC
- **"Testing Database Engines via Pivoted Query Synthesis"** (Chen et al.) - PQS approach
- **"Detecting Optimization Bugs in Database Engines"** (Jung et al.) - NoREC oracle
- **TigerBeetle**: Deterministic query testing
- **FoundationDB**: Simulation testing for SQL
- **CockroachDB**: Metamorphic testing in production
- **SQLsmith**: Random SQL query generation

---

**Phase 8 Status**: ✅ **COMPLETE**
**Date Completed**: 2026-02-02
**Tests Passing**: 225/225 (kimberlite-sim)
**SQL Oracles**: 2 (TLP, NoREC)
**Coverage Tracking**: Query plan coverage
**Mutation Framework**: 7 database actions
**Integration**: Ready for VOPR (future phase)
**Next Phase**: Phase 9 - LLM Integration (Safe Architecture)
