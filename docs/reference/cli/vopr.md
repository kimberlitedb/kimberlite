# VOPR CLI Reference

VOPR (Viewstamped Operation Replication) is a deterministic simulation testing tool for Kimberlite. It provides 10 commands for running simulations, debugging failures, and analyzing coverage.

## Quick Reference

```bash
# Run simulations
vopr run --scenario baseline --iterations 1000

# Reproduce a failure
vopr repro failure.kmb

# Show failure details
vopr show failure.kmb --events

# List scenarios
vopr scenarios

# Display statistics
vopr stats --detailed

# Visualize timeline
vopr timeline failure.kmb

# Find first failing event
vopr bisect failure.kmb

# Minimize test case
vopr minimize failure.kmb

# Web dashboard
vopr dashboard --port 8080

# Interactive TUI
vopr tui
```

---

## Commands

### 1. `run` - Run Simulations

Run a simulation with fault injection and invariant checking.

#### Usage

```bash
vopr run [OPTIONS]
```

#### Options

- `--scenario <SCENARIO>` - Scenario to run (default: baseline)
  - See `vopr scenarios` for full list of 46 scenarios
  - Examples: baseline, combined, multi_tenant_isolation, byzantine_view_change_merge
- `--iterations <N>` - Number of iterations (default: 1000)
- `--seed <SEED>` - Random seed for determinism (default: random)
- `--faults <FAULTS>` - Comma-separated fault types to inject
  - Options: network, storage, crash, clock, byzantine
  - Example: `--faults network,storage,crash`
- `--output <FORMAT>` - Output format: human, json, compact (default: human)
- `--verbose, -v` - Verbose output
- `--save-failures` - Save failure bundles as .kmb files

#### Examples

```bash
# Run baseline scenario with 10k iterations
vopr run --scenario baseline --iterations 10000

# Run with specific seed (for reproduction)
vopr run --scenario combined --seed 42 --iterations 1000

# Enable all fault types
vopr run --scenario multi_tenant_isolation --faults network,storage,crash,byzantine

# Save failures for debugging
vopr run --scenario byzantine_commit_desync --save-failures

# Quiet mode with JSON output (for CI)
vopr run --scenario baseline --iterations 100 --output json --quiet
```

#### Exit Codes

- `0` - All simulations passed
- `1` - One or more failures detected
- `2` - Invalid arguments or configuration error

---

### 2. `repro` - Reproduce Failure

Reproduce a failure from a `.kmb` (Kimberlite Bundle) file.

#### Usage

```bash
vopr repro <BUNDLE_FILE> [OPTIONS]
```

#### Arguments

- `<BUNDLE_FILE>` - Path to .kmb failure bundle

#### Options

- `--verbose, -v` - Show detailed execution trace
- `--output <FORMAT>` - Output format: human, json, compact
- `--check-invariants` - Re-check all invariants (default: true)
- `--no-check-invariants` - Skip invariant checking

#### Examples

```bash
# Reproduce a failure
vopr repro failure-20260205-143022.kmb

# Reproduce with verbose output
vopr repro failure.kmb --verbose

# Reproduce and output as JSON
vopr repro failure.kmb --output json
```

#### Bundle File Format

`.kmb` files are compressed (zstd) binary bundles containing:
- Random seed
- Scenario configuration
- Full event log
- Failure details
- Coverage metrics

**Size:** Typically 50-500KB per failure.

---

### 3. `show` - Display Failure Summary

Display a human-readable summary of a failure bundle.

#### Usage

```bash
vopr show <BUNDLE_FILE> [OPTIONS]
```

#### Arguments

- `<BUNDLE_FILE>` - Path to .kmb failure bundle

#### Options

- `--events` - Show event timeline
- `--coverage` - Show coverage metrics
- `--invariants` - Show invariant violations
- `--output <FORMAT>` - Output format: human, json, compact
- `--detailed` - Show full details (implies all flags)

#### Examples

```bash
# Show summary
vopr show failure.kmb

# Show with event timeline
vopr show failure.kmb --events

# Show everything
vopr show failure.kmb --detailed

# Export as JSON
vopr show failure.kmb --output json > failure-report.json
```

#### Output Sections

