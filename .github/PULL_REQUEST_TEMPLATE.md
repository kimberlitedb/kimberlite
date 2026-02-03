## Description

<!-- Describe your changes in detail -->

Fixes #(issue number)

## Type of Change

- [ ] Bug fix (non-breaking change which fixes an issue)
- [ ] New feature (non-breaking change which adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to change)
- [ ] Documentation update
- [ ] Refactoring (no functional changes)
- [ ] Performance improvement

## Code Quality Checklist

### PRESSURECRAFT Compliance

- [ ] Code follows Functional Core / Imperative Shell pattern
- [ ] No `unsafe` code (workspace lint enforces this)
- [ ] No recursion (use bounded loops with explicit limits)
- [ ] Functions are ≤70 lines (soft limit)
- [ ] No `unwrap()` in library code (use `expect()` with reason for invariants)
- [ ] Makes illegal states unrepresentable (uses newtypes, enums over bools)
- [ ] Parse, don't validate (validation at boundaries only)

### Assertions

- [ ] Added production assertions for cryptographic invariants, consensus safety, or state machine correctness
- [ ] Added debug assertions for performance-critical or redundant checks
- [ ] Every new production assertion has a corresponding `#[should_panic]` test
- [ ] Assertion density ≥2 per function (preconditions + postconditions)

### Testing

- [ ] All tests pass locally (`just test` or `just nextest`)
- [ ] Added unit tests for new functionality
- [ ] Added property tests for critical logic (if applicable)
- [ ] No clippy warnings (`just clippy`)
- [ ] Code is formatted (`just fmt`)

## Documentation Checklist

- [ ] Updated CHANGELOG.md (following Keep a Changelog format)
- [ ] Added/updated API documentation (rustdoc comments)
- [ ] Updated relevant docs/ files if architecture changed
- [ ] Added examples for new public APIs

## Impact Analysis

### Core Invariants

Does this change affect any core invariants? (Check all that apply)

- [ ] Immutable append-only log
- [ ] Hash chain integrity (SHA-256 for compliance, BLAKE3 for internal)
- [ ] Consensus safety (VSR protocol)
- [ ] MVCC correctness
- [ ] Deterministic kernel (no IO, no clocks, no randomness)
- [ ] Multi-tenant isolation
- [ ] None

**If yes, explain how invariants are preserved:**

### Performance

- [ ] No performance regression expected
- [ ] Performance improvement (attach benchmark results)
- [ ] Potential performance impact (explain trade-offs)

### Breaking Changes

If this is a breaking change:

- [ ] Documented migration path in CHANGELOG.md
- [ ] Updated version to reflect breaking change (v0.x.0 → v0.(x+1).0)
- [ ] Considered backward compatibility options

## Additional Notes

<!-- Any additional context, trade-offs, or implementation details -->

## Review Focus Areas

<!-- Guide reviewers on what to pay attention to -->

---

**By submitting this PR, I confirm that:**

- [ ] I have read and followed the [Code of Conduct](../CODE_OF_CONDUCT.md)
- [ ] My code follows the project's coding standards
- [ ] I have performed a self-review of my code
- [ ] I have commented my code where necessary, particularly in hard-to-understand areas
