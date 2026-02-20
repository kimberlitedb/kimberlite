---
title: "Kimberlite Agent Protocol"
section: "reference"
slug: "agent-protocol"
order: 2
---

# Kimberlite Agent Protocol

This document describes the protocol for communication between Kimberlite cluster agents and control plane systems.

## Overview

The agent protocol enables:

- **Health monitoring**: Agents report node status via heartbeats
- **Configuration management**: Control planes push configuration updates
- **Observability**: Metrics and logs are streamed from agents
- **Administration**: Control planes can issue administrative commands
- **Authentication**: Secure agent identity verification
- **Flow control**: Backpressure handling for high-volume data
- **Health checks**: On-demand health verification

## Transport

The protocol uses WebSocket connections with JSON-encoded messages. Agents connect to the control plane endpoint and maintain a persistent connection.

```
Agent                          Control Plane
  |                                  |
  |-------- WebSocket Connect ------>|
  |                                  |
  |<------- AuthChallenge -----------|
  |-------- AuthResponse ----------->|
  |                                  |
  |<------- HeartbeatRequest --------|
  |-------- Heartbeat ------------->|
  |                                  |
  |-------- MetricsBatch ---------->|
  |-------- LogsBatch ------------->|
  |                                  |
  |<------- ConfigUpdate -----------|
  |-------- ConfigAck ------------->|
  |                                  |
  |<------- AdminCommand -----------|
  |-------- ControlAck ------------>|
  |                                  |
  |<------- HealthCheck ------------|
  |-------- HealthCheckResponse --->|
  |                                  |
  |<------- FlowControl ------------|
  |                                  |
```

## Message Types

### Agent → Control Plane

#### Heartbeat

Periodic status update sent by the agent.

```json
{
  "type": "heartbeat",
  "node_id": "node-001",
  "status": "healthy",
  "role": "leader",
  "resources": {
    "cpu_percent": 45.2,
    "memory_used_bytes": 1073741824,
    "memory_total_bytes": 8589934592,
    "disk_used_bytes": 10737418240,
    "disk_total_bytes": 107374182400
  },
  "replication": null,
  "buffer_stats": [
    {
      "stream_type": "metrics",
      "state": "normal",
      "pending_items": 50,
      "capacity": 1000,
      "dropped_count": 0,
      "oldest_item_age_ms": 500
    }
  ]
}
```

**Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `node_id` | string | Unique identifier for the node |
| `status` | enum | `healthy`, `degraded`, `unhealthy`, `starting`, `stopping` |
| `role` | enum | `leader`, `follower`, `candidate`, `learner` |
| `resources` | object | Current resource utilization |
| `replication` | object? | Replication status (followers only) |
| `buffer_stats` | array | Buffer statistics for backpressure monitoring |

**Replication object:**

| Field | Type | Description |
|-------|------|-------------|
| `leader_id` | string | ID of the leader being replicated from |
| `lag_ms` | u64 | Replication lag in milliseconds |
| `pending_entries` | u64 | Number of entries waiting to replicate |

**Buffer stats object:**

| Field | Type | Description |
|-------|------|-------------|
| `stream_type` | enum | `heartbeats`, `metrics`, `logs`, `all` |
| `state` | enum | `empty`, `normal`, `high`, `critical` |
| `pending_items` | u64 | Items currently buffered |
| `capacity` | u64 | Maximum buffer capacity |
| `dropped_count` | u64 | Items dropped since last report |
| `oldest_item_age_ms` | u64 | Age of oldest buffered item |

#### MetricsBatch

Batch of collected metric samples.

```json
{
  "type": "metrics_batch",
  "node_id": "node-001",
  "metrics": [
    {
      "name": "kmb.writes.total",
      "value": 12345.0,
      "timestamp_ms": 1700000000000,
      "labels": [["tenant", "acme"]]
    }
  ]
}
```

**Metric sample fields:**

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Metric name (e.g., `kmb.writes.total`) |
| `value` | f64 | Metric value |
| `timestamp_ms` | u64 | Unix timestamp in milliseconds |
| `labels` | array | Optional key-value pairs |

#### LogsBatch

Batch of log entries.

```json
{
  "type": "logs_batch",
  "node_id": "node-001",
  "entries": [
    {
      "timestamp_ms": 1700000000000,
      "level": "info",
      "message": "Snapshot completed",
      "fields": [["duration_ms", "1234"]]
    }
  ]
}
```