1. **Metadata**: Scenario, seed, timestamp, duration
2. **Failure Summary**: Which invariant failed and why
3. **Event Timeline** (if `--events`): Sequence of events leading to failure
4. **Coverage** (if `--coverage`): Code paths exercised
5. **Invariants** (if `--invariants`): All invariant check results

---

### 4. `scenarios` - List Scenarios

List all 46 available test scenarios.

#### Usage

```bash
vopr scenarios [OPTIONS]
```

#### Options

- `--filter <PATTERN>` - Filter scenarios by name pattern
- `--phase <PHASE>` - Filter by phase (0-10)
- `--output <FORMAT>` - Output format: human, json, compact
- `--describe` - Show detailed descriptions

#### Examples

```bash
# List all scenarios
vopr scenarios

# List with descriptions
vopr scenarios --describe

# Filter Byzantine scenarios
vopr scenarios --filter byzantine

# Show Phase 1 scenarios only
vopr scenarios --phase 1

# Export as JSON
vopr scenarios --output json
```

#### Scenario Phases

- **Phase 0: Core (6)** - Baseline, swizzle clogging, gray failures, multi-tenant isolation, time compression, combined
- **Phase 1: Byzantine (11)** - Protocol attacks (split brain, malicious leader, equivocation, etc.)
- **Phase 2: Corruption (3)** - Bit flips, checksum validation, silent disk failures
- **Phase 3: Crash/Recovery (3)** - Crash during commit, view change, corrupt log recovery
- **Phase 4: Gray Failures (2)** - Slow disk, intermittent network
- **Phase 5: Race Conditions (2)** - Concurrent view changes, commit during DVC
- **Phase 6: Clock (3)** - Clock drift, offset exceeded, NTP failure
- **Phase 7: Client Sessions (3)** - Session crash, view change lockout, eviction
- **Phase 8: Repair/Timeout (5)** - Repair budget, EWMA selection, sync timeout, primary abdicate, commit stall
- **Phase 9: Scrubbing (4)** - Scrub detects corruption, completes tour, rate limited, triggers repair
- **Phase 10: Reconfiguration (3)** - Add replicas, remove replicas, during partition

**Total:** 46 scenarios (as of v0.4.0)

See [/docs-internal/vopr/scenarios.md](../../../docs-internal/vopr/scenarios.md) for full scenario documentation (contributor docs).

---

### 5. `stats` - Display Statistics

Display coverage and invariant checking statistics.

#### Usage

```bash
vopr stats [OPTIONS]
```

#### Options

- `--detailed` - Show per-scenario breakdowns
- `--coverage` - Show code coverage metrics
- `--invariants` - Show invariant check counts
- `--output <FORMAT>` - Output format: human, json, compact

#### Examples

```bash
# Show overall statistics
vopr stats

# Show detailed per-scenario stats
vopr stats --detailed

# Show only coverage metrics
vopr stats --coverage

# Export as JSON for analysis
vopr stats --output json > stats.json
```

#### Metrics Displayed

1. **Simulation Throughput**: Iterations per second
2. **Coverage**: State coverage, message coverage, fault coverage, path coverage
3. **Invariants**: Check counts, pass/fail rates
4. **Performance**: Average time per iteration, p99 latency
5. **Per-Scenario**: Individual scenario statistics (if `--detailed`)

---

### 6. `timeline` - Visualize Timeline

Visualize simulation timeline as an ASCII Gantt chart.

#### Usage

```bash
vopr timeline <BUNDLE_FILE> [OPTIONS]
```

#### Arguments

- `<BUNDLE_FILE>` - Path to .kmb failure bundle

#### Options

- `--width <COLS>` - Terminal width in columns (default: auto-detect)
- `--show-messages` - Annotate with message types
- `--show-faults` - Highlight injected faults
- `--output <FILE>` - Save to file instead of stdout

#### Examples

```bash
# Show timeline
vopr timeline failure.kmb

# Show with message annotations
vopr timeline failure.kmb --show-messages

# Show with fault highlighting
vopr timeline failure.kmb --show-faults

# Save to file
vopr timeline failure.kmb --output timeline.txt
```

#### Timeline Format

