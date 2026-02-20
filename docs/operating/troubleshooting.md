---
title: "Troubleshooting"
section: "operating"
slug: "troubleshooting"
order: 7
---

# Troubleshooting

Debug common operational issues in Kimberlite clusters.

## Quick Diagnostic Commands

```bash
# Check cluster status
curl http://localhost:9090/status | jq

# View recent logs
journalctl -u kimberlite -n 100 --no-pager

# Check metrics
curl http://localhost:9090/metrics | grep -E "(consensus_view|consensus_leader|projection_lag)"

# Verify configuration
kimberlite-server --config /etc/kimberlite/config.toml --show-config
```

## Common Issues

### Issue 1: Cluster Has No Leader

**Symptoms:**
- All writes fail with "No leader elected"
- `kmb_consensus_leader` metric shows 0
- Logs show repeated election timeouts

**Diagnostic:**

```bash
# Check if nodes can reach each other
for node in node1 node2 node3; do
  curl http://$node:9090/status
done

# Check view numbers (should be identical)
curl http://node1:9090/status | jq '.view'
curl http://node2:9090/status | jq '.view'
curl http://node3:9090/status | jq '.view'
```

**Common Causes:**

1. **Network partition** - Nodes cannot reach each other
   ```bash
   # Test connectivity
   nc -zv node2 7001
   nc -zv node3 7001
   ```
   **Solution:** Fix network configuration, ensure firewall allows port 7001

2. **Clock skew** - Node clocks differ by >500ms
   ```bash
   # Check time on all nodes
   date +%s
   ```
   **Solution:** Enable NTP on all nodes

3. **Lost quorum** - Majority of nodes are down
   ```bash
   # Check node status
   systemctl status kimberlite
   ```
   **Solution:** Start failed nodes to restore quorum

