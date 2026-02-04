# Testing Strategy

**Internal Guide** - For Kimberlite contributors

## Testing Layers

Kimberlite uses a multi-layered testing strategy to achieve high confidence:

```
┌─────────────────────────────────────────┐
│   VOPR Simulation (46 scenarios)        │  ← Byzantine, corruption, crashes
├─────────────────────────────────────────┤
│   Fuzzing (libFuzzer/AFL)               │  ← Parser robustness
├─────────────────────────────────────────┤
│   Property Testing (proptest)           │  ← Invariants, commutativity
├─────────────────────────────────────────┤
│   Integration Tests                     │  ← Multi-crate interactions
├─────────────────────────────────────────┤
│   Unit Tests                            │  ← Individual functions
└─────────────────────────────────────────┘
```

## When to Use Each

| Test Type | Use When | Example |
|-----------|----------|---------|
| **Unit** | Testing pure functions | `hash_chain::verify()` |
| **Property** | Testing invariants | Offset monotonicity |
| **Integration** | Testing crate boundaries | VSR + Storage interaction |
| **VOPR** | Testing distributed behavior | View changes under partition |
| **Fuzzing** | Testing parsers/decoders | SQL parser, message framing |

## Coverage Goals

| Layer | Target Coverage | Current |
|-------|-----------------|---------|
| Unit tests | 80% line coverage | ~75% |
| Property tests | All invariants | 19 invariants |
| VOPR scenarios | All failure modes | 46 scenarios |
| Fuzzing | All parsers | 3 fuzz targets |

## Testing Checklist

When adding new code, ensure:

### For All Code
- [ ] Unit tests for happy path
- [ ] Unit tests for error cases
- [ ] Assertions for invariants (2+ per function)
- [ ] Documentation examples (if public API)

### For Consensus/Storage Code
- [ ] Property tests for invariants
- [ ] VOPR scenario if new failure mode
- [ ] Integration test if crossing crate boundaries

### For Parsers/Decoders
- [ ] Fuzz target added
- [ ] Edge cases tested (empty, max size, invalid)

### For Crypto Code
- [ ] Test vectors from standards (SHA-256, AES-256-GCM)
- [ ] All-zero detection tests
- [ ] Production assertions (no `debug_assert!` for crypto)

## Testing Commands

```bash
# All tests
just test                    # Standard test runner
just nextest                 # Faster parallel test runner

# Specific tests
just test-one test_name      # Run single test
cargo test --package kimberlite-vsr  # Test one crate

# Property tests (more cases)
PROPTEST_CASES=10000 cargo test --workspace

# VOPR smoke test
just vopr-quick              # 100 iterations

# VOPR full suite
just vopr-full 10000         # All 46 scenarios, 10k iterations each

# Fuzzing
just fuzz-list               # List fuzz targets
just fuzz parse_sql          # Fuzz SQL parser
```

## Writing Good Tests

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_chain_verifies_correct_sequence() {
        let entries = vec![
            Entry { hash: compute_hash(b"data1"), prev_hash: Hash::ZERO },
            Entry { hash: compute_hash(b"data2"), prev_hash: compute_hash(b"data1") },
        ];

        assert!(verify_hash_chain(&entries).is_ok());
    }

    #[test]
    fn test_hash_chain_rejects_broken_link() {
        let entries = vec![
            Entry { hash: compute_hash(b"data1"), prev_hash: Hash::ZERO },
            Entry { hash: compute_hash(b"data2"), prev_hash: Hash::ZERO },  // Wrong!
        ];

        assert!(matches!(verify_hash_chain(&entries), Err(Error::BrokenChain)));
    }
}
```

### Property Tests

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_offset_monotonically_increases(
        commands in prop::collection::vec(any::<Command>(), 1..100)
    ) {
        let mut state = State::default();
        let mut prev_offset = Offset(0);

        for cmd in commands {
            let (new_state, effects) = apply_committed(state, cmd)?;
            for effect in effects {
                if let Effect::LogAppend { offset, .. } = effect {
                    prop_assert!(offset > prev_offset, "Offset must increase");
                    prev_offset = offset;
                }
            }
            state = new_state;
        }
    }
}
```

### VOPR Scenarios

```rust
// In crates/kimberlite-sim/src/scenarios.rs

#[derive(Debug, Clone, Copy)]
pub enum ScenarioType {
    // ...existing scenarios...

    /// New scenario: Test repair during concurrent view change
    RepairDuringViewChange,
}

impl ScenarioBuilder {
    pub fn build(&self, scenario: ScenarioType) -> Scenario {
        match scenario {
            // ...

            ScenarioType::RepairDuringViewChange => {
                Scenario::new("Repair During View Change")
                    .with_cluster_size(3)
                    .with_faults(vec![
                        Fault::NetworkPartition { duration: 100 },
                        Fault::MessageDrop { rate: 0.1 },
                    ])
                    .with_invariants(vec![
                        Invariant::NoLogDivergence,
                        Invariant::EventualConsistency,
                    ])
            }
        }
    }
}
```

## Test Organization

```
crates/kimberlite-vsr/
├── src/
│   ├── lib.rs
│   ├── replica/
│   │   ├── mod.rs
│   │   ├── normal.rs
│   │   └── tests.rs       ← Unit tests for replica module
│   └── tests.rs           ← Unit tests for top-level
└── tests/
    ├── integration_tests.rs  ← Integration tests
    └── property_tests.rs     ← Property-based tests
```

## CI/CD Testing

```yaml
# .github/workflows/ci.yml
- name: Unit tests
  run: cargo nextest run --workspace

- name: Property tests
  run: PROPTEST_CASES=1000 cargo test --workspace

- name: VOPR smoke test
  run: cargo run --bin vopr -- run --scenario combined --iterations 1000

- name: Fuzzing (1 minute smoke)
  run: just fuzz-smoke
```

## Debugging Test Failures

### VOPR Failures

```bash
# Reproduce from .kmb bundle
vopr repro failure.kmb

# Show event timeline
vopr show failure.kmb --events

# Visualize with ASCII Gantt chart
vopr timeline failure.kmb

# Minimize test case
vopr minimize failure.kmb
```

### Property Test Failures

```bash
# Proptest automatically saves failing cases to proptest-regressions/
# Re-run to reproduce:
cargo test test_name

# Shrink to minimal failing case:
PROPTEST_MAX_SHRINK_ITERS=10000 cargo test test_name
```

## Performance Testing

```bash
# Criterion benchmarks
cargo bench --package kimberlite-bench

# Profile with perf
perf record -g target/release/benchmark_name
perf report

# Memory profiling
heaptrack target/release/benchmark_name
```

## Related Documentation

- **[VOPR Overview](../vopr/overview.md)** - Detailed simulation testing
- **[VOPR Scenarios](../vopr/scenarios.md)** - All 46 scenarios
- **[Testing Overview](../../docs/internals/testing/overview.md)** - Public testing guide
- **[Assertions Guide](../../docs/internals/testing/assertions.md)** - Assertion patterns

---

**Key Takeaway:** Use unit tests for functions, property tests for invariants, VOPR for distributed behavior, and fuzzing for parsers. Aim for 80% line coverage and 100% invariant coverage.