**Log entry fields:**

| Field | Type | Description |
|-------|------|-------------|
| `timestamp_ms` | u64 | Unix timestamp in milliseconds |
| `level` | enum | `trace`, `debug`, `info`, `warn`, `error` |
| `message` | string | Log message |
| `fields` | array | Optional structured fields |

#### ConfigAck

Acknowledgment of a configuration update.

```json
{
  "type": "config_ack",
  "version": 42,
  "success": true,
  "error": null
}
```

**Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `version` | u64 | Configuration version being acknowledged |
| `success` | bool | Whether configuration was applied |
| `error` | string? | Error message if failed |

#### ControlAck

Acknowledgment of a control message (AdminCommand, etc.).

```json
{
  "type": "control_ack",
  "message_id": 42,
  "success": true,
  "error": null,
  "result": "{\"snapshot_id\": \"snap-001\"}",
  "duration_ms": 1234
}
```

**Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `message_id` | u64 | ID of the message being acknowledged |
| `success` | bool | Whether the command succeeded |
| `error` | string? | Error message if failed |
| `result` | string? | JSON-encoded command-specific result |
| `duration_ms` | u64 | Time taken to execute the command |

#### AuthResponse

Response to authentication challenge.

```json
{
  "type": "auth_response",
  "credentials": {
    "type": "bearer",
    "token": "eyJhbGciOiJIUzI1NiIs..."
  },
  "agent_info": {
    "version": "1.0.0",
    "protocol_version": "v1",
    "capabilities": ["snapshots", "compaction"]
  }
}
```

**Credential types:**

| Type | Fields | Description |
|------|--------|-------------|
| `bearer` | `token` | JWT or API key |
| `pre_shared_key` | `key_id`, `signature` | HMAC signature of challenge |
| `certificate` | `fingerprint` | SHA-256 certificate fingerprint |

#### HealthCheckResponse

Response to a health check request.

```json
{
  "type": "health_check_response",
  "request_id": 42,
  "status": "healthy",
  "checks": [
    {
      "check_type": "liveness",
      "passed": true,
      "message": "OK",
      "details": []
    },
    {
      "check_type": "storage",
      "passed": true,
      "message": "Disk usage at 45%",
      "details": [["used_bytes", "48318382080"]]
    }
  ],
  "duration_ms": 5
}
```

**Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `request_id` | u64 | Correlation ID from request |
| `status` | enum | `healthy`, `degraded`, `unhealthy` |
| `checks` | array | Individual check results |
| `duration_ms` | u64 | Time taken for all checks |

### Control Plane → Agent

#### ConfigUpdate

Push new configuration to the agent.

```json
{
  "type": "config_update",
  "message_id": 1,
  "version": 42,
  "config": "{\"max_connections\": 100}",
  "checksum": "sha256:abc123..."
}
```

**Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `message_id` | u64? | Optional correlation ID for ack |
| `version` | u64 | Configuration version |
| `config` | string | JSON-encoded configuration |
| `checksum` | string | Integrity checksum |

The agent should verify the checksum before applying the configuration and respond with a `ConfigAck`.

#### AdminCommand

Execute an administrative command.

```json
{
  "type": "admin_command",
  "message_id": 42,
  "command": {
    "command": "take_snapshot"
  }
}
```

**Available commands:**

| Command | Fields | Description |
|---------|--------|-------------|
| `take_snapshot` | - | Trigger a state snapshot |
| `compact_log` | `up_to_offset` | Compact log up to offset |
| `step_down` | - | Step down from leader role |
| `transfer_leadership` | `target_node_id` | Transfer to target |
| `pause_replication` | - | Pause replication for maintenance |
| `resume_replication` | - | Resume replication |

The agent should respond with a `ControlAck` containing the `message_id`.

#### HeartbeatRequest

Request an immediate heartbeat from the agent.

```json
{
  "type": "heartbeat_request"
}
```

#### Shutdown

Request graceful shutdown.

```json
{
  "type": "shutdown",
  "reason": "Cluster scaling down"
}
```

#### AuthChallenge

Authentication challenge sent after connection.

```json
{
  "type": "auth_challenge",
  "challenge": "random-challenge-string",
  "supported_methods": ["bearer", "pre_shared_key", "certificate"],
  "expires_in_ms": 30000
}
```

**Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `challenge` | string | Random challenge for PSK auth |
| `supported_methods` | array | Supported auth methods |
| `expires_in_ms` | u64 | Challenge expiration time |

#### FlowControl

Backpressure signal for high-volume data.

```json
{
  "type": "flow_control",
  "stream_type": "metrics",
  "signal": {
    "slow_down": {
      "min_interval_ms": 10000
    }
  }
}
```

**Signal types:**

| Signal | Fields | Description |
|--------|--------|-------------|
| `resume` | - | Resume normal transmission |
| `slow_down` | `min_interval_ms` | Reduce transmission rate |
| `pause` | - | Stop transmission |

#### HealthCheck

Request health check from agent.

```json
{
  "type": "health_check",
  "request_id": 42,
  "checks": ["liveness", "storage", "replication"]
}
```

**Check types:**

| Type | Description |
|------|-------------|
| `liveness` | Basic process health |
| `storage` | Storage subsystem health |
| `replication` | Replication status |
| `resources` | Disk/memory availability |
| `all` | All available checks |

## Connection Lifecycle

### Authentication

After WebSocket connection is established:

1. Control plane sends `AuthChallenge`
2. Agent responds with `AuthResponse` containing credentials
3. Control plane validates credentials
4. On success, connection transitions to authenticated state

### Initial Handshake

1. Agent connects to WebSocket endpoint
2. Authentication exchange (see above)
3. Control plane sends `HeartbeatRequest`
4. Agent responds with `Heartbeat`
5. Connection is established

### Steady State

- Agent sends `Heartbeat` every 10 seconds (configurable)
- Agent batches and sends `MetricsBatch` every 5 seconds
- Agent batches and sends `LogsBatch` every 5 seconds
- Control plane pushes `ConfigUpdate` as needed
- Agent includes `buffer_stats` in heartbeats for backpressure monitoring
- Control plane sends `FlowControl` when overwhelmed

### Reconnection with Exponential Backoff

If the connection drops, agents should:

1. Wait with exponential backoff (initial: 1s, max: 60s, multiplier: 2.0)
2. Add jitter (±25%) to prevent thundering herd
3. Reconnect and perform full authentication handshake
4. Resume normal operation

**Backoff Configuration:**

```rust
BackoffConfig {
    initial_delay_ms: 1_000,   // 1 second
    max_delay_ms: 60_000,      // 60 seconds
    multiplier: 2.0,
    jitter_factor: 0.25,
    max_attempts: 0,           // unlimited
}
```

### Connection States

| State | Description |
|-------|-------------|
| `disconnected` | Not connected |
| `backoff` | Waiting before reconnect |
| `connecting` | Connection in progress |
| `connected` | Connected, not authenticated |
| `authenticated` | Ready for normal operation |
| `closing` | Graceful shutdown in progress |

## Health Monitoring

The control plane monitors agent health using these thresholds:

| Metric | Warning | Critical |
|--------|---------|----------|
| Heartbeat timeout | - | 30 seconds |
| Replication lag | 5 seconds | 30 seconds |
| Disk usage | 80% | 95% |
| Memory usage | 85% | 95% |

## Using the Protocol

### Rust Crate

The `kimberlite-agent-protocol` crate provides typed definitions:

```rust
use kmb_agent_protocol::{AgentMessage, NodeStatus, NodeRole, Resources};

let heartbeat = AgentMessage::Heartbeat {
    node_id: "node-001".to_string(),
    status: NodeStatus::Healthy,
    role: NodeRole::Leader,
    resources: Resources {
        cpu_percent: 45.2,
        memory_used_bytes: 1_073_741_824,
        memory_total_bytes: 8_589_934_592,
        disk_used_bytes: 10_737_418_240,
        disk_total_bytes: 107_374_182_400,
    },
    replication: None,
    buffer_stats: vec![],
};

let json = serde_json::to_string(&heartbeat)?;
```

### Other Languages

The protocol uses standard JSON, so any language with JSON support can implement an agent or control plane. The type definitions in this document serve as the canonical specification.

## Versioning

The protocol version is negotiated during the WebSocket handshake via the `Sec-WebSocket-Protocol` header:

```
Sec-WebSocket-Protocol: kimberlite-agent-protocol-v1
```

Breaking changes will increment the version number.
