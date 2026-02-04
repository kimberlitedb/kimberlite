# Release Process

**Internal Guide** - For Kimberlite maintainers

## Release Schedule

- **Minor releases** (0.x.0): Every 4-6 weeks
- **Patch releases** (0.x.y): As needed for critical bugs
- **Major releases** (1.0.0+): When API is stable

## Pre-Release Checklist

### 1. Code Quality
- [ ] All CI checks passing
- [ ] No open P0/P1 bugs
- [ ] Code review completed for all PRs
- [ ] Documentation updated

### 2. Testing
- [ ] All tests passing: `just test`
- [ ] VOPR full suite: `just vopr-full 10000`
- [ ] Property tests: `PROPTEST_CASES=10000 cargo test`
- [ ] Fuzzing (24-hour run): `just fuzz-all`
- [ ] Performance benchmarks run and reviewed

### 3. Documentation
- [ ] CHANGELOG.md updated with all changes
- [ ] Migration guide written (if breaking changes)
- [ ] API documentation reviewed
- [ ] Examples tested and updated

### 4. Dependencies
- [ ] Cargo.lock updated: `cargo update`
- [ ] Security audit clean: `cargo audit`
- [ ] License compliance: `cargo deny check licenses`

## Release Steps

### 1. Update Version Numbers

```bash
# Update workspace version in Cargo.toml
vim Cargo.toml
# Change: version = "0.4.0" to version = "0.5.0"

# Update all crate versions (automated script)
./scripts/update-versions.sh 0.5.0
```

### 2. Update CHANGELOG.md

```markdown
## [0.5.0] - 2024-02-15

### Added
- Reconfiguration support (add/remove replicas dynamically)
- Client session tracking with eviction
- Repair budget to prevent repair storms

### Changed
- Improved view change merge logic
- Updated to Rust 1.88

### Fixed
- Byzantine DVC tail length mismatch detection
- Repair EWMA calculation overflow
- Clock drift handling in consensus

### Performance
- 15% faster log appends with optimized CRC32
- Reduced memory usage in projection cache

### Documentation
- Added reconfiguration design doc
- Updated deployment guide with 5-node clusters
```

### 3. Create Release Branch

```bash
git checkout -b release/v0.5.0
git add -A
git commit -m "chore: Prepare v0.5.0 release"
git push origin release/v0.5.0
```

### 4. Run Final Checks

```bash
# Full CI locally
just ci-full

# Cross-platform builds
just verify-builds-all

# Run release tests
cargo test --workspace --release

# VOPR extended run (1M iterations)
cargo run --bin vopr --release -- run --scenario combined --iterations 1000000
```

### 5. Create Git Tag

```bash
git tag -a v0.5.0 -m "Release v0.5.0

Major features:
- Reconfiguration support
- Client session tracking
- Improved Byzantine fault detection

See CHANGELOG.md for full details."

git push origin v0.5.0
```

### 6. Publish to crates.io

```bash
# Dry run first
./scripts/publish-crates.sh --dry-run

# Publish (order matters - dependencies first)
./scripts/publish-crates.sh

# Script publishes in order:
# 1. kimberlite-types
# 2. kimberlite-crypto
# 3. kimberlite-storage
# 4. kimberlite-kernel
# ... (30 crates total)
```

### 7. Create GitHub Release

1. Go to https://github.com/kimberlitedb/kimberlite/releases/new
2. Select tag: `v0.5.0`
3. Title: `Kimberlite v0.5.0`
4. Description: Copy from CHANGELOG.md
5. Attach binaries (if prebuilt):
   - `kimberlite-v0.5.0-x86_64-unknown-linux-gnu.tar.gz`
   - `kimberlite-v0.5.0-x86_64-apple-darwin.tar.gz`
   - `kimberlite-v0.5.0-aarch64-apple-darwin.tar.gz`
6. Mark as "Latest release"
7. Publish

### 8. Update Documentation Site

```bash
cd website
# Update version in config
vim config.toml

# Deploy
just deploy
```

### 9. Announce Release

- [ ] Post on Discord
- [ ] Tweet from @KimberliteDB
- [ ] Update README.md with new version
- [ ] Send email to mailing list

## Post-Release

### 1. Merge Release Branch

```bash
git checkout main
git merge --no-ff release/v0.5.0
git push origin main
```

### 2. Monitor Issues

Watch for critical bugs in first 48 hours after release.

### 3. Update Milestones

- Close v0.5.0 milestone
- Create v0.6.0 milestone
- Triage open issues

## Hotfix Process

For critical bugs in released versions:

```bash
# Checkout release tag
git checkout v0.5.0

# Create hotfix branch
git checkout -b hotfix/v0.5.1

# Fix bug
vim src/bug_file.rs
git commit -m "fix(vsr): Critical consensus bug"

# Update version to 0.5.1
vim Cargo.toml
git commit -m "chore: Bump to v0.5.1"

# Tag and publish
git tag v0.5.1
git push origin v0.5.1
./scripts/publish-crates.sh

# Merge back to main
git checkout main
git merge hotfix/v0.5.1
git push origin main
```

## Version Numbering

Kimberlite follows Semantic Versioning (SemVer):

- **MAJOR** (1.0.0): Breaking API changes
- **MINOR** (0.x.0): New features, backward compatible
- **PATCH** (0.x.y): Bug fixes, backward compatible

### Pre-1.0 Versioning

Before 1.0.0, minor versions (0.x.0) MAY contain breaking changes.

## Release Artifacts

Each release includes:
- **Source code** (GitHub)
- **crates.io packages** (30 crates)
- **Documentation** (docs.rs)
- **Binaries** (GitHub releases) - Linux, macOS, Windows
- **Docker images** (ghcr.io/kimberlitedb/kimberlite)
- **Changelog** (CHANGELOG.md)

## Rollback Procedure

If a release has critical issues:

1. **Yank from crates.io**
   ```bash
   cargo yank --vers 0.5.0 kimberlite
   ```

2. **Delete GitHub release**
   - Mark as "Pre-release" or delete

3. **Notify users**
   - Discord announcement
   - GitHub issue

4. **Fix and re-release as patch version**
   - Fix bug → v0.5.1
   - Test thoroughly
   - Release v0.5.1

## Related Documentation

- **[Getting Started](getting-started.md)** - Contributor setup
- **[Testing Strategy](testing-strategy.md)** - Release testing requirements
- **[Code Review](code-review.md)** - Pre-release review checklist

---

**Key Takeaway:** Test thoroughly before release. Run VOPR full suite, update CHANGELOG, publish in dependency order, and monitor for issues after release.
