# Kimberlite Fuzz Testing

This directory contains fuzz targets for finding bugs, panics, and security issues in Kimberlite's core components using `cargo-fuzz` and LLVM's libFuzzer.

## Setup

1. Install cargo-fuzz (if not already installed):
```bash
cargo install cargo-fuzz
```

2. Fuzzing requires nightly Rust (configured automatically via `rust-toolchain.toml` in this directory)

## Fuzz Targets

### 1. `fuzz_wire_deserialize` - Wire Protocol Parsing

Tests the robustness of the binary wire protocol deserialization:
- Frame header parsing (magic, version, length, checksum)
- Payload size limits (max 16 MiB)
- CRC32 checksum validation
- Bincode message deserialization (Request/Response)
- Buffer boundary conditions
- Malformed message handling

**Run:**
```bash
# From the fuzz directory
cargo fuzz run fuzz_wire_deserialize

# Limit to 1 million iterations
cargo fuzz run fuzz_wire_deserialize -- -runs=1000000

# Run for 1 hour
cargo fuzz run fuzz_wire_deserialize -- -max_total_time=3600

# Run with specific seed for reproducibility
cargo fuzz run fuzz_wire_deserialize -- -seed=123456
```

### 2. `fuzz_crypto_encrypt` - Encryption Operations

Tests the security and robustness of AES-256-GCM encryption:
- Encryption with arbitrary keys, nonces, and plaintexts
- Round-trip encrypt/decrypt validation
- Handling of weak/degenerate keys (all zeros, patterns)
- Authentication tag validation
- Wrong key detection
- Field-level encryption API
- Buffer overflow protection

**Run:**
```bash
# From the fuzz directory
cargo fuzz run fuzz_crypto_encrypt

# Longer run for thorough testing
cargo fuzz run fuzz_crypto_encrypt -- -max_total_time=7200
```

## Understanding Results

### Success (No Crashes)
```
INFO: Done 1000000 runs in 42 second(s)
```
No crashes found. Coverage metrics show how much code was explored.

### Crash Found
If libFuzzer finds a crash, it will:
1. Print the crash details to stderr
2. Save the crashing input to `artifacts/fuzz_<target>/crash-<hash>`
3. Exit with a non-zero status

**Reproduce a crash:**
```bash
cargo fuzz run fuzz_wire_deserialize artifacts/fuzz_wire_deserialize/crash-abc123
```

**Minimize a crashing input:**
```bash
cargo fuzz cmin fuzz_wire_deserialize
```

## Corpus Management

Fuzz corpora are automatically saved in `corpus/fuzz_<target>/`. These are interesting inputs that increase code coverage.

**Build corpus from existing tests:**
```bash
# Extract interesting test cases
cargo fuzz run fuzz_wire_deserialize corpus/fuzz_wire_deserialize -- -only_ascii=1
```

**Merge corpora from multiple runs:**
```bash
cargo fuzz cmin fuzz_wire_deserialize
```

## Continuous Fuzzing

For long-running fuzzing campaigns:

```bash
# Run overnight (8 hours)
cargo fuzz run fuzz_wire_deserialize -- -max_total_time=28800

# Run with AddressSanitizer for memory safety (default)
RUSTFLAGS="-Zsanitizer=address" cargo fuzz run fuzz_crypto_encrypt

# Run with MemorySanitizer (requires rebuilding std)
RUSTFLAGS="-Zsanitizer=memory" cargo fuzz run fuzz_wire_deserialize
```

## Integration with CI

To run fuzzing in CI with a time budget:

```bash
# Quick smoke test (1000 iterations each)
for target in fuzz_wire_deserialize fuzz_crypto_encrypt; do
    cargo fuzz run $target -- -runs=1000 || exit 1
done
```

## Coverage Reports

Generate coverage data from fuzzing:

```bash
# Build coverage instrumented binary
cargo fuzz coverage fuzz_wire_deserialize

# Generate HTML report
cargo cov -- show target/aarch64-apple-darwin/coverage/aarch64-apple-darwin/release/fuzz_wire_deserialize \
    -format=html -instr-profile=coverage/fuzz_wire_deserialize/coverage.profdata \
    > coverage.html
```

## Best Practices

1. **Run regularly**: Fuzz for at least 10 minutes per target before each release
2. **Seed corpus**: Add valid test cases to `corpus/` for faster coverage
3. **Monitor coverage**: Track `cov:` metric to ensure new code is tested
4. **Reproduce crashes**: Always minimize and add regression tests
5. **Long runs**: Consider overnight/weekend fuzzing campaigns

## Troubleshooting

### "No crashes found but test fails"
- Check for assertion failures in debug builds
- Look for timeout issues (increase with `-timeout=60`)

### "Low coverage"
- Add seed inputs to corpus directories
- Check that fuzz target exercises the right code paths
- Ensure input validation isn't rejecting most inputs

### "Out of memory"
- Limit RSS: `-rss_limit_mb=2048`
- Reduce corpus: `cargo fuzz cmin <target>`

## Adding New Fuzz Targets

1. Create new file: `fuzz_targets/fuzz_<name>.rs`
2. Add target to `Cargo.toml`:
   ```toml
   [[bin]]
   name = "fuzz_<name>"
   path = "fuzz_targets/fuzz_<name>.rs"
   ```
3. Implement fuzz target:
   ```rust
   #![no_main]
   use libfuzzer_sys::fuzz_target;

   fuzz_target!(|data: &[u8]| {
       // Test code here
   });
   ```
4. Build and run: `cargo fuzz run fuzz_<name>`

## Resources

- [cargo-fuzz book](https://rust-fuzz.github.io/book/cargo-fuzz.html)
- [libFuzzer documentation](https://llvm.org/docs/LibFuzzer.html)
- [Fuzzing in Rust](https://rust-fuzz.github.io/book/)