```
Replica 0 |====== VIEW=0 ======|.........|== VIEW=1 ==|
Replica 1 |====== VIEW=0 ======|XXXXXXXXX|== VIEW=1 ==|
Replica 2 |====== VIEW=0 ======|.........|== VIEW=1 ==|
          0ms                  100ms     150ms        200ms

Legend:
  === Active/healthy
  XXX Crashed/partitioned
  ... Silent (no messages)
  ^^^ Fault injection
  >>> Message sent
```

---

### 7. `bisect` - Binary Search for First Failing Event

Find the first event that triggers an invariant violation using binary search.

#### Usage

```bash
vopr bisect <BUNDLE_FILE> [OPTIONS]
```

#### Arguments

- `<BUNDLE_FILE>` - Path to .kmb failure bundle

#### Options

- `--verbose, -v` - Show binary search progress
- `--output <FORMAT>` - Output format: human, json, compact

#### Examples

```bash
# Find first failing event
vopr bisect failure.kmb

# Show search progress
vopr bisect failure.kmb --verbose

# Export result as JSON
vopr bisect failure.kmb --output json
```

#### Algorithm

1. Start with full event log (N events)
2. Run simulation with first N/2 events
3. If passes: failure is in second half, continue with events [N/2, N]
4. If fails: failure is in first half, continue with events [0, N/2]
5. Repeat until single failing event found

**Complexity:** O(log N) simulations where N = number of events.

#### Output

```
Binary search progress:
  [0..1000] - FAIL (checking first 500)
  [0..500]  - PASS (checking 500..750)
  [500..750] - FAIL (checking 500..625)
  [500..625] - PASS (checking 625..687)
  ...
  [672..673] - FAIL

First failing event: #672
Event type: NetworkPartition
Details: Replica 1 isolated from quorum
```

---

### 8. `minimize` - Delta Debugging

Minimize a test case by removing events while preserving failure.

#### Usage

```bash
vopr minimize <BUNDLE_FILE> [OPTIONS]
```

#### Arguments

- `<BUNDLE_FILE>` - Path to .kmb failure bundle

#### Options

- `--strategy <STRATEGY>` - Minimization strategy: linear, binary, delta (default: delta)
- `--output <FILE>` - Save minimized bundle to file
- `--verbose, -v` - Show minimization progress

#### Examples

```bash
# Minimize using delta debugging
vopr minimize failure.kmb

# Use binary minimization (faster)
vopr minimize failure.kmb --strategy binary

# Save minimized bundle
vopr minimize failure.kmb --output minimal.kmb
```

#### Strategies

1. **Linear**: Remove one event at a time (slow, best result)
2. **Binary**: Remove half at a time (fast, good result)
3. **Delta (default)**: Delta debugging algorithm (balanced)

#### Delta Debugging Algorithm

1. Start with N events
2. Split into 2 chunks, try removing each chunk
3. If removing a chunk preserves failure, use that smaller set
4. If neither works, split into 4 chunks and repeat
5. Continue until no more events can be removed

**Result:** Minimal event sequence that still triggers the failure.

---

### 9. `dashboard` - Web Coverage Dashboard

Launch a web dashboard for exploring coverage and invariant metrics.

#### Usage

```bash
vopr dashboard [OPTIONS]
```

#### Options

- `--port <PORT>` - Port to listen on (default: 8080)
- `--host <HOST>` - Host to bind to (default: 127.0.0.1)
- `--data-dir <DIR>` - Directory with .kmb bundles (default: ./failures/)

#### Examples

```bash
# Launch dashboard on default port
vopr dashboard

# Launch on custom port
vopr dashboard --port 3000

# Point to specific failures directory
vopr dashboard --data-dir /var/vopr/failures/
```

#### Dashboard Features

- **Coverage Heatmap**: Visual representation of code coverage by scenario
- **Invariant Matrix**: Which scenarios test which invariants
- **Failure Timeline**: Chronological view of all failures
- **Scenario Explorer**: Drill down into individual scenario results
- **Interactive Filters**: Filter by phase, fault type, invariant

#### URL Structure

```
http://localhost:8080/                    # Home
http://localhost:8080/scenarios           # All scenarios
http://localhost:8080/scenarios/:id       # Specific scenario details
http://localhost:8080/failures            # All failures
http://localhost:8080/failures/:id        # Failure details
http://localhost:8080/coverage            # Coverage heatmap
http://localhost:8080/invariants          # Invariant matrix
```

