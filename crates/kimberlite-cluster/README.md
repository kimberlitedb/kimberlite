# kimberlite-cluster

Multi-node cluster management for Kimberlite database.

## Overview

This crate provides local multi-node cluster orchestration for testing and development. It enables running multiple Kimberlite nodes on a single machine with automatic topology configuration, process supervision, and health monitoring.

## Features

- **Automatic Topology Generation**: Creates peer lists and port assignments
- **Process Supervision**: Monitors node health and handles restarts
- **Configuration Persistence**: TOML-based cluster config in `.kimberlite/cluster/`
- **Graceful Shutdown**: Handles SIGTERM/SIGINT for clean shutdowns
- **Status Monitoring**: Tracks node states (Stopped, Running, Crashed)

## Architecture

```
ClusterSupervisor
├── NodeProcess (id=0, port=5432)
├── NodeProcess (id=1, port=5433)
└── NodeProcess (id=2, port=5434)
```

Each `NodeProcess` manages a child process running a Kimberlite server instance. The supervisor coordinates startup, shutdown, and monitors health.

## Usage

### From CLI

```bash
# Initialize 3-node cluster
kmb cluster init --nodes 3

# Start all nodes
kmb cluster start

# Check status
kmb cluster status

# Stop specific node (for failover testing)
kmb cluster stop --node 0

# Stop all nodes
kmb cluster stop

# Destroy cluster
kmb cluster destroy
```

### From Rust

```rust
use kimberlite_cluster::{ClusterConfig, ClusterSupervisor};

// Create cluster configuration
let config = ClusterConfig::new(data_dir, 3, 5432);
config.save(&project_path)?;

// Start supervisor
let mut supervisor = ClusterSupervisor::new(config);
supervisor.start_all().await?;

// Monitor (blocks until shutdown signal)
supervisor.monitor_loop().await;

// Cleanup
supervisor.stop_all().await?;
```

## Configuration

Cluster topology is stored in `.kimberlite/cluster/cluster.toml`:

```toml
node_count = 3
base_port = 5432
data_dir = "/path/to/project"

[[nodes]]
id = 0
port = 5432
data_dir = "/path/to/project/.kimberlite/cluster/node-0"
peers = ["127.0.0.1:5433", "127.0.0.1:5434"]

[[nodes]]
id = 1
port = 5433
data_dir = "/path/to/project/.kimberlite/cluster/node-1"
peers = ["127.0.0.1:5432", "127.0.0.1:5434"]

# ... node 2 ...
```

Each node gets:
- Unique ID (0..N-1)
- Port (base_port + id)
- Data directory (`.kimberlite/cluster/node-{id}/`)
- Peer list (all other nodes)

## Testing

```bash
# Run all cluster tests
cargo test -p kimberlite-cluster

# Run specific test
cargo test -p kimberlite-cluster test_supervisor_creation
```

Tests use `tempfile::TempDir` for isolation and mock processes for validation.

## Implementation Notes

### Process Management

Uses `tokio::process::Command` for spawning nodes:
- Non-blocking async I/O
- Safe signal handling (no unsafe code)
- Automatic cleanup on drop

### Health Monitoring

Basic liveness checking via `Child::id()`:
```rust
pub fn is_alive(&self) -> bool {
    self.process.as_ref()
        .and_then(|child| child.id())
        .is_some()
}
```

### Current Limitations (MVP)

- **Placeholder commands**: Uses `sleep infinity` until server binary integrated
- **No leader detection**: Requires Raft integration
- **No replication lag**: Requires server metrics API
- **No daemonization**: Supervisor runs in foreground (use systemd/supervisor in production)
- **Local only**: All nodes on 127.0.0.1 (production would use actual IPs)

## Future Enhancements

1. **Server Integration**: Replace placeholder with actual `kimberlite start` command
2. **Leader Detection**: Query Raft leader from server API
3. **Metrics**: Expose replication lag, throughput, latency
4. **Daemonization**: Background supervisor with IPC for status queries
5. **Cloud Support**: Remote node management via SSH/cloud APIs

## Safety

- No `unsafe` code (workspace lint enforced)
- Process cleanup on panic (tokio `Drop` guarantees)
- Graceful shutdown handling (SIGTERM/SIGINT)
- Port conflict detection (startup errors if port in use)

## See Also

- [kimberlite-cli](../kimberlite-cli/) - CLI commands using this crate
- [kimberlite-server](../kimberlite-server/) - Server that will be orchestrated
- Plan: Phase 5 (Cluster Management) in `/Users/jaredreyes/.claude/plans/prancy-launching-bubble.md`
