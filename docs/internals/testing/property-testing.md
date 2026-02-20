---
title: "Property-Based Testing"
section: "internals/testing"
slug: "property-testing"
order: 3
---

# Property-Based Testing

Test invariants using proptest to find edge cases automatically.

## What is Property Testing?

Property testing generates hundreds of random test cases to verify that invariants ("properties") hold for all inputs.

**Example invariant:** "Offset must increase monotonically"

```rust
// Instead of testing specific cases:
assert_eq!(apply(state, cmd1).offset, 1);
assert_eq!(apply(state, cmd2).offset, 2);

// Test the property for all possible commands:
proptest! {
    #[test]
    fn offset_increases(commands in vec(any::<Command>(), 1..100)) {
        let mut prev_offset = 0;
        for cmd in commands {
            let offset = apply(state, cmd).offset;
            assert!(offset > prev_offset);
            prev_offset = offset;
        }
    }
}
```

## When to Use Property Testing

Use property tests for:
- **Invariants** - Properties that must always hold
- **Symmetry** - `encode(decode(x)) == x`
- **Idempotence** - `f(f(x)) == f(x)`
- **Commutativity** - `a + b == b + a`
- **Associativity** - `(a + b) + c == a + (b + c)`

Don't use for:
- Specific business logic (use unit tests)
- Integration tests (use integration tests)
- Fuzz testing parsers (use fuzzing)

## Kimberlite Property Tests

### 1. Offset Monotonicity

```rust
use proptest::prelude::*;
use kimberlite_kernel::*;

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
                    prop_assert!(offset > prev_offset);
                    prev_offset = offset;
                }
            }

            state = new_state;
        }
    }
}
```

### 2. Hash Chain Integrity

```rust
proptest! {
    #[test]
    fn test_hash_chain_unbreakable(
        entries in prop::collection::vec(any::<Entry>(), 1..50)
    ) {
        // Build valid chain
        let mut chain = vec![];
        let mut prev_hash = Hash::ZERO;

        for entry in entries {
            let e = Entry {
                data: entry.data,
                prev_hash,
            };
            prev_hash = e.compute_hash();
            chain.push(e);
        }

        // Verify chain is valid
        prop_assert!(verify_hash_chain(&chain).is_ok());

        // Break chain by modifying any entry
        if !chain.is_empty() {
            let idx = prop::sample::Index::from(0..chain.len());
            chain[idx.index(&chain.len())].data = vec![0xFF];

            // Broken chain must be detected
            prop_assert!(verify_hash_chain(&chain).is_err());
        }
    }
}
```

### 3. State Machine Commutativity

```rust
proptest! {
    #[test]
    fn test_commutative_operations(
        cmd1 in any::<ReadCommand>(),
        cmd2 in any::<ReadCommand>()
    ) {
        let state = State::default();

        // Apply in order: cmd1 then cmd2
        let state1 = apply_read(apply_read(state.clone(), cmd1), cmd2);

        // Apply in reverse: cmd2 then cmd1
        let state2 = apply_read(apply_read(state.clone(), cmd2), cmd1);

        // Reads are commutative - order doesn't matter
        prop_assert_eq!(state1, state2);
    }
}
```

### 4. Serialization Round-Trip

```rust
proptest! {
    #[test]
    fn test_message_serialization_round_trip(
        msg in any::<Message>()
    ) {
        let bytes = msg.encode();
        let decoded = Message::decode(&bytes)?;

        prop_assert_eq!(msg, decoded);
    }
}
```

### 5. Encryption/Decryption Symmetry

```rust
proptest! {
    #[test]
    fn test_encryption_symmetry(
        plaintext in prop::collection::vec(any::<u8>(), 1..1000),
        key in any::<SymmetricKey>()
    ) {
        let ciphertext = key.encrypt(&plaintext)?;
        let decrypted = key.decrypt(&ciphertext)?;

        prop_assert_eq!(plaintext, decrypted);
    }
}
```

## Custom Generators

Create custom generators for domain types:

