# GitHub Actions Workflows

This directory contains CI/CD workflows for Kimberlite.

## Core Workflows

### `ci.yml`
Main CI pipeline - runs on every push and PR
- Builds all crates
- Runs full test suite
- Checks code formatting and linting
- Validates documentation

### `build-verification.yml`
Multi-platform build verification
- Tests builds on Linux, macOS, Windows
- Validates cross-compilation

### `release.yml`
Release automation
- Creates GitHub releases
- Builds release artifacts
- Publishes crates to crates.io

## Testing Workflows

### `vopr-determinism.yml` ⭐ NEW
**Determinism validation on every commit**
- Runs VOPR with `--check-determinism` flag
- Tests multiple scenarios (baseline, combined, multi-tenant)
- Enforces coverage thresholds
- **Fast**: ~5-10 minutes
- See [docs/vopr-ci-integration.md](../../docs/vopr-ci-integration.md) for details

### `vopr-nightly.yml` ⭐ NEW
**Nightly stress testing**
- Long-running VOPR tests (10k+ iterations)
- Comprehensive scenario coverage
- Auto-creates issues on failure
- **Slow**: ~1-2 hours
- Runs daily at 2 AM UTC

## Security & Compliance

### `security.yml`
Security scanning
- Dependency audits
- Vulnerability scanning
- SAST analysis

## Documentation

### `docs.yml`
Documentation generation and deployment
- Builds API documentation
- Validates doc examples
- Deploys to GitHub Pages

### `deploy-site.yml`
Marketing site deployment

## FFI & SDKs

### `build-ffi.yml`
FFI library builds
- C header generation
- Cross-language bindings

### `sdk-python.yml`
Python SDK CI
- Runs Python tests
- Validates bindings

### `sdk-typescript.yml`
TypeScript SDK CI
- Runs TypeScript tests
- Type checking

## Code Quality

### `claude-code-review.yml`
AI-powered code review
- Automated review suggestions
- Pattern detection

### `claude.yml`
Claude AI integration helpers

## Running Workflows Locally

### Determinism check (matches CI)
```bash
cargo build --release -p kimberlite-sim --bin vopr
./target/release/vopr --iterations 100 --check-determinism
```

### Coverage enforcement (matches CI)
```bash
./target/release/vopr \
  --iterations 200 \
  --min-fault-coverage 80.0 \
  --min-invariant-coverage 100.0 \
  --require-all-invariants \
  --check-determinism
```

### Manual nightly run
```bash
gh workflow run vopr-nightly.yml --field iterations=10000
```

## Workflow Dependencies

```
ci.yml
  ├─ build-verification.yml (parallel)
  ├─ vopr-determinism.yml (parallel) ⭐
  └─ security.yml (parallel)

vopr-nightly.yml (scheduled)
  └─ Creates issue on failure

release.yml
  ├─ Requires: ci.yml passing
  ├─ Triggers: build-ffi.yml
  ├─ Triggers: sdk-python.yml
  └─ Triggers: sdk-typescript.yml
```

## Exit Codes

All workflows use standard exit codes:
- **0**: Success
- **1**: Test/build failures
- **2**: Coverage/quality thresholds not met (VOPR only)

## Artifacts

Workflows upload artifacts for debugging:
- **vopr-determinism.yml**: Failure logs (7 days retention)
- **vopr-nightly.yml**: JSON results + traces (30 days retention)
- **build-verification.yml**: Build artifacts per platform
- **docs.yml**: Generated documentation

## Troubleshooting

### VOPR workflow fails with "coverage too low"
- Increase `--iterations` count
- Check if new fault points were added
- See [docs/vopr-ci-integration.md](../../docs/vopr-ci-integration.md)

### VOPR workflow fails with "determinism violation"
- Critical bug - investigate immediately
- Download artifacts for failure traces
- Reproduce locally with failing seed
- See debugging guide in [docs/vopr-ci-integration.md](../../docs/vopr-ci-integration.md)

### Workflow timeout
- Check for infinite loops
- Reduce iteration counts
- Optimize hot paths

## Adding New Workflows

1. Create `.yml` file in this directory
2. Test locally with [act](https://github.com/nektos/act)
3. Document in this README
4. Add to appropriate dependency chain

## Best Practices

1. **Keep PR checks fast** (<15 minutes)
2. **Use nightly for heavy testing** (>30 minutes)
3. **Cache dependencies** to speed up builds
4. **Upload artifacts** on failure
5. **Auto-create issues** for critical failures
6. **Monitor trends** in nightly results
