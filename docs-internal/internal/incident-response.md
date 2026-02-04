# Kimberlite VSR Incident Response Playbook

**Version:** 1.0
**Date:** 2026-02-05
**Status:** Production Ready

## Table of Contents

1. [Overview](#overview)
2. [Incident Response Process](#incident-response-process)
3. [Failure Mode Catalog](#failure-mode-catalog)
4. [Diagnostic Procedures](#diagnostic-procedures)
5. [Recovery Procedures](#recovery-procedures)
6. [Communication Templates](#communication-templates)
7. [Post-Mortem Process](#post-mortem-process)
8. [Escalation Paths](#escalation-paths)

---

## Overview

This playbook provides structured guidance for responding to Kimberlite VSR incidents. It covers failure modes, diagnostic steps, recovery procedures, and post-incident analysis.

### Incident Severity Levels

| Severity | Impact | Examples | Response Time |
|----------|--------|----------|---------------|
| **SEV1** | Complete service outage | All replicas down, data loss | < 15 minutes |
| **SEV2** | Degraded service | Quorum lost, high latency | < 30 minutes |
| **SEV3** | Partial degradation | Single replica failure | < 2 hours |
| **SEV4** | Minor issue | Warning alerts, low impact | Next business day |

### On-Call Rotation

- **Primary On-Call:** First responder (PagerDuty escalation)
- **Secondary On-Call:** Backup if primary unavailable (15-minute timeout)
- **Escalation Engineer:** Senior engineer for complex issues (30-minute timeout)
- **Manager On-Call:** Executive escalation for SEV1 incidents

---

## Incident Response Process

### Phase 1: Detection & Triage (0-5 minutes)

**Objectives:**
1. Confirm incident is real (not false positive)
2. Assess severity level
3. Begin initial investigation

**Steps:**
1. ✅ **Acknowledge alert** in PagerDuty (stops escalation)
2. ✅ **Check monitoring dashboards** (Grafana "VSR Cluster Overview")
   - Cluster health status
   - Operations throughput
   - Error rate
3. ✅ **Verify user impact** (application logs, user reports)
4. ✅ **Declare severity level** (SEV1-SEV4)
5. ✅ **Create incident ticket** (Jira, PagerDuty incident)
6. ✅ **Join incident bridge** (Zoom, Slack huddle) for SEV1/SEV2

**Triage Checklist:**
- [ ] Alert acknowledged
- [ ] Severity determined
- [ ] Incident ticket created
- [ ] War room established (if SEV1/SEV2)

---

### Phase 2: Investigation & Diagnosis (5-30 minutes)

**Objectives:**
1. Identify root cause
2. Determine recovery strategy
3. Communicate status

**Steps:**
1. ✅ **Run diagnostic procedures** (see [Diagnostic Procedures](#diagnostic-procedures))
2. ✅ **Check recent changes** (deployments, config changes)
3. ✅ **Review logs** (`journalctl -u kimberlite-vsr -n 500`)
4. ✅ **Consult runbook** (see [Failure Mode Catalog](#failure-mode-catalog))
5. ✅ **Update incident ticket** with findings
6. ✅ **Communicate status** (Slack, status page) every 15 minutes

**Investigation Checklist:**
- [ ] Root cause identified (or hypothesis formed)
- [ ] Recovery strategy determined
- [ ] Stakeholders updated (15-minute intervals)

---

### Phase 3: Mitigation & Recovery (30 minutes - 2 hours)

**Objectives:**
1. Restore service to operational state
2. Verify functionality
3. Monitor stability

**Steps:**
1. ✅ **Execute recovery procedure** (see [Recovery Procedures](#recovery-procedures))
2. ✅ **Verify metrics return to normal** (throughput, latency, error rate)
3. ✅ **Run smoke tests** (test queries, write operations)
4. ✅ **Monitor for 15 minutes** (ensure no regression)
5. ✅ **Declare incident resolved** (in ticket and communications)
6. ✅ **Schedule post-mortem** (within 24-48 hours)

**Recovery Checklist:**
- [ ] Service restored
- [ ] Metrics normalized
- [ ] Smoke tests passing
- [ ] Monitoring stable for 15+ minutes
- [ ] Incident resolved
- [ ] Post-mortem scheduled

---

### Phase 4: Post-Incident Review (24-48 hours after resolution)

**Objectives:**
1. Document what happened
2. Identify preventive measures
3. Create action items

**Steps:**
1. ✅ **Write post-mortem** (see [Post-Mortem Template](#post-mortem-template))
2. ✅ **Schedule review meeting** (all stakeholders)
3. ✅ **Identify action items** (preventive measures, monitoring gaps)
4. ✅ **Assign owners** for action items
5. ✅ **Share post-mortem** (engineering team, leadership)
6. ✅ **Track completion** of action items (Jira tickets)

---

## Failure Mode Catalog

### FM-001: Quorum Lost (SEV1)

**Symptoms:**
- `vsr_replica_status` shows <quorum replicas in Normal state
- `vsr_operations_total` rate drops to zero
- Client requests timing out

**Root Causes:**
- Network partition (split-brain)
- Multiple replica crashes (hardware, OOM)
- Cascading failure (disk full, memory exhaustion)

**Immediate Actions:**
1. Check how many replicas are down (`systemctl status kimberlite-vsr` on each node)
2. If 2/3 replicas down in 3-node cluster: **CRITICAL - restore at least one backup immediately**
3. Check network connectivity (`ping` between nodes)
4. Check disk space (`df -h`)
5. Check memory (`free -h`, `dmesg | grep -i oom`)

**Recovery:**
- **Scenario 1: Network partition** → Fix network, replicas auto-recover
- **Scenario 2: Crashed replicas** → Restart replicas (`systemctl restart kimberlite-vsr`)
- **Scenario 3: Disk full** → Clear space or add disk, restart replicas

**Prevention:**
- Monitor disk space (alert at 80%)
- Monitor memory usage (alert at 90%)
- Network redundancy (bonded NICs, multiple switches)
- Regular capacity planning

---

### FM-002: Leader Stuck in ViewChange (SEV2)

**Symptoms:**
- `vsr_view_number` incrementing rapidly
- `vsr_replica_status` = 1 (ViewChange) on all replicas
- No operations committing

**Root Causes:**
- Deadlock in view change protocol
- Backup replicas not responding to DoViewChange
- Clock skew preventing view change completion

**Immediate Actions:**
1. Check if new leader elected (`vsr_view_number` stable?)
2. Check backup replica logs for DoViewChange messages
3. Check clock synchronization (`vsr_clock_offset_ms` >100ms?)
4. Check network latency between replicas (`ping -c 100`)

**Recovery:**
- **Scenario 1: Deadlock** → Restart all replicas in sequence (leader first, then backups)
- **Scenario 2: Clock skew** → Fix NTP, wait for convergence (or force time sync)
- **Scenario 3: Network issues** → Fix network, wait for auto-recovery

**Prevention:**
- Monitor view change frequency (alert on >5/hour)
- Monitor clock offset (alert on >100ms)
- Test view change scenarios in staging

---

### FM-003: Data Corruption Detected (SEV1)

**Symptoms:**
- `vsr_checksum_failures_total` incrementing
- Replica logs showing checksum validation errors
- Operations failing on specific replica

**Root Causes:**
- Disk hardware failure (bit flips, bad sectors)
- Memory corruption (ECC errors)
- Software bug in serialization

**Immediate Actions:**
1. **ISOLATE CORRUPTED REPLICA** - stop accepting requests
2. Identify which replica has corruption (check logs on all replicas)
3. Check disk health (`smartctl -a /dev/nvme0n1`)
4. Check memory errors (`dmesg | grep -i ecc`)
5. **DO NOT RESTART** corrupted replica (preserve evidence)

**Recovery:**
- **Step 1:** Stop corrupted replica (`systemctl stop kimberlite-vsr`)
- **Step 2:** Verify cluster has quorum without it (should auto-recover)
- **Step 3:** Trigger state transfer or repair from healthy replicas
- **Step 4:** If repair successful, bring replica back online
- **Step 5:** If repair fails, wipe replica data and re-sync from scratch

**Prevention:**
- Enable background scrubbing (Phase 3 feature)
- Monitor `vsr_scrub_tours_completed` (should increment daily)
- Use ECC memory
- Regular disk health checks (`smartctl` monitoring)

---

### FM-004: High Latency Spike (SEV2)

**Symptoms:**
- `vsr_client_latency_ms` p95 >50ms (normally <15ms)
- `vsr_prepare_latency_ms` p95 >20ms (normally <5ms)
- User reports slow requests

**Root Causes:**
- Network congestion (saturated NIC)
- CPU contention (high load)
- Disk I/O bottleneck (slow NVMe)
- Garbage collection pause (memory pressure)

**Immediate Actions:**
1. Check network utilization (`iftop`, `nload`)
2. Check CPU usage (`top`, load average)
3. Check disk I/O latency (`iostat -x 1`)
4. Check for GC pauses (if using JVM-based app layer)

**Recovery:**
- **Scenario 1: Network saturated** → Rate-limit traffic, upgrade NIC
- **Scenario 2: CPU maxed out** → Scale vertically or horizontally
- **Scenario 3: Disk slow** → Check disk health, consider upgrade
- **Scenario 4: Memory pressure** → Increase memory, tune GC

**Prevention:**
- Monitor network bandwidth (alert at 80%)
- Monitor CPU utilization (alert at 85%)
- Monitor disk I/O latency (alert at >10ms)
- Capacity planning (add nodes proactively)

---

### FM-005: Single Replica Crash (SEV3)

**Symptoms:**
- 1 of 3 replicas down
- Cluster still operational (quorum maintained)
- `vsr_replica_status` unavailable for one replica

**Root Causes:**
- Process crash (bug, OOM)
- Hardware failure (disk, memory, motherboard)
- Planned maintenance (OS updates, reboots)

**Immediate Actions:**
1. Check replica status (`systemctl status kimberlite-vsr`)
2. Check logs for crash reason (`journalctl -u kimberlite-vsr -n 200`)
3. Check disk/memory/CPU health
4. Attempt restart if not hardware failure

**Recovery:**
- **Scenario 1: Process crash** → Restart replica, monitor for repeat
- **Scenario 2: Hardware failure** → Replace hardware, provision new replica
- **Scenario 3: Planned maintenance** → Wait for completion, restart

**Prevention:**
- Monitor replica health (alert on any replica down >5 minutes)
- Regular health checks (disk, memory)
- Rolling upgrades for planned maintenance

---

### FM-006: Commit Lag Accumulation (SEV3)

**Symptoms:**
- `vsr_op_number - vsr_commit_number` >100 (normally <10)
- `vsr_pending_requests` increasing
- Operations not completing

**Root Causes:**
- Backup replica lagging (slow disk, network)
- Quorum partially available (1 backup slow)
- Application logic blocking (long-running commands)

**Immediate Actions:**
1. Check which backup is lagging (compare `vsr_commit_number` per replica)
2. Check lagging replica CPU, disk, network
3. Check for long-running application commands (logs)

**Recovery:**
- **Scenario 1: Slow backup** → Restart backup, trigger state transfer if needed
- **Scenario 2: Application blocking** → Identify slow command, optimize or timeout
- **Scenario 3: Network issues** → Fix network, wait for catch-up

**Prevention:**
- Monitor commit lag per replica (alert at >50 ops)
- Timeout long-running commands (application-level)
- Regular performance testing

---

### FM-007: Disk Full (SEV2)

**Symptoms:**
- `node_filesystem_avail_bytes` → 0
- Write operations failing
- Replica logs showing "no space left on device"

**Root Causes:**
- Log growth without checkpointing
- Unexpected data volume increase
- Failed log cleanup (checkpoint/truncation bug)

**Immediate Actions:**
1. Check disk usage (`df -h`)
2. Identify largest files/directories (`du -sh /var/lib/kimberlite/*`)
3. Check if checkpoint is running (`ps aux | grep checkpoint`)

**Recovery:**
- **Emergency:** Delete old logs manually (`rm /var/lib/kimberlite/log.old.*`)
- **Proper:** Trigger checkpoint, then log cleanup
- **Long-term:** Add disk capacity or enable automatic cleanup

**Prevention:**
- Monitor disk space (alert at 80%, critical at 90%)
- Configure automatic checkpointing
- Retention policy for old checkpoints

---

### FM-008: Clock Drift Exceeds Tolerance (SEV3)

**Symptoms:**
- `vsr_clock_offset_ms` >200ms
- Clock synchronization warnings in logs
- Potential timestamp inconsistencies

**Root Causes:**
- NTP daemon stopped or misconfigured
- Network partition from NTP servers
- System clock jump (hardware issue)

**Immediate Actions:**
1. Check NTP status (`systemctl status ntp` or `systemctl status chronyd`)
2. Check NTP peer status (`ntpq -p` or `chronyc sources`)
3. Check clock offset manually (`ntpdate -q pool.ntp.org`)

**Recovery:**
- **Scenario 1: NTP stopped** → Restart NTP daemon
- **Scenario 2: NTP unreachable** → Fix network or switch NTP servers
- **Scenario 3: Clock jumped** → Force time sync (be careful - can break consensus)

**Prevention:**
- Monitor NTP daemon health
- Monitor clock offset (alert at >100ms)
- Multiple NTP servers for redundancy
- Use hardware with stable clocks (GPS, PTP)

---

## Diagnostic Procedures

### Procedure: Check Cluster Health

**Purpose:** Quick assessment of overall cluster status

**Steps:**
```bash
# 1. Check replica status on all nodes
for node in replica-1 replica-2 replica-3; do
  echo "=== $node ==="
  ssh $node "systemctl status kimberlite-vsr | grep Active"
done

# 2. Check metrics endpoint
for node in replica-1 replica-2 replica-3; do
  echo "=== $node ==="
  curl -s http://$node:9090/metrics | grep vsr_replica_status
  curl -s http://$node:9090/metrics | grep vsr_view_number
  curl -s http://$node:9090/metrics | grep vsr_commit_number
done

# 3. Check Prometheus targets
curl -s http://prometheus:9090/api/v1/targets | jq '.data.activeTargets[] | select(.labels.job=="kimberlite-vsr") | {instance: .labels.instance, health: .health}'
```

**Expected Output:**
- All replicas "Active: active (running)"
- All replicas `vsr_replica_status{} 0` (Normal)
- All replicas have same `vsr_view_number`
- All replicas have similar `vsr_commit_number` (±10 ops)

---

### Procedure: Identify Leader

**Purpose:** Determine which replica is currently the leader

**Steps:**
```bash
# Calculate leader from view number and cluster config
# Leader = view % replica_count

# Method 1: Check which replica is sending Prepare messages
for node in replica-1 replica-2 replica-3; do
  echo "=== $node ==="
  curl -s http://$node:9090/metrics | grep vsr_messages_sent_prepare
done
# Leader will have high vsr_messages_sent_prepare rate

# Method 2: Check replica logs
for node in replica-1 replica-2 replica-3; do
  echo "=== $node ==="
  ssh $node "journalctl -u kimberlite-vsr -n 10 | grep -i 'leader\\|primary'"
done
```

**Expected Output:**
- One replica has high Prepare message rate (leader)
- Other replicas have zero or low Prepare rate (backups)

---

### Procedure: Check Network Connectivity

**Purpose:** Verify network connectivity between replicas

**Steps:**
```bash
# 1. Ping test (ICMP)
for node in replica-1 replica-2 replica-3; do
  echo "=== Ping to $node ==="
  ping -c 10 $node | tail -1
done

# 2. TCP connectivity test (port 8080 - VSR protocol port)
for node in replica-1 replica-2 replica-3; do
  echo "=== TCP to $node:8080 ==="
  timeout 5 bash -c "echo > /dev/tcp/$node/8080" && echo "OK" || echo "FAILED"
done

# 3. Bandwidth test (iperf3)
# From replica-1 to replica-2
ssh replica-2 "iperf3 -s -1" &
sleep 2
ssh replica-1 "iperf3 -c replica-2 -t 10"

# 4. Check packet loss
for node in replica-1 replica-2 replica-3; do
  echo "=== $node packet stats ==="
  ssh $node "netstat -s | grep -i 'packet loss\\|retrans'"
done
```

**Expected Output:**
- Ping latency <1ms (same datacenter), <5ms (same region)
- TCP connectivity succeeds on port 8080
- Bandwidth ≥10 Gbps
- Packet loss <0.01%

---

### Procedure: Analyze Logs for Errors

**Purpose:** Extract and analyze error messages from replica logs

**Steps:**
```bash
# 1. Get recent errors (last hour)
for node in replica-1 replica-2 replica-3; do
  echo "=== $node errors (last hour) ==="
  ssh $node "journalctl -u kimberlite-vsr --since '1 hour ago' | grep -i 'error\\|fatal\\|panic' | tail -20"
done

# 2. Count error types
for node in replica-1 replica-2 replica-3; do
  echo "=== $node error frequency ==="
  ssh $node "journalctl -u kimberlite-vsr --since '1 hour ago' | grep -i error | awk '{print \$NF}' | sort | uniq -c | sort -rn | head -10"
done

# 3. Check for specific error patterns
# Checksum failures
for node in replica-1 replica-2 replica-3; do
  echo "=== $node checksum failures ==="
  ssh $node "journalctl -u kimberlite-vsr --since '1 day ago' | grep -i 'checksum' | wc -l"
done

# OOM kills
for node in replica-1 replica-2 replica-3; do
  echo "=== $node OOM kills ==="
  ssh $node "dmesg | grep -i 'killed process.*kimberlite'"
done
```

**Expected Output:**
- Few or no errors (0-10 per hour normal)
- No checksum failures
- No OOM kills

---

### Procedure: Check Resource Utilization

**Purpose:** Verify CPU, memory, disk, and network are not bottlenecks

**Steps:**
```bash
# 1. CPU usage
for node in replica-1 replica-2 replica-3; do
  echo "=== $node CPU ==="
  ssh $node "top -bn1 | grep kimberlite | head -5"
  ssh $node "cat /proc/loadavg"
done

# 2. Memory usage
for node in replica-1 replica-2 replica-3; do
  echo "=== $node Memory ==="
  ssh $node "free -h"
  ssh $node "ps aux | grep kimberlite | awk '{print \$4\" \"\$11}'"
done

# 3. Disk I/O
for node in replica-1 replica-2 replica-3; do
  echo "=== $node Disk I/O ==="
  ssh $node "iostat -x 1 5 | grep nvme0n1"
done

# 4. Network I/O
for node in replica-1 replica-2 replica-3; do
  echo "=== $node Network ==="
  ssh $node "sar -n DEV 1 5 | grep eth0"
done

# 5. Disk space
for node in replica-1 replica-2 replica-3; do
  echo "=== $node Disk Space ==="
  ssh $node "df -h /var/lib/kimberlite"
done
```

**Expected Output:**
- CPU utilization <80%
- Load average < number of cores
- Memory utilization <90%
- Disk I/O latency <10ms
- Network utilization <80% of capacity
- Disk space >20% free

---

## Recovery Procedures

### Procedure: Restart Single Replica

**Purpose:** Restart a crashed or hung replica

**Prerequisites:**
- Cluster has quorum (at least 2 of 3 replicas healthy)
- Replica is not the leader (or backup can handle leader role)

**Steps:**
```bash
# 1. Stop replica gracefully
ssh replica-2 "systemctl stop kimberlite-vsr"

# 2. Wait for graceful shutdown (up to 30 seconds)
sleep 5
ssh replica-2 "systemctl status kimberlite-vsr"

# 3. If still running, force kill
ssh replica-2 "systemctl kill -s SIGKILL kimberlite-vsr"

# 4. Start replica
ssh replica-2 "systemctl start kimberlite-vsr"

# 5. Verify startup
ssh replica-2 "journalctl -u kimberlite-vsr -n 50 -f"
# Look for "Replica started" or "Entering Normal status"

# 6. Check metrics
curl -s http://replica-2:9090/metrics | grep vsr_replica_status
# Should show vsr_replica_status{} 0 (Normal) within 1-2 minutes

# 7. Monitor catch-up progress
watch "curl -s http://replica-2:9090/metrics | grep vsr_commit_number"
# Should increase rapidly until caught up with leader
```

**Validation:**
- Replica status is Normal (0) within 2 minutes
- Commit number catches up to leader within 5 minutes
- No errors in logs

**Rollback:**
- If replica doesn't start: Check logs for errors, fix configuration
- If replica doesn't catch up: Trigger state transfer (see next procedure)

---

### Procedure: Trigger State Transfer

**Purpose:** Fully synchronize a lagging replica from a healthy replica

**Prerequisites:**
- Cluster has quorum (at least 2 of 3 replicas healthy)
- Lagging replica is responsive (not crashed)

**Steps:**
```bash
# 1. Identify lag
lag=$(ssh replica-2 "curl -s http://localhost:9090/metrics | grep vsr_commit_number" | awk '{print $2}')
leader_commit=$(ssh replica-1 "curl -s http://localhost:9090/metrics | grep vsr_commit_number" | awk '{print $2}')
echo "Lag: $((leader_commit - lag)) operations"

# 2. If lag >1000 operations, trigger state transfer
# (Automatic in normal operation, but can force via admin API if needed)

# 3. Monitor state transfer progress
ssh replica-2 "journalctl -u kimberlite-vsr -f | grep -i 'state transfer\\|StateTransfer'"

# 4. Check replica status during transfer
watch "curl -s http://replica-2:9090/metrics | grep vsr_replica_status"
# Should show vsr_replica_status{} 3 (StateTransfer) during transfer

# 5. Wait for completion (can take minutes to hours depending on data size)
while [ $(curl -s http://replica-2:9090/metrics | grep vsr_replica_status | awk '{print $2}') -eq 3 ]; do
  echo "State transfer in progress..."
  sleep 10
done

# 6. Verify replica caught up
curl -s http://replica-2:9090/metrics | grep vsr_commit_number
# Should match leader's commit_number
```

**Validation:**
- Replica status returns to Normal (0)
- Commit number matches leader
- No errors in logs

**Estimated Duration:**
- 1 GB data: ~5 minutes
- 10 GB data: ~30 minutes
- 100 GB data: ~3 hours

---

### Procedure: Replace Failed Replica

**Purpose:** Remove a permanently failed replica and add a new one

**Prerequisites:**
- Cluster has quorum without failed replica
- New hardware provisioned and reachable
- Cluster configuration allows reconfiguration

**Steps:**
```bash
# 1. Remove failed replica from cluster (via reconfiguration)
# This requires cluster reconfiguration support (Phase 4 feature)
ssh replica-1 "kimberlite-admin reconfig remove replica-2"

# 2. Wait for reconfiguration to complete (Joint→Stable transition)
watch "curl -s http://replica-1:9090/metrics | grep vsr_reconfig_state"
# Should transition 1 (Joint) → 0 (Stable)

# 3. Provision new replica (replica-4)
# - Install software
# - Configure with cluster addresses
# - Initialize empty data directory

# 4. Add new replica to cluster
ssh replica-1 "kimberlite-admin reconfig add replica-4"

# 5. Wait for reconfiguration to complete
watch "curl -s http://replica-1:9090/metrics | grep vsr_reconfig_state"

# 6. Verify new replica synced
curl -s http://replica-4:9090/metrics | grep vsr_commit_number
# Should match leader after state transfer

# 7. Update monitoring (Prometheus scrape config)
# Add replica-4:9090 to targets

# 8. Update load balancer / DNS
# Add replica-4 to client connection pool
```

**Validation:**
- New replica status is Normal (0)
- New replica commit number matches leader
- Cluster size reflects new configuration
- Monitoring shows all 3 replicas healthy

**Estimated Duration:** 2-4 hours (depending on data size and network speed)

---

### Procedure: Recover from Total Cluster Failure

**Purpose:** Restore service after all replicas failed (disaster recovery)

**Prerequisites:**
- Backups available (checkpoint + incremental logs)
- At least quorum hardware available
- Decision made on recovery point (which backup to restore)

**Steps:**
```bash
# 1. STOP ALL REPLICAS (ensure no split-brain)
for node in replica-1 replica-2 replica-3; do
  ssh $node "systemctl stop kimberlite-vsr"
done

# 2. Identify most recent checkpoint on each replica
for node in replica-1 replica-2 replica-3; do
  echo "=== $node checkpoints ==="
  ssh $node "ls -lh /var/lib/kimberlite/checkpoints/ | tail -5"
done

# 3. Choose replica with highest checkpoint number (most recent)
# Let's say replica-1 has highest checkpoint

# 4. Restore replica-1 from backup (if needed)
# If data directory intact, skip this step
# If data lost, restore from backup:
ssh replica-1 "systemctl stop kimberlite-vsr"
ssh replica-1 "rm -rf /var/lib/kimberlite/data/*"
ssh replica-1 "tar -xzf /backups/checkpoint-12345.tar.gz -C /var/lib/kimberlite/"

# 5. Start replica-1 FIRST (becomes seed for others)
ssh replica-1 "systemctl start kimberlite-vsr"

# 6. Wait for replica-1 to enter Normal status
watch "curl -s http://replica-1:9090/metrics | grep vsr_replica_status"
# Should show vsr_replica_status{} 0 (Normal)

# 7. Start replica-2 and replica-3 (will state-transfer from replica-1)
ssh replica-2 "systemctl start kimberlite-vsr"
sleep 5
ssh replica-3 "systemctl start kimberlite-vsr"

# 8. Monitor state transfer on replicas 2 and 3
ssh replica-2 "journalctl -u kimberlite-vsr -f | grep -i 'state transfer'"
ssh replica-3 "journalctl -u kimberlite-vsr -f | grep -i 'state transfer'"

# 9. Wait for all replicas to reach Normal status
for node in replica-1 replica-2 replica-3; do
  echo "=== $node ==="
  curl -s http://$node:9090/metrics | grep vsr_replica_status
done

# 10. Verify cluster health
# Check that all replicas have same view and commit number
for node in replica-1 replica-2 replica-3; do
  echo "=== $node ==="
  curl -s http://$node:9090/metrics | grep vsr_view_number
  curl -s http://$node:9090/metrics | grep vsr_commit_number
done

# 11. Run smoke tests
# Test write operation
curl -X POST http://replica-1:8080/api/write -d '{"stream":"test","event":"recovery-test"}'

# Test read operation
curl http://replica-1:8080/api/read?stream=test
```

**Validation:**
- All replicas status is Normal (0)
- All replicas have same view and commit number
- Write and read operations succeed
- No errors in logs

**Estimated Duration:** 1-6 hours (depending on data size)

**Data Loss:**
- Data committed before last checkpoint: ✅ Recovered
- Data after last checkpoint: ❌ **LOST** (RPO = checkpoint interval)

---

## Communication Templates

### Template: Initial Incident Notification (SEV1/SEV2)

**Subject:** [SEV1] Kimberlite VSR Cluster Outage - Investigating

**Message:**
```
🚨 INCIDENT ALERT 🚨

Severity: SEV1
Service: Kimberlite VSR (Production Cluster)
Status: Investigating
Detected: 2026-02-05 14:32 UTC

Impact:
- All write operations failing
- Quorum lost (2 of 3 replicas down)
- Estimated affected users: ~5,000

Actions Taken:
- On-call engineer paged and investigating
- War room established: https://zoom.us/j/incident-12345
- Incident ticket: JIRA-6789

Current Hypothesis:
- Network partition suspected (replicas unreachable)
- Investigating network switch in DC-EAST-1

Next Update: 14:45 UTC (15 minutes)

War Room: https://zoom.us/j/incident-12345
Incident Commander: Alice Smith (alice@company.com)
```

---

### Template: Incident Update (Every 15 minutes)

**Subject:** [SEV1] Kimberlite VSR Cluster Outage - UPDATE #2

**Message:**
```
Severity: SEV1
Service: Kimberlite VSR (Production Cluster)
Status: Mitigating
Updated: 2026-02-05 14:45 UTC

Progress:
✅ Root cause identified: Network switch failure in DC-EAST-1
✅ Network team rerouting traffic through backup switch
⏳ Waiting for replicas to reconnect (ETA: 5 minutes)

Impact:
- Write operations still failing
- Read operations degraded (single replica serving)

Next Update: 15:00 UTC (15 minutes)
```

---

### Template: Incident Resolution

**Subject:** [RESOLVED] Kimberlite VSR Cluster Outage

**Message:**
```
✅ INCIDENT RESOLVED

Severity: SEV1 → RESOLVED
Service: Kimberlite VSR (Production Cluster)
Resolved: 2026-02-05 15:12 UTC
Duration: 40 minutes (14:32 - 15:12 UTC)

Root Cause:
- Network switch hardware failure in DC-EAST-1
- Caused network partition isolating 2 of 3 replicas
- Cluster lost quorum and stopped accepting writes

Resolution:
- Network team rerouted traffic through backup switch
- Replicas reconnected automatically
- Quorum restored at 15:10 UTC
- Service fully operational at 15:12 UTC

Verification:
✅ All 3 replicas healthy (Normal status)
✅ Write operations resuming (1,200 ops/sec)
✅ Read operations normal (latency <10ms)
✅ No data loss (all committed operations preserved)

Impact Summary:
- Affected users: ~5,000
- Downtime: 40 minutes
- Failed requests: ~2,400 (writes only)

Next Steps:
- Post-mortem scheduled: 2026-02-06 10:00 AM
- Network team investigating switch failure
- Reviewing redundancy for single points of failure

Thank you for your patience.

Incident Commander: Alice Smith
Incident Ticket: JIRA-6789
```

---

## Post-Mortem Process

### Post-Mortem Template

```markdown
# Post-Mortem: [Service] [Brief Description]

**Date:** YYYY-MM-DD
**Authors:** [Name(s)]
**Status:** Draft | In Review | Final
**Severity:** SEV1 | SEV2 | SEV3 | SEV4

---

## Summary

[2-3 sentence summary of what happened, impact, and resolution]

Example:
> On February 5, 2026 at 14:32 UTC, a network switch failure in DC-EAST-1 caused a network partition that isolated 2 of 3 VSR replicas. This resulted in quorum loss and a 40-minute outage affecting ~5,000 users. The issue was resolved by rerouting traffic through a backup switch, allowing replicas to reconnect and restore quorum.

---

## Timeline (All times UTC)

| Time | Event |
|------|-------|
| 14:30 | Network switch in DC-EAST-1 experiences hardware failure |
| 14:31 | Replicas 2 and 3 lose connectivity to replica 1 (leader) |
| 14:32 | Monitoring alerts fire: QuorumLost, ClusterDown |
| 14:32 | On-call engineer paged (Alice Smith) |
| 14:35 | Alice acknowledges alert, begins investigation |
| 14:40 | Root cause identified: network switch failure |
| 14:42 | Network team engaged, begins rerouting traffic |
| 14:50 | Traffic successfully rerouted through backup switch |
| 15:05 | Replicas 2 and 3 reconnect to replica 1 |
| 15:10 | Quorum restored, cluster enters Normal status |
| 15:12 | Service fully operational, incident declared resolved |

**Total Duration:** 40 minutes (detection to resolution)

---

## Impact

### User Impact
- **Affected Users:** ~5,000 active users
- **Failed Requests:** ~2,400 write operations (read operations degraded but not failed)
- **Downtime:** 40 minutes (14:32 - 15:12 UTC)

### Business Impact
- **Revenue Loss:** $X,XXX (estimated from failed transactions)
- **SLA Impact:** Violated 99.95% monthly availability SLO (40 min / 43,200 min = 0.092%)
- **Customer Escalations:** 3 enterprise customers contacted support

### Technical Impact
- **Data Loss:** None (all committed operations preserved)
- **Data Corruption:** None
- **Recovery Time:** 40 minutes

---

## Root Cause Analysis

### What Happened
[Detailed technical explanation of the failure]

Example:
> The network switch in DC-EAST-1 (Cisco Nexus 9300) experienced a hardware failure at 14:30 UTC. The switch's ASIC chip overheated due to a failed cooling fan, causing the switch to enter a protection mode and stop forwarding packets. This created a network partition where replica 1 (leader) in DC-EAST-2 could not communicate with replicas 2 and 3 in DC-EAST-1.
>
> The VSR protocol requires quorum (2 of 3 replicas) to commit operations. With the network partition, the leader (replica 1) could not reach a quorum, so the cluster stopped accepting writes. Replicas 2 and 3 attempted to start a view change, but they also could not reach quorum without replica 1.

### Why It Wasn't Prevented
[Explanation of why existing safeguards didn't work]

Example:
> 1. **Single Point of Failure:** All replicas in DC-EAST-1 were connected to a single network switch with no redundancy.
> 2. **Monitoring Gap:** Switch health monitoring was in place, but the cooling fan failure was not detected until the ASIC overheated.
> 3. **No Automatic Failover:** Network team had a backup switch available but required manual intervention to reroute traffic.

### Contributing Factors
- Replicas 2 and 3 co-located in same datacenter (DC-EAST-1)
- Network switch had no redundant power supply or cooling
- Switch firmware version known to have ASIC overheating issues (CVE-XXXX)

---

## What Went Well

✅ **Monitoring detected the issue immediately** (within 2 minutes)
✅ **On-call response was fast** (acknowledged within 3 minutes)
✅ **Root cause identified quickly** (within 8 minutes)
✅ **No data loss** (all committed operations preserved)
✅ **Communication was clear and timely** (updates every 15 minutes)
✅ **Cluster auto-recovered** once network restored (no manual intervention needed)

---

## What Went Wrong

❌ **Single point of failure in network topology** (all DC-EAST-1 replicas on one switch)
❌ **Switch hardware failure not detected early** (cooling fan failure should have alerted)
❌ **No automatic network failover** (required manual rerouting by network team)
❌ **Replica placement not geographically diverse** (2 of 3 in same DC)

---

## Action Items

| ID | Action | Owner | Priority | Due Date | Status |
|----|--------|-------|----------|----------|--------|
| AI-1 | Add network redundancy: Connect DC-EAST-1 replicas to two switches with bonded NICs | Network Team | P0 | 2026-02-12 | In Progress |
| AI-2 | Implement geographic replica distribution: Move replica 3 to DC-WEST-1 | SRE Team | P0 | 2026-02-19 | Not Started |
| AI-3 | Enable automatic network failover: Configure VRRP/HSRP on switches | Network Team | P1 | 2026-02-26 | Not Started |
| AI-4 | Add switch health monitoring: Alert on cooling fan failures, ASIC temp >80°C | Monitoring Team | P1 | 2026-02-15 | In Progress |
| AI-5 | Upgrade switch firmware to patched version (fixes ASIC overheating) | Network Team | P1 | 2026-02-10 | In Progress |
| AI-6 | Document network topology and single points of failure | SRE Team | P2 | 2026-02-28 | Not Started |
| AI-7 | Test disaster recovery scenario (total network failure) in staging | SRE Team | P2 | 2026-03-15 | Not Started |

---

## Lessons Learned

### What We Learned
1. **Network redundancy is critical:** A single switch failure can take down multiple replicas simultaneously.
2. **Geographic distribution matters:** Co-locating replicas in the same datacenter creates correlated failures.
3. **Monitoring physical infrastructure is as important as software:** Cooling fan failures can cascade into major outages.
4. **Automatic failover reduces MTTR:** Manual intervention added 10+ minutes to recovery time.

### What We'll Do Differently
1. **Require multi-switch redundancy** for all production deployments (bonded NICs, separate switches)
2. **Enforce geographic distribution** in cluster placement policy (max 1 replica per datacenter)
3. **Expand monitoring to physical layer** (fan speeds, temperatures, power supplies)
4. **Automate network failover** where possible (VRRP, BGP Anycast)

---

## Appendix

### Relevant Logs

```
[2026-02-05 14:31:03] replica-2: ERROR Network timeout connecting to replica-1 (10.0.1.1:8080)
[2026-02-05 14:31:05] replica-2: WARN Heartbeat timeout from leader, starting view change
[2026-02-05 14:31:08] replica-3: ERROR Network timeout connecting to replica-1 (10.0.1.1:8080)
[2026-02-05 14:31:10] replica-3: WARN Cannot reach quorum for view change (need 2, have 1)
[2026-02-05 14:32:00] monitoring: ALERT QuorumLost - cluster has 1/3 replicas healthy
```

### Related Documents
- Incident ticket: JIRA-6789
- Network topology diagram: Confluence page
- Switch firmware CVE: CVE-XXXX
```

---

## Escalation Paths

### Technical Escalation

**Level 1: On-Call Engineer** (Primary responder)
- **Responsibility:** Triage, diagnose, attempt recovery
- **Escalate if:** Unable to resolve within 30 minutes (SEV1) or 2 hours (SEV2)

**Level 2: Senior Engineer / Subject Matter Expert**
- **Responsibility:** Deep technical analysis, complex recovery procedures
- **Escalate if:** Requires architectural changes, data loss risk, or external dependencies

**Level 3: Engineering Manager / Architect**
- **Responsibility:** High-level decisions, cross-team coordination, vendor engagement
- **Escalate if:** Vendor bug, requires emergency change approval, or >4 hour outage

---

### Executive Escalation

**Trigger Conditions:**
- SEV1 incident >2 hours
- Data loss affecting >100 customers
- Security breach or compliance violation
- Media attention or viral social media

**Escalation Chain:**
1. Engineering Manager → VP Engineering
2. VP Engineering → CTO
3. CTO → CEO (if business-critical or PR issue)

---

## Appendix: Emergency Contacts

| Role | Name | Phone | Email | Backup |
|------|------|-------|-------|--------|
| **On-Call Engineer** | PagerDuty rotation | - | oncall@company.com | Secondary on-call |
| **Engineering Manager** | Bob Johnson | +1-555-0100 | bob@company.com | Alice Smith |
| **VP Engineering** | Carol White | +1-555-0200 | carol@company.com | David Lee |
| **Network Team Lead** | Eve Martinez | +1-555-0300 | eve@company.com | Frank Chen |
| **Database Team Lead** | Grace Kim | +1-555-0400 | grace@company.com | Henry Patel |
| **Security Team** | security@company.com | - | security@company.com | 24/7 rotation |

---

## Appendix: Useful Commands

### Quick Health Check
```bash
# One-liner to check cluster status
for i in {1..3}; do echo "=== Replica $i ==="; curl -s http://replica-$i:9090/metrics | grep -E 'vsr_replica_status|vsr_view_number|vsr_commit_number'; done
```

### Tail Logs (All Replicas)
```bash
# Follow logs on all replicas simultaneously (requires tmux)
tmux new-session -d -s incident \; \
  send-keys 'ssh replica-1 "journalctl -u kimberlite-vsr -f"' C-m \; \
  split-window -h \; \
  send-keys 'ssh replica-2 "journalctl -u kimberlite-vsr -f"' C-m \; \
  split-window -v \; \
  send-keys 'ssh replica-3 "journalctl -u kimberlite-vsr -f"' C-m \; \
  attach
```

### Emergency Shutdown (All Replicas)
```bash
# Use with caution - stops entire cluster
for i in {1..3}; do ssh replica-$i "systemctl stop kimberlite-vsr"; done
```

---

**Document Version History:**
- v1.0 (2026-02-05): Initial release for Phase 5