```rust
use proptest::prelude::*;

// Strategy for TenantId (1-1000)
fn tenant_id_strategy() -> impl Strategy<Value = TenantId> {
    (1u64..=1000).prop_map(TenantId::new)
}

// Strategy for valid Commands
fn command_strategy() -> impl Strategy<Value = Command> {
    prop_oneof![
        // Append command
        (tenant_id_strategy(), any::<Vec<u8>>())
            .prop_map(|(tenant, data)| Command::Append { tenant, data }),

        // Query command
        (tenant_id_strategy(), any::<String>())
            .prop_map(|(tenant, query)| Command::Query { tenant, query }),
    ]
}

// Use custom strategy
proptest! {
    #[test]
    fn test_with_custom_commands(
        commands in prop::collection::vec(command_strategy(), 1..50)
    ) {
        // Test with generated commands
        let mut state = State::default();
        for cmd in commands {
            state = apply_committed(state, cmd)?;
        }
    }
}
```

## Shrinking

When a property test fails, proptest automatically shrinks the input to the minimal failing case:

```rust
proptest! {
    #[test]
    fn test_parse_fails_on_invalid(
        input in ".*"  // Any string
    ) {
        // This will fail on some inputs
        let result = parse_sql(&input);

        // Proptest will shrink to smallest failing input
        // e.g., "(" or "SELECT" or whatever triggers the bug
    }
}
```

**Shrinking output:**
```
thread 'test_parse_fails_on_invalid' panicked at 'Test failed after 23 iterations.
Minimal failing input: "("
```

## Configuration

Control proptest behavior:

```rust
proptest! {
    // Run more test cases (default: 256)
    #![proptest_config(ProptestConfig::with_cases(1000))]

    #[test]
    fn test_with_more_cases(input in any::<u64>()) {
        // Runs 1000 times instead of 256
    }
}
```

Or via environment variable:
```bash
PROPTEST_CASES=10000 cargo test
```

## Regression Tests

Proptest saves failing cases to `proptest-regressions/`:

```bash
proptest-regressions/
├── kernel.txt         # Failing cases for kernel tests
├── storage.txt        # Failing cases for storage tests
└── crypto.txt         # Failing cases for crypto tests
```

These are automatically re-run on every test to prevent regressions.

## Performance Tips

### 1. Use Smaller Ranges

```rust
// ❌ Slow: Generates huge collections
vec(any::<Entry>(), 0..10000)

// ✅ Fast: Reasonable size
vec(any::<Entry>(), 1..100)
```

### 2. Filter Early

```rust
// ❌ Slow: Generates then filters
any::<u64>().prop_filter("must be even", |x| x % 2 == 0)

// ✅ Fast: Generate only evens
(0u64..1000).prop_map(|x| x * 2)
```

### 3. Parallel Execution

```bash
# Run property tests in parallel
cargo nextest run
```

## Common Patterns

### Invariant Testing

```rust
proptest! {
    #[test]
    fn invariant_holds(input in any::<Input>()) {
        let result = function_under_test(input);
        prop_assert!(invariant_check(result));
    }
}
```

### Round-Trip Testing

```rust
proptest! {
    #[test]
    fn round_trip(data in any::<Data>()) {
        let serialized = serialize(data.clone());
        let deserialized = deserialize(&serialized)?;
        prop_assert_eq!(data, deserialized);
    }
}
```

### Comparison Testing

```rust
proptest! {
    #[test]
    fn implementations_equivalent(input in any::<Input>()) {
        let result1 = implementation_v1(input.clone());
        let result2 = implementation_v2(input);
        prop_assert_eq!(result1, result2);
    }
}
```

## Related Documentation

- **[Testing Overview](overview.md)** - General testing philosophy
- **[Assertions Guide](assertions.md)** - Runtime assertion patterns
- **[Testing Strategy](../../../docs-internal/contributing/testing-strategy.md)** (Internal) - Detailed testing approach

---

**Key Takeaway:** Property tests find edge cases you wouldn't think to test manually. Use them for invariants, symmetry, and round-trips. Proptest will shrink failures to minimal cases.
