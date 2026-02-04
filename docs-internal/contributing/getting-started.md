# Getting Started - Contributing to Kimberlite

**Internal Guide** - For Kimberlite contributors and maintainers

## Prerequisites

- Rust 1.88+ (enforced by `rust-toolchain.toml`)
- Git
- just (task runner): `cargo install just`
- Optional but recommended:
  - cargo-nextest: `cargo install cargo-nextest`
  - bacon (watch mode): `cargo install bacon`
  - cargo-audit: `cargo install cargo-audit`

## Initial Setup

### 1. Clone Repository

```bash
git clone https://github.com/kimberlitedb/kimberlite
cd kimberlite
```

### 2. Build Project

```bash
# Debug build (fast)
just build

# Release build (optimized)
just build-release
```

### 3. Run Tests

```bash
# Run all tests
just test

# Or use nextest (faster)
just nextest

# Run specific test
just test-one replica_normal_operation
```

### 4. Check Code Quality

```bash
# Run pre-commit checks (formatting, linting, tests)
just pre-commit

# Format code
just fmt

# Run clippy
just clippy
```

## Development Workflow

### Daily Development

```bash
# Watch mode with bacon
bacon                        # Default: check compilation
bacon test                   # Watch tests
bacon test-pkg -- kimberlite-vsr  # Test specific package
```

### Before Committing

```bash
# ALWAYS run pre-commit checks
just pre-commit

# This runs:
# 1. fmt-check (formatting)
# 2. clippy (linting, -D warnings)
# 3. test (all tests)
# 4. test-docs (documentation examples)
```

### Making Changes

1. **Create a branch**
   ```bash
   git checkout -b feature/your-feature-name
   ```

2. **Make changes and test**
   ```bash
   # Edit code
   vim crates/kimberlite-vsr/src/replica/normal.rs

   # Run tests
   just nextest

   # Run specific tests
   cargo test --package kimberlite-vsr test_name
   ```

3. **Add tests**
   - Unit tests in same file as implementation
   - Integration tests in `tests/` directory
   - VOPR scenarios in `crates/kimberlite-sim/src/scenarios.rs`

4. **Run pre-commit**
   ```bash
   just pre-commit
   ```

5. **Commit**
   ```bash
   git add .
   git commit -m "feat(vsr): Add repair budget tracking

   - Implement EWMA-based repair selection
   - Add tests for repair budget exhaustion
   - Document repair algorithm in comments"
   ```

### Commit Message Format

```
<type>(<scope>): <subject>

<body>

<footer>
```

**Types:**
- `feat`: New feature
- `fix`: Bug fix
- `refactor`: Code refactoring
- `perf`: Performance improvement
- `test`: Add or update tests
- `docs`: Documentation changes
- `chore`: Maintenance tasks
- `ci`: CI/CD changes

**Examples:**
```
feat(vsr): Implement view change merge

fix(storage): Handle torn writes during crash

refactor(kernel): Extract command validation

perf(crypto): Use BLAKE3 for internal hashing

test(vsr): Add Byzantine DVC scenario

docs(guides): Update deployment guide

chore(deps): Update proptest to 1.5
```

## Project Structure

```
kimberlite/
├── crates/                      # All crates (30 total)
│   ├── kimberlite/              # Main crate (re-exports)
│   ├── kimberlite-types/        # Core types (TenantId, StreamId, etc.)
│   ├── kimberlite-crypto/       # Cryptography (SHA-256, BLAKE3, AES-256-GCM)
│   ├── kimberlite-storage/      # Append-only log with CRC32
│   ├── kimberlite-kernel/       # Pure functional state machine
│   ├── kimberlite-vsr/          # VSR consensus protocol
│   ├── kimberlite-sim/          # VOPR deterministic simulation
│   └── ...                      # 23 more crates
├── docs/                        # Public documentation
├── docs-internal/               # Internal/contributor documentation
├── justfile                     # Task runner commands
├── rust-toolchain.toml          # Rust version (1.88)
└── Cargo.toml                   # Workspace configuration
```

## Testing Philosophy

Kimberlite uses multiple testing strategies:

1. **Unit tests** - Test individual functions/modules
2. **Property tests** - Proptest for invariants
3. **Integration tests** - Test crate interactions
4. **VOPR simulation** - Deterministic fault injection (46 scenarios)
5. **Fuzzing** - AFL/libFuzzer for parsers

See [Testing Strategy](testing-strategy.md) for details.

## Running VOPR

```bash
# Quick smoke test
just vopr-quick

# Run specific scenario
just vopr-scenario baseline 10000

# List all scenarios (46 total)
just vopr-scenarios

# Full test suite
just vopr-full 10000
```

See [VOPR Overview](../vopr/overview.md) for detailed VOPR usage.

## Code Style

### Rust Style

- **No unsafe code** - Workspace lint denies it
- **No recursion** - Use bounded loops
- **No unwrap in library code** - Use `expect()` with reason for invariants
- **70-line soft limit** per function
- **Assertion density** - 2+ assertions per non-trivial function

### Assertion Guidelines

```rust
// Good: Production assertions for crypto/consensus
assert!(!all_zero, "encryption key cannot be all zeros");  // Prevent weak keys
assert!(view_number >= state.view, "view cannot decrease");  // Consensus safety

// Good: Debug assertions for performance-critical paths
debug_assert!(offset < log.len(), "offset out of bounds");

// Bad: Assertions for expected errors
assert!(parse_result.is_ok());  // Use proper error handling instead
```

See [Assertions Guide](../../docs/internals/testing/assertions.md) for complete guidelines.

### Documentation

```rust
/// Brief one-line summary.
///
/// Detailed explanation with examples if needed.
///
/// # Arguments
///
/// * `arg1` - Description
///
/// # Returns
///
/// Description of return value
///
/// # Panics
///
/// Conditions that cause panic (if any)
pub fn function_name(arg1: Type) -> Result<ReturnType> {
    // ...
}
```

## Common Tasks

### Add New VOPR Scenario

1. Add scenario to `ScenarioType` enum in `crates/kimberlite-sim/src/scenarios.rs`
2. Implement scenario logic
3. Add to scenario registry
4. Document in [VOPR Scenarios](../vopr/scenarios.md)
5. Run scenario: `just vopr-scenario your_scenario 1000`

### Add New Assertion

1. Identify invariant to check
2. Add `assert!()` or `debug_assert!()` as appropriate
3. Write `#[should_panic]` test
4. Document in [Assertions Guide](../../docs/internals/testing/assertions.md)

### Profile Performance

```bash
# Benchmark with criterion
cargo bench --package kimberlite-bench

# CPU profiling
perf record -g target/release/kimberlite-server
perf report

# Memory profiling
heaptrack target/release/kimberlite-server
```

## Getting Help

- **Questions?** Ask in Discord or GitHub Discussions
- **Bug found?** Open a GitHub issue
- **Security issue?** See [Bug Bounty Program](../internal/bug-bounty.md)

## Related Documentation

- **[Testing Strategy](testing-strategy.md)** - Detailed testing approach
- **[Release Process](release-process.md)** - How to cut releases
- **[Code Review Guide](code-review.md)** - Review checklist
- **[VOPR Overview](../vopr/overview.md)** - Simulation testing deep dive

---

**Welcome to the Kimberlite team!** Questions? Reach out in Discord.