---

### 10. `tui` - Interactive Terminal UI

Launch an interactive terminal user interface for exploring simulations.

#### Usage

```bash
vopr tui [OPTIONS]
```

#### Options

- `--data-dir <DIR>` - Directory with .kmb bundles (default: ./failures/)

#### Examples

```bash
# Launch TUI
vopr tui

# Point to specific directory
vopr tui --data-dir /var/vopr/failures/
```

#### TUI Features

- **Scenario Selection**: Interactive picker for choosing scenarios
- **Live Progress**: Real-time simulation progress with ETA
- **Failure Navigation**: Browse failures with arrow keys
- **Event Timeline**: Step through events with playback controls
- **Coverage View**: Live coverage metrics during simulation
- **Log Viewer**: Searchable, filterable log output

#### Keyboard Shortcuts

```
Navigation:
  ↑/↓ or j/k    - Move selection up/down
  ←/→ or h/l    - Switch panels
  Enter         - Select item
  Esc           - Go back
  q             - Quit

Playback (Timeline view):
  Space         - Play/pause
  n             - Next event
  p             - Previous event
  0             - Jump to start
  $             - Jump to end

Search:
  /             - Search
  n             - Next match
  N             - Previous match

Other:
  r             - Refresh
  f             - Toggle filter
  s             - Sort by column
  ?             - Show help
```

#### Panels

1. **Scenarios**: Browse and run scenarios
2. **Failures**: View recent failures
3. **Timeline**: Step through failure events
4. **Coverage**: Live coverage metrics
5. **Logs**: Searchable log output

---

## Integration with Justfile

The VOPR CLI integrates with the project's Justfile for convenience:

```bash
# Run VOPR with default scenario
just vopr

# List all scenarios
just vopr-scenarios

# Run specific scenario with iteration count
just vopr-scenario baseline 10000

# Quick smoke test (100 iterations)
just vopr-quick

# Full test suite (all scenarios, 10k iterations each)
just vopr-full 10000

# Reproduce from bundle
just vopr-repro failure.kmb
```

See `justfile` for full list of VOPR shortcuts.

---

## Output Formats

All commands support three output formats via `--output`:

### Human (default)

Rich, colored terminal output with Unicode box drawing:

```
✓ Scenario: baseline
  Iterations: 1000
  Duration: 2.3s
  Throughput: 434 sims/sec

  ✓ All invariants passed
```

### JSON

Machine-readable JSON for tooling integration:

```json
{
  "scenario": "baseline",
  "iterations": 1000,
  "duration_ms": 2300,
  "throughput": 434,
  "invariants": {
    "passed": 19,
    "failed": 0
  }
}
```

### Compact

One-line summary for logs:

```
baseline 1000 2.3s 434/s ✓
```

---

## Exit Codes

All commands use consistent exit codes:

- `0` - Success (all checks passed)
- `1` - Failure detected (invariant violation, test failure)
- `2` - Invalid arguments or configuration error
- `3` - IO error (bundle not found, permission denied, etc.)

---

## Environment Variables

- `VOPR_DATA_DIR` - Default directory for .kmb bundles (default: ./failures/)
- `VOPR_SEED` - Default random seed (default: random)
- `VOPR_ITERATIONS` - Default iteration count (default: 1000)
- `VOPR_NO_COLOR` - Disable colored output (default: auto-detect TTY)
- `RUST_LOG` - Log level (debug, info, warn, error)

---

## Related Documentation

- [VOPR Testing Overview](../../internals/testing/overview.md) - User-facing testing overview
- [VOPR Deep Dive](/docs-internal/vopr/overview.md) - Internal implementation details (contributors)
- [All 46 Scenarios](/docs-internal/vopr/scenarios.md) - Complete scenario documentation (contributors)
- [VOPR Deployment](/docs-internal/vopr/deployment.md) - AWS testing infrastructure (contributors)
- [Writing Scenarios](/docs-internal/vopr/writing-scenarios.md) - How to add new scenarios (contributors)

---

**Last updated:** 2026-02-05 (v0.4.0)