4. **Corrupted state** - Node state is corrupted
   ```bash
   # Check for corruption
   grep -i "corrupt\|invalid\|checksum" /var/log/kimberlite/server.log
   ```
   **Solution:** See [Recovering from Corruption](#recovering-from-corruption)

### Issue 2: High Write Latency

**Symptoms:**
- `kmb_write_duration_seconds` P99 > 100ms
- Client writes timing out
- Logs show slow fsync operations

**Diagnostic:**

```bash
# Check disk I/O
iostat -x 1 10

# Check write latency breakdown
curl http://localhost:9090/metrics | grep kmb_write_duration_seconds

# Check if disk is full
df -h /var/lib/kimberlite
```

**Common Causes:**

1. **Slow disk** - fsync taking >10ms
   ```bash
   # Test disk sync performance
   dd if=/dev/zero of=/var/lib/kimberlite/test bs=4k count=1000 oflag=dsync
   ```
   **Solution:** Upgrade to faster disks (NVMe SSD recommended)

2. **Disk full** - No space for new writes
   ```bash
   df -h /var/lib/kimberlite
   ```
   **Solution:** Add disk space or enable log compaction

3. **Network latency** - High inter-node latency
   ```bash
   # Ping other nodes
   ping -c 10 node2
   ```
   **Solution:** Deploy nodes in same availability zone

4. **Overloaded CPU** - CPU saturated with other work
   ```bash
   top -n 1
   ```
   **Solution:** Reduce load or add more nodes

### Issue 3: Projection Lag Growing

**Symptoms:**
- `kmb_projection_lag` increasing over time
- Queries return stale data
- Logs show projection apply backlog

**Diagnostic:**

```bash
# Check projection lag
curl http://localhost:9090/metrics | grep kmb_projection_lag

# Check CPU usage
top -n 1 | grep kimberlite

# Check query load
curl http://localhost:9090/metrics | grep kmb_query_duration_seconds
```

**Common Causes:**

1. **Heavy query load** - Queries blocking projection updates
   ```bash
   # Check query rate
   curl http://localhost:9090/metrics | grep kmb_requests_total
   ```
   **Solution:** Scale read replicas or reduce query load

2. **Slow queries** - Long-running queries holding locks
   ```bash
   # Find slow queries in logs
   grep "query_duration_ms" /var/log/kimberlite/server.log | sort -k4 -n | tail -20
   ```
   **Solution:** Add indexes or optimize queries

3. **Insufficient CPU** - Projection processing CPU-bound
   ```bash
   mpstat -P ALL 1
   ```
   **Solution:** Add more CPU cores or reduce write rate

### Issue 4: Frequent View Changes

**Symptoms:**
- `kmb_consensus_view_changes_total` increasing rapidly
- Logs show repeated leader elections
- Write latency spikes during view changes

**Diagnostic:**

```bash
# Check view change rate
curl http://localhost:9090/metrics | grep kmb_consensus_view_changes_total

# Check network packet loss
ping -c 100 node2 | grep loss
```

**Common Causes:**

1. **Network flakiness** - Intermittent packet loss
   ```bash
   # Test network stability
   ping -c 1000 -i 0.01 node2 | grep loss
   ```
   **Solution:** Fix network infrastructure

2. **GC pauses** - Long GC pauses causing timeouts
   ```bash
   # Check for GC issues (Rust has no GC, but allocator could stall)
   perf record -p $(pidof kimberlite-server) -g -- sleep 10
   perf report
   ```
   **Solution:** Increase election timeout or reduce memory pressure

3. **Overloaded node** - Leader can't send heartbeats
   ```bash
   # Check CPU on leader
   ssh node1 "top -n 1 | grep kimberlite"
   ```
   **Solution:** Reduce load or add more nodes

### Issue 5: Node Won't Join Cluster

**Symptoms:**
- New node fails to join existing cluster
- Logs show "Rejected by leader"
- Node stays in "Recovering" state

**Diagnostic:**

```bash
# Check node status
curl http://new-node:9090/status

# Check if leader sees the node
curl http://leader:9090/status | jq '.cluster_size'

# Check configuration
kimberlite-server --config /etc/kimberlite/config.toml --show-config
```

**Common Causes:**

1. **Mismatched cluster name**
   ```toml
   # config.toml
   [server]
   cluster_name = "production"  # Must match existing cluster
   ```

2. **Wrong node ID** - Node ID already in use
   ```bash
   # Check existing node IDs
   curl http://leader:9090/status | jq '.cluster_members'
   ```

3. **TLS certificate mismatch**
   ```bash
   # Verify certificate
   openssl x509 -in /etc/kimberlite/server.crt -text -noout
   ```

### Issue 6: Data Corruption Detected

**Symptoms:**
- `kmb_checksum_failures_total` increasing
- Logs show "Checksum mismatch" errors
- Queries return errors

**Diagnostic:**

```bash
# Check corruption metrics
curl http://localhost:9090/metrics | grep kmb_checksum_failures_total

# Find corrupted segments
kimberlite-admin verify --data-dir /var/lib/kimberlite
```

**Common Causes:**

1. **Disk failure** - Silent data corruption
   ```bash
   # Check disk health
   smartctl -a /dev/sda
   ```
   **Solution:** See [Recovering from Corruption](#recovering-from-corruption)

2. **Power loss during write** - Torn writes
   **Solution:** Enable battery-backed write cache or UPS

3. **Kernel bug** - Rare kernel I/O bug
   ```bash
   dmesg | grep -i error
   ```
   **Solution:** Update kernel

## Advanced Debugging

### Enable Debug Logging

```bash
# Temporary (until restart)
export RUST_LOG=kimberlite=debug
systemctl restart kimberlite

# Permanent
echo 'RUST_LOG=kimberlite=debug' >> /etc/kimberlite/environment
systemctl restart kimberlite
```

### Capture Packet Traces

```bash
# Capture cluster traffic
tcpdump -i eth0 port 7001 -w cluster-traffic.pcap

# Analyze with wireshark
wireshark cluster-traffic.pcap
```

### Profile CPU Usage

```bash
# Install perf
apt install linux-tools-generic

# Record CPU profile
perf record -p $(pidof kimberlite-server) -g -- sleep 30

# Generate flamegraph
perf script | stackcollapse-perf.pl | flamegraph.pl > flame.svg
```

### Analyze Memory Usage

```bash
# Check memory usage
ps aux | grep kimberlite-server

# Heap profile (requires debug build)
heaptrack kimberlite-server --config /etc/kimberlite/config.toml
```

## Recovery Procedures

### Recovering from Corruption

If a node detects corruption:

1. **Stop the node**
   ```bash
   systemctl stop kimberlite
   ```

2. **Verify corruption**
   ```bash
   kimberlite-admin verify --data-dir /var/lib/kimberlite
   ```

3. **Restore from replica** (if node is follower)
   ```bash
   # Delete corrupted data
   rm -rf /var/lib/kimberlite/log/*

   # Restart and let it recover from leader
   systemctl start kimberlite
   ```

4. **Restore from backup** (if node is leader or quorum lost)
   ```bash
   # Stop all nodes
   systemctl stop kimberlite

   # Restore data from backup
   rsync -av backup/ /var/lib/kimberlite/

   # Start cluster
   systemctl start kimberlite
   ```

### Recovering from Quorum Loss

If majority of nodes are down and cannot be recovered:

1. **DO NOT do this lightly** - Can cause data loss
2. **Force a node to become leader**
   ```bash
   # DANGER: Only if quorum permanently lost
   kimberlite-admin force-leader --node-id 1 --data-dir /var/lib/kimberlite
   ```

3. **Add new nodes to restore redundancy**
   ```bash
   # Start new nodes
   kimberlite-server --node-id 2 --cluster-peers "node1:7001" ...
   kimberlite-server --node-id 3 --cluster-peers "node1:7001" ...
   ```

## Getting Help

### Collect Diagnostic Bundle

```bash
# Generate diagnostic bundle
kimberlite-admin diagnostics \
  --output diagnostics-$(date +%Y%m%d-%H%M%S).tar.gz

# Bundle includes:
# - Configuration
# - Recent logs
# - Metrics snapshot
# - Cluster status
# - System info
```

### Enable Support Access

```bash
# Generate one-time support token (expires in 24h)
kimberlite-admin support-token --expires 24h

# Share token with support team
```

### Report a Bug

If you've found a bug, report it with:
- Diagnostic bundle
- Steps to reproduce
- Expected vs actual behavior
- Kimberlite version: `kimberlite-server --version`

See [Bug Bounty Program](https://github.com/kimberlitedb/kimberlite/security) for security issues.

## Related Documentation

- **[Monitoring Guide](monitoring.md)** - Metrics and alerts
- **[Configuration Guide](configuration.md)** - Configuration options
- **[Deployment Guide](deployment.md)** - Deployment patterns
- **[Security Guide](security.md)** - TLS and authentication

---

**Key Takeaway:** Most issues are network, disk, or configuration problems. Check metrics first, enable debug logging if needed, and always verify configuration before restarting.
