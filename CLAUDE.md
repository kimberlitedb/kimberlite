# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Kimberlite is a compliance-first, verifiable database for regulated industries (healthcare, finance, legal). Built on a single principle: **All data is an immutable, ordered log. All state is a derived view.**

## Repository Organization Guidelines

**CRITICAL**: Keep the repository root clean and organized. Follow these rules strictly when implementing features, writing code, or generating documentation.

### Where Files Go

1. **Documentation**:
   - User-facing docs → `/docs/` (organized by audience: start/, concepts/, coding/, operating/, reference/, internals/)
   - Internal/contributor docs → `/docs-internal/` (vopr/, contributing/, design-docs/, internal/)
   - **NEVER** create standalone .md files in root or random directories

2. **Future Work & Planning**:
   - All TODO items, planned features, roadmap items → `ROADMAP.md`
   - **DO NOT** create separate TODO.md, TASKS.md, FUTURE_WORK.md, or similar files
   - Use structured format with version targets (v0.5.0, v0.6.0, v1.0.0)

3. **Changes & Progress**:
   - Release notes, completed work, version history → `CHANGELOG.md`
   - Follow [Keep a Changelog](https://keepachangelog.com) format
   - **DO NOT** create separate release notes, progress tracking, or implementation status files

4. **Temporary Artifacts**:
   - Build artifacts, logs, test outputs → `.artifacts/` (fully gitignored)
   - VOPR logs (can be 70GB+) → `.artifacts/vopr/logs/`
   - Test states → `.artifacts/vopr/states/`
   - Formal verification outputs → `.artifacts/formal-verification/solver-outputs/`
   - Profiling data → `.artifacts/profiling/`
   - **NEVER** leave .log files, .kmb bundles, or test artifacts in root or crates/ directories
   - Use `just archive-vopr-logs` to move logs to .artifacts/ before long runs

5. **Tools & Commands**:
   - Development tools → `tools/` (e.g., `tools/formal-verification/alloy/`, `tools/formal-verification/docker/`)
   - **NO /scripts directory** - All commands consolidated in `justfile` (102 commands)
   - Run `just --list` to discover available commands
   - All logic inlined in justfile (no external bash scripts)

### What NOT to Do

❌ **DO NOT** create random .md files in root (IMPLEMENTATION_NOTES.md, PROGRESS.md, STATUS.md, DESIGN.md, etc.)
❌ **DO NOT** leave log files, .kmb bundles, profile.json.gz, or test artifacts anywhere except `.artifacts/`
❌ **DO NOT** create scripts/ directory or individual .sh files - use justfile recipes instead
❌ **DO NOT** duplicate documentation across multiple locations (docs/ vs ROADMAP.md vs random .md files)
❌ **DO NOT** create top-level directories (states/, results/, Quorum/, etc.) - these belong in `.artifacts/`
❌ **DO NOT** commit files to .artifacts/ - it's fully gitignored for a reason

### Clean Root Principle

The repository root should **ONLY** contain:
- Standard config files: `Cargo.toml`, `.gitignore`, `rust-toolchain.toml`, `.rustfmt.toml`
- Essential project docs: `README.md`, `CONTRIBUTING.md`, `LICENSE`, `CODE_OF_CONDUCT.md`
- Lifecycle tracking: `ROADMAP.md`, `CHANGELOG.md`, `CLAUDE.md`
- Build runner: `justfile`
- Organized directories: `crates/`, `docs/`, `docs-internal/`, `tools/`, `examples/`, `website/`, `specs/`, `.artifacts/`, `.github/`

**Before creating any file**, ask yourself:
1. Is this user-facing documentation? → `/docs/`
2. Is this internal/contributor documentation? → `/docs-internal/`
3. Is this a TODO or future feature? → `ROADMAP.md`
4. Is this a completed change or release note? → `CHANGELOG.md`
5. Is this a temporary artifact? → `.artifacts/`
6. None of the above? → **Consult the maintainers first**

### Directory Purpose Reference

```
kimberlite/
├── .artifacts/         # ALL temporary/generated files (gitignored)
│   ├── vopr/          # Simulation logs, states, results
│   ├── formal-verification/  # Solver outputs, proof artifacts
│   ├── profiling/     # Performance profiling data
│   └── coverage/      # Code coverage reports
├── crates/            # All Rust source code (30+ crates)
├── docs/              # Public user-facing documentation
├── docs-internal/     # Internal contributor documentation
├── examples/          # Example applications (Rust, Python, Node.js)
├── tools/             # Development tools (formal verification)
├── specs/             # Formal specifications (TLA+, Coq, Alloy, Ivy)
├── website/           # Public website content and blog
├── .github/           # GitHub Actions, issue templates
├── ROADMAP.md         # Future work and planned features
├── CHANGELOG.md       # Release history and completed work
└── justfile           # All commands (no scripts/)
```

## Build & Test Commands

```bash
# Build
just build                    # Debug build
just build-release            # Release build

# Testing
just test                     # Run all tests
just nextest                  # Faster test runner (preferred)
just test-one <name>          # Run single test

# Property Testing
PROPTEST_CASES=10000 cargo test --workspace  # More test cases

# Fuzzing
just fuzz-list                # List fuzz targets
just fuzz parse_sql           # Run SQL parser fuzzing
just fuzz-smoke               # CI smoke test (1 minute)

# VOPR Simulation (Antithesis-Grade Testing)
just vopr                     # Run VOPR with default scenario
just vopr-scenarios           # List available scenarios
just vopr-scenario baseline 100000           # Run specific scenario
just vopr-scenario multi_tenant_isolation 50000

# VOPR Enhanced Commands (v0.3.1)
just vopr-quick               # Smoke test (100 iterations)
just vopr-full 10000          # All scenarios (10k iterations)
just vopr-repro failure.kmb   # Reproduce from .kmb bundle

# VOPR CLI (via cargo)
cargo run --bin vopr -- run --scenario combined --iterations 1000
cargo run --bin vopr -- repro failure.kmb --verbose
cargo run --bin vopr -- show failure.kmb --events
cargo run --bin vopr -- scenarios
cargo run --bin vopr -- stats --detailed

# Code Quality (run before commits)
just pre-commit               # fmt-check + clippy + test
just clippy                   # Linting (enforces -D warnings)
just fmt                      # Format code

# Full CI locally
just ci                       # fmt-check + clippy + test + doc-check
just ci-full                  # Above + security audits

# Live development with bacon
bacon                         # Watch mode (default)
bacon test-pkg -- kimberlite-crypto  # Test specific package
```

## Architecture

```
┌──────────────────────────────────────────────────┐
│  Kernel (pure state machine: Cmd → State + FX)   │
├──────────────────────────────────────────────────┤
│  Append-Only Log (hash-chained, CRC32)           │
├──────────────────────────────────────────────────┤
│  Crypto (SHA-256, BLAKE3, AES-256-GCM, Ed25519)  │
└──────────────────────────────────────────────────┘
```

**Crate Structure** (`crates/`):
- `kimberlite` - Facade, re-exports all modules
- `kimberlite-types` - Entity IDs (TenantId, StreamId, Offset), data classification
- `kimberlite-crypto` - Cryptographic primitives (hash chains, signatures, encryption)
- `kimberlite-storage` - Binary append-only log with CRC32 checksums
- `kimberlite-kernel` - Pure functional state machine (Commands → State + Effects)
- `kimberlite-directory` - Placement routing for multi-tenant isolation
- `kimberlite-sim` - VOPR deterministic simulation testing framework

## VOPR Testing Infrastructure (v0.3.1)

VOPR (Viewstamped Operation Replication) is our deterministic simulation testing framework achieving 90-95% Antithesis-grade testing without a hypervisor.

**Core Capabilities**:
- **46 test scenarios** across 10 phases: Byzantine attacks, corruption detection, crash recovery, gray failures, race conditions, clock issues, client sessions, repair/timeout, scrubbing, and reconfiguration
- **19 invariant checkers** validating consensus safety, storage integrity, offset monotonicity, and MVCC correctness
- **Industry-proven approach** - Offset monotonicity + VSR safety (FoundationDB/TigerBeetle pattern), no O(n!) linearizability checker
- **100% determinism** - Same seed → same execution (validated in CI)
- **85k-167k sims/sec** throughput with full fault injection
- **5 canary mutations** (100% detection rate) proving VOPR catches bugs

**Enhanced Capabilities (v0.3.1)**:
- **Storage Realism** (`storage_reordering.rs`, `concurrent_io.rs`, `crash_recovery.rs`):
  - 4 I/O scheduler policies (FIFO, Random, Elevator, Deadline)
  - Concurrent out-of-order I/O (up to 32 operations/device)
  - 5 crash scenarios (DuringWrite, DuringFsync, PowerLoss, etc.)
  - Block-level granularity (4KB), torn write simulation

- **Byzantine Attacks** (`protocol_attacks.rs`):
  - 10 protocol-level attack patterns (SplitBrain, MaliciousLeader, PrepareEquivocation, etc.)
  - 3 pre-configured suites (Standard, Aggressive, Subtle)
  - 100% attack detection rate

- **Observability** (`event_log.rs`):
  - Event logging with compact binary format (~100 bytes/event)
  - `.kmb` failure reproduction bundles (bincode + zstd)
  - Perfect reproduction from seed + event log

- **Workloads** (`workload_generator.rs`, `coverage_fuzzer.rs`):
  - 6 realistic patterns (Uniform, Hotspot, Sequential, MultiTenant, Bursty, ReadModifyWrite)
  - Multi-dimensional coverage tracking (state, messages, faults, paths)
  - Coverage-guided fuzzing with 3 selection strategies

- **CLI** (`cli/` modules):
  - 10 commands: run, repro, show, scenarios, stats, timeline, bisect, minimize, dashboard, tui
  - Progress bars, multiple output formats (Human, JSON, Compact)
  - Automatic .kmb bundle generation on failure
  - Interactive TUI and web dashboard for debugging

**Key Modules**:
```
crates/kimberlite-sim/src/
├── storage_reordering.rs    # Write reordering engine (4 policies)
├── concurrent_io.rs         # Concurrent I/O tracker (out-of-order completion)
├── crash_recovery.rs        # Crash semantics (5 scenarios, torn writes)
├── protocol_attacks.rs      # Byzantine attack patterns (10 attacks)
├── event_log.rs            # Event logging & .kmb bundles
├── workload_generator.rs   # Realistic workloads (6 patterns)
├── coverage_fuzzer.rs      # Coverage-guided fuzzing
└── cli/                    # CLI commands (10 commands)
```

**Performance**:
- Storage realism: <5% overhead
- Event logging: <10% overhead
- Overall: >70k sims/sec maintained

**See**: `docs/TESTING.md` (VOPR Enhanced Capabilities section) for complete documentation

## Project Structure

**Documentation Layout**:
- `/docs/` - Public user-facing documentation (progressive disclosure model)
  - `start/` - Get running in <10 minutes (quick-start, installation, first-app)
  - `concepts/` - Understanding Kimberlite (architecture, data model, consensus, compliance)
  - `coding/` - Building applications (quickstarts, guides, recipes)
  - `operating/` - Deployment & operations (deployment, monitoring, security, performance)
  - `reference/` - API documentation (CLI, SQL, SDKs, protocols)
  - `internals/` - Deep technical details (architecture, testing, design docs)
- `/docs-internal/` - Internal contributor/maintainer documentation
  - `vopr/` - VOPR testing details (46 scenarios, AWS deployment, debugging, writing scenarios)
  - `contributing/` - Contributor guides (getting started, code review, release process, testing strategy)
  - `design-docs/` - Active and archived design discussions
  - `internal/` - Team processes and internal materials
- `ROADMAP.md` - Future work and planned features
  - Performance optimizations, cluster enhancements, compliance features
  - Version targets (v0.5.0 - v1.0.0)
- `CHANGELOG.md` - Release history and completed work
  - Detailed milestone releases (0.1.0, 0.1.5, 0.1.10, 0.2.0)
  - Keep a Changelog format with full context for each release
- `/website/content/` - Public-facing documentation and blog

**Clear Separation Principle**:
- `/docs` contains ONLY current state and implemented features
- `ROADMAP.md` contains ONLY future work and planned features
- `CHANGELOG.md` contains complete historical release notes
- No duplication between these three categories

## Core Design Patterns

### Functional Core / Imperative Shell (FCIS)

**Mandatory pattern.** The kernel is pure and deterministic. All IO lives at the edges.

```rust
// Core (pure): No IO, no clocks, no randomness
fn apply_committed(state: State, cmd: Command) -> Result<(State, Vec<Effect>)>

// Shell (impure): Executes effects, handles IO
impl Runtime {
    fn execute_effect(&mut self, effect: Effect) -> Result<()>
}
```

For types requiring randomness (crypto keys), use struct-level FCIS:
- `from_random_bytes()` - Pure core, `pub(crate)` to prevent weak input
- `generate()` - Impure shell that provides randomness

### Make Illegal States Unrepresentable

- Use enums over booleans
- Use newtypes over primitives (`TenantId(u64)` not `u64`)
- Encode state machines in types (compile-time enforcement)

### Parse, Don't Validate

Validate at boundaries once, then use typed representations throughout.

### Assertion Density

Every function should have 2+ assertions (preconditions and postconditions). Write assertions in pairs at write and read sites.

**Production vs Development Assertions**:
- Use `assert!()` for cryptographic invariants, consensus safety, state machine correctness, and compliance-critical properties
- Use `debug_assert!()` for performance-critical checks (after profiling), redundant checks, and internal helpers
- Never use assertions for input validation (use `Result`), control flow (use `if/else`), or expected errors

**As of v0.2.0**: 38 critical assertions promoted to production enforcement:
- Cryptography (25): All-zero detection, key hierarchy integrity, ciphertext validation
- Consensus (9): Leader-only operations, view/commit monotonicity, quorum validation
- State Machine (4): Stream existence, effect completeness, offset monotonicity

**Performance Impact**: <0.1% throughput regression, +1μs p99 latency. Assertions are cold branches with negligible overhead.

**Testing**: Every production assertion requires a `#[should_panic]` test. See `docs/ASSERTIONS.md` for complete guide.

## Dual-Hash Cryptography

- **SHA-256**: Compliance-critical paths (hash chains, checkpoints, exports)
- **BLAKE3**: Internal hot paths (content addressing, Merkle trees)

Use `HashPurpose` enum to enforce the boundary at compile time.

## Key Constraints

- **No unsafe code** - Workspace lint denies it
- **No recursion** - Use bounded loops with explicit limits
- **No unwrap in library code** - Use `expect()` with reason for invariants
- **70-line soft limit** per function
- **Rust 1.88 MSRV** - Pinned in `rust-toolchain.toml`

## Error Handling

- `thiserror` for library error types
- `anyhow` for application code with context
- Rich error context via `#[from]` and `#[error(...)]`

## Testing Conventions

- Unit tests in `src/tests.rs` per crate
- Property-based testing with `proptest`
- Parametrized tests with `test-case`
