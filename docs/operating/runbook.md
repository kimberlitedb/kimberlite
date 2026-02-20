---
title: "Operational Runbook"
section: "operating"
slug: "runbook"
order: 8
---

# Operational Runbook

**Target Audience:** On-call Engineers, SREs, Operations Teams
**Purpose:** Step-by-step procedures for common operational scenarios
**Severity Levels:** P0 (Critical), P1 (High), P2 (Medium), P3 (Low)

---

## Table of Contents

- [Emergency Contacts](#emergency-contacts)
- [Incident Response Procedures](#incident-response-procedures)
- [Common Failure Scenarios](#common-failure-scenarios)
- [Performance Troubleshooting](#performance-troubleshooting)
- [Upgrade Procedures](#upgrade-procedures)
- [Escalation Paths](#escalation-paths)
- [Post-Incident Review](#post-incident-review)

---

## Emergency Contacts

### On-Call Rotation

| Role | Primary | Secondary | Escalation |
|------|---------|-----------|------------|
| **Database SRE** | +1-555-0100 | +1-555-0101 | +1-555-0102 |
| **Platform Engineering** | +1-555-0200 | +1-555-0201 | +1-555-0202 |
| **Security Team** | +1-555-0300 | +1-555-0301 | +1-555-0302 |
| **Compliance Officer** | +1-555-0400 | N/A | +1-555-0401 |

### Communication Channels

- **Slack:** `#kimberlite-incidents` (immediate response)
- **PagerDuty:** `kimberlite-production` service
- **Status Page:** https://status.kimberlite.io
- **Incident Management:** https://incidents.kimberlite.io

---

## Incident Response Procedures

### P0: Critical - Service Down / Data Loss Risk

**Examples:** All replicas down, quorum lost, assertion failures, data corruption

**Response Time:** 15 minutes (24/7)

**Procedure:**

```bash
# 1. ACKNOWLEDGE ALERT (within 5 minutes)
# - PagerDuty acknowledgment
# - Post in #kimberlite-incidents: "P0 incident - investigating"

# 2. ASSESS SEVERITY
kimberlite-cli cluster status
# Check: How many replicas are healthy? Is quorum available?

# 3. IMMEDIATE TRIAGE
# If quorum available (2/3 or 3/5):
#   - Service degraded but operational
#   - Continue to step 4

# If quorum lost (< 2/3):
#   - SERVICE DOWN - All writes failing
#   - Escalate immediately to Platform Engineering
#   - Update status page: "Major Outage"

# 4. CHECK MONITORING DASHBOARDS
# - Grafana: "Kimberlite Cluster Overview"
# - Look for: replica status, view changes, replication lag, disk errors

# 5. COLLECT DIAGNOSTIC INFO
kimberlite-cli cluster logs --since 1h > /tmp/cluster-logs-$(date +%s).txt
kimberlite-cli replica status --all > /tmp/replica-status-$(date +%s).txt

# 6. FOLLOW SPECIFIC RUNBOOK
# - See "Common Failure Scenarios" section below
# - Document all actions taken in incident ticket

# 7. COMMUNICATION (every 15 minutes)
# - Update #kimberlite-incidents with progress
# - Update status page if customer-facing impact

# 8. RESOLVE OR ESCALATE
# - If resolved: Run post-incident review
# - If not resolved within 30 minutes: Escalate to engineering lead
```

### P1: High - Performance Degradation

**Examples:** High latency (>100ms p99), replication lag, view change storms

**Response Time:** 30 minutes (business hours)

**Procedure:**

```bash
# 1. ACKNOWLEDGE ALERT
# Post in #kimberlite-incidents: "P1 incident - investigating"

# 2. CHECK CURRENT METRICS
# Prometheus queries:
rate(kimberlite_consensus_commits_total[5m])  # Throughput
kimberlite_consensus_commit_latency_seconds{quantile="0.99"}  # p99 latency
kimberlite_replication_lag_ms  # Lag per replica

# 3. IDENTIFY ROOT CAUSE
# Common causes:
# - High disk latency (check IOPS)
# - Network issues (check packet loss)
# - Memory pressure (check available RAM)
# - CPU saturation (check load average)

# 4. APPLY MITIGATION
# See "Performance Troubleshooting" section

# 5. MONITOR FOR IMPROVEMENT
# Wait 10 minutes, recheck metrics
# If not improving: Escalate to Database SRE lead

# 6. DOCUMENT AND CLOSE
# Create incident ticket with:
# - Root cause
# - Mitigation applied
# - Follow-up actions (if any)
```

### P2: Medium - Non-Critical Issues

**Examples:** Single replica down, moderate lag, configuration drift

**Response Time:** 2 hours (business hours)

**Procedure:**

```bash
# 1. TRIAGE
# - Service operational? Yes (quorum maintained)
# - Customer impact? No (transparent failover)

# 2. INVESTIGATE
# - Check replica logs
# - Review recent changes (deployments, config)

# 3. FIX OR SCHEDULE
# - If quick fix (restart, config): Apply immediately
# - If requires deep investigation: Schedule maintenance window

# 4. DOCUMENT
# - Update runbook if new issue discovered
# - Create Jira ticket for follow-up
```

---

## Common Failure Scenarios

### Scenario 1: Replica Crash / Unresponsive

**Symptoms:**
- Alert: `kimberlite_cluster_replicas_total{status="healthy"} < 3`
- Replica not responding to heartbeats
- View change triggered

**Diagnosis:**

```bash
# 1. Check replica status
kimberlite-cli replica status --replica-id 2
# Output: "status": "unreachable"

# 2. Check host-level health
ssh replica-2.kimberlite.prod
top  # Check CPU/RAM
df -h  # Check disk space
dmesg | tail -50  # Check kernel errors

# 3. Check replica logs
journalctl -u kimberlite -n 1000 --no-pager
# Look for: panics, assertion failures, OOM killer
```

**Resolution:**

```bash
# Option A: Restart replica (if simple crash)
systemctl restart kimberlite
# OR (Kubernetes)
kubectl delete pod kimberlite-2 -n production

# Wait for replica to rejoin
kimberlite-cli replica status --replica-id 2 --watch
# Expected: "status": "recovering" -> "normal"

# Option B: If restart fails, check for corruption
kimberlite-cli storage verify --replica-id 2 --full-scan

# If corruption detected:
kimberlite-cli storage restore \
    --replica-id 2 \
    --backup-source "s3://backups/latest"

# Option C: If hardware failure suspected
# 1. Remove from cluster
kimberlite-cli cluster reconfig remove-replica --replica-id 2
# 2. Replace hardware
# 3. Add back as new replica
kimberlite-cli cluster reconfig add-replica --replica-id 2
```

**Prevention:**
- Monitor: `kimberlite_assertion_failures_total` (must be 0)
- Enable: Automatic restarts (Kubernetes liveness probes)
- Schedule: Weekly storage scrubbing

### Scenario 2: Quorum Lost (All Replicas Down)

**Symptoms:**
- Alert: `kimberlite_cluster_replicas_total{status="healthy"} < 2`
- All client writes failing
- Status page: "Major Outage"

**CRITICAL:** This is a P0 incident. Escalate immediately.

**Diagnosis:**

```bash
# 1. Check if replicas are actually down or network partitioned
for id in 0 1 2; do
    ping -c 3 replica-$id.kimberlite.prod
done

# 2. Check load balancer / network infrastructure
# Contact network operations team

# 3. If replicas are truly down, check why
# - Power outage in datacenter?
# - Kubernetes cluster failure?
# - Cascading failure (e.g., disk full on all nodes)?
```

**Resolution:**

```bash
# Priority: Get ANY replica online to restore read access

# Option A: If network partition
# - Fix network connectivity
# - Replicas will auto-recover once they can communicate

# Option B: If datacenter failure (multi-region with standby)
# FAILOVER TO DR REGION
kimberlite-cli cluster failover \
    --target-region us-west-2 \
    --promote-standby 100 \
    --approve-data-loss-risk

# Update DNS
aws route53 change-resource-record-sets \
    --hosted-zone-id Z1234567890ABC \
    --change-batch file://failover-to-dr.json

# Option C: If all data lost (LAST RESORT)
# 1. Restore from most recent backup
kimberlite-cli cluster restore-from-backup \
    --backup-source "s3://backups/snapshot-20260206-120000.tar.gz" \
    --new-cluster-id $(uuidgen)

# 2. Data loss will occur (RPO = backup age)
# 3. Notify compliance officer (breach reporting may be required)
```

**Prevention:**
- Deploy: Multi-region with standby replicas
- Test: Monthly DR drills
- Monitor: `kimberlite_cluster_replicas_total{status="healthy"}` >= 2

### Scenario 3: View Change Storm (Rapid Leader Elections)

**Symptoms:**
- Alert: `rate(kimberlite_consensus_view_changes_total[5m]) > 10`
- Frequent leader changes (every few seconds)
- High latency, low throughput

**Diagnosis:**

```bash
# 1. Check view change history
kimberlite-cli cluster view-changes --since 10m
# Output: View changes with timestamps and reasons

# 2. Check clock synchronization
for id in 0 1 2; do
    kimberlite-cli replica clock-offset --replica-id $id
done
# If offset > 500ms: CLOCK DESYNC DETECTED

# 3. Check network latency between replicas
for id in 0 1 2; do
    ping -c 10 replica-$id.kimberlite.prod | grep avg
done
# If RTT > 100ms: NETWORK ISSUE
```

**Resolution:**

```bash
# If clock desync:
# 1. Force clock synchronization
for id in 0 1 2; do
    ssh replica-$id.kimberlite.prod "systemctl restart chronyd"
done

# 2. Wait for clocks to stabilize (30-60 seconds)
# 3. Monitor view changes should stop

# If network latency:
# 1. Check network infrastructure (switch ports, congestion)
# 2. Contact network operations
# 3. Increase election timeout temporarily (emergency only)
kimberlite-cli cluster config set election-timeout-ms 2000

# If persistent issue:
# 1. Isolate problematic replica
kimberlite-cli replica isolate --replica-id 1
# 2. Run diagnostics
# 3. Remove from cluster if necessary
```

**Prevention:**
- Monitor: Clock offset `< 100ms` (alert at 200ms)
- NTP: Use multiple reliable NTP servers
- Network: Ensure <1ms RTT between replicas

### Scenario 4: High Replication Lag

**Symptoms:**
- Alert: `kimberlite_replication_lag_ms > 1000`
- Backup replicas falling behind primary
- Standby replicas marked ineligible for promotion

**Diagnosis:**

```bash
# 1. Check current lag
kimberlite-cli replication lag --all
# Output: Lag in operations and milliseconds per replica

# 2. Check disk performance
iostat -x 5 3
# Look for: %util > 90%, await > 20ms

# 3. Check network throughput
iftop
# Look for: saturation, packet drops

# 4. Check replica CPU/memory
top
# Look for: CPU > 80%, memory pressure
```

**Resolution:**

```bash
# If disk I/O bottleneck:
# 1. Check for runaway queries
kimberlite-cli query slow-queries --top 10
# Kill expensive queries if found

# 2. Trigger log compaction (free up IOPS)
kimberlite-cli storage compact --replica-id 1

# 3. Increase disk IOPS (cloud)
aws ec2 modify-volume --volume-id vol-123 --iops 10000

# If network bottleneck:
# 1. Check for bandwidth saturation
# 2. Throttle read queries (if standby serving reads)
kimberlite-cli standby throttle --replica-id 100 --max-qps 1000

# If CPU bottleneck:
# 1. Scale up instance size
# 2. Add more standby replicas to distribute read load

# Emergency: Remove lagging replica
kimberlite-cli replica isolate --replica-id 1 --reason "lag-exceeded"
# Fix issue, then rejoin
```

**Prevention:**
- Monitor: Lag every 15 seconds
- Alert: Lag > 500ms (P2), Lag > 1000ms (P1)
- Capacity: Plan for 50% headroom on disk IOPS

### Scenario 5: Assertion Failure (Formal Verification Violation)

**Symptoms:**
- Alert: `kimberlite_assertion_failures_total > 0` (CRITICAL)
- Replica panicked with assertion failure
- Log contains: `assertion failed: ...`

**CRITICAL:** This indicates a bug in Kimberlite or hardware corruption. Treat as P0.

**Diagnosis:**

```bash
# 1. IMMEDIATELY ISOLATE AFFECTED REPLICA
kimberlite-cli replica isolate --replica-id 2 --reason "assertion-failure"

# 2. Capture diagnostics BEFORE any recovery
kimberlite-cli debug dump-state --replica-id 2 > /tmp/assertion-failure-$(date +%s).json
kimberlite-cli storage export --replica-id 2 --output /tmp/storage-export.tar.gz

# 3. Check log for assertion details
journalctl -u kimberlite --since "10 minutes ago" | grep "assertion failed"
# Example: "assertion failed: commit_number <= op_number"

# 4. Check for hardware issues
smartctl -a /dev/nvme0n1  # Disk errors?
memtester 1G 1  # Memory corruption?
```

**Resolution:**

```bash
# DO NOT ATTEMPT AUTOMATIC RECOVERY

# 1. Create incident ticket with:
#    - Assertion failure message
#    - Replica state dump
#    - Storage export

# 2. Contact Kimberlite engineering (escalate to P0)
#    - This is a formal verification violation
#    - May indicate software bug or hardware corruption

# 3. Remove replica from cluster
kimberlite-cli cluster reconfig remove-replica --replica-id 2

# 4. Restore from verified backup
kimberlite-cli storage restore \
    --replica-id 2 \
    --backup-source "s3://backups/snapshot-verified-20260205.tar.gz" \
    --verify-hash-chain

# 5. Run full verification before rejoining
kimberlite-cli storage verify --replica-id 2 --full-scan
kimberlite-cli replica test-all-kani-proofs --replica-id 2

# 6. Rejoin cluster only after engineering approval
```

**Prevention:**
- Monitor: `kimberlite_assertion_failures_total` == 0 (always)
- Hardware: Use ECC RAM, enterprise-grade SSDs
- Testing: Run VOPR scenarios in staging before production deploy

### Scenario 6: Disk Full

**Symptoms:**
- Alert: `kimberlite_storage_disk_usage_percent > 90`
- Log writes failing
- Replica unable to accept new operations

**Diagnosis:**

```bash
# 1. Check disk usage
df -h /var/lib/kimberlite
# Output: 95% used

# 2. Identify largest files
du -sh /var/lib/kimberlite/* | sort -h
# Likely culprit: log files
```

**Resolution:**

```bash
# Option A: Trigger log compaction (fast, minutes)
kimberlite-cli storage compact --replica-id 2 --aggressive
# Removes old log entries, frees space

# Option B: Increase disk size (cloud, minutes)
aws ec2 modify-volume --volume-id vol-123 --size 2000
# Then extend filesystem
resize2fs /dev/nvme0n1

# Option C: Emergency cleanup (use with caution)
# Delete old backup snapshots (if stored locally)
find /var/lib/kimberlite/backup -name "snapshot-*.tar.gz" -mtime +7 -delete

# Option D: Offload to S3 (for cold data)
kimberlite-cli storage archive \
    --replica-id 2 \
    --before-date 2025-01-01 \
    --destination s3://kimberlite-archive-prod
```

**Prevention:**
- Monitor: Disk usage every 5 minutes
- Alert: 80% (P3), 90% (P1), 95% (P0)
- Automation: Auto-compaction at 85%
- Capacity: Plan for 30% free space

---

## Performance Troubleshooting

### High Commit Latency (p99 > 100ms)

**Target:** p99 < 50ms, p99.9 < 100ms

**Diagnosis Tree:**

```
High p99 latency?
├─ Check disk fsync latency
│  └─ iostat -x 5 3 | grep nvme
│     └─ await > 10ms? → Disk I/O issue
│        ├─ Check for competing I/O (other processes)
│        ├─ Check disk queue depth (nr_requests)
│        └─ Consider faster storage (Optane)
│
├─ Check network RTT
│  └─ ping replica-1 | grep time
│     └─ RTT > 5ms? → Network latency
│        ├─ Check for packet loss (mtr)
│        ├─ Check switch configuration
│        └─ Consider placement groups (AWS)
│
├─ Check quorum size
│  └─ kimberlite-cli cluster status
│     └─ 5-replica cluster? → Inherently higher latency
│        └─ Consider reducing to 3-replica if acceptable
│
└─ Check CPU saturation
   └─ top | grep kimberlite
      └─ CPU > 80%? → CPU bottleneck
         ├─ Check for expensive queries
         ├─ Scale up instance size
         └─ Optimize query patterns
```

**Quick Fixes:**

```bash
# 1. Reduce batch size (lower latency, lower throughput)
kimberlite-cli cluster config set max-batch-size 100

# 2. Increase heartbeat frequency (detect issues faster)
kimberlite-cli cluster config set heartbeat-interval-ms 50

# 3. Check for slow queries
kimberlite-cli query slow-queries --threshold-ms 100
# Kill if found: kimberlite-cli query kill <query-id>
```

### Low Throughput (< 10K ops/sec)

**Target:** >50K ops/sec (3-replica, NVMe SSDs)

**Diagnosis:**

```bash
# 1. Check current throughput
rate(kimberlite_consensus_commits_total[1m])

# 2. Check batch size
kimberlite-cli cluster config get max-batch-size
# If < 1000: Increase for higher throughput
kimberlite-cli cluster config set max-batch-size 1000

# 3. Check for bottlenecks
# - CPU: top (should be < 60%)
# - Disk: iostat -x 5 (await < 5ms)
# - Network: iftop (< 5 Gbps on 10G link)

# 4. Check for serialization bottlenecks
kimberlite-cli debug profile --replica-id 0 --duration 60s
# Look for: hot functions in flamegraph
```

**Quick Fixes:**

```bash
# 1. Enable batching (groups small writes)
kimberlite-cli cluster config set batching-enabled true

# 2. Tune compaction (run less frequently during peak)
kimberlite-cli storage compact-schedule --off-peak-only

# 3. Add standby replicas (offload read queries)
kimberlite-cli cluster reconfig add-standby --replica-id 100
```

---

## Upgrade Procedures

### Rolling Upgrade (Zero Downtime)

**Prerequisites:**
- Backup completed in last 24 hours
- Staging environment tested
- Change window approved

**Procedure:**

```bash
# 1. PRE-UPGRADE CHECKS
kimberlite-cli cluster status
# Verify: All replicas healthy, no lag

kimberlite-cli cluster version
# Current version: v0.3.0

# 2. UPGRADE REPLICAS SEQUENTIALLY (one at a time)
for replica_id in 2 1 0; do  # Backups first, primary last
    echo "Upgrading replica $replica_id..."

    # Kubernetes
    kubectl set image statefulset/kimberlite \
        kimberlite=kimberlite/server:v0.4.0 \
        --namespace production
    kubectl rollout status statefulset/kimberlite

    # OR Docker
    # docker pull kimberlite/server:v0.4.0
    # systemctl restart kimberlite

    # Wait for replica to rejoin
    kimberlite-cli replica status --replica-id $replica_id --wait-healthy

    # Verify cluster version progressed
    kimberlite-cli cluster version
    # Expected: cluster_version increases as each replica upgrades

    # Wait 5 minutes between replicas (observe stability)
    sleep 300
done

# 3. VERIFY FEATURE ACTIVATION
kimberlite-cli cluster features
# Expected: New features enabled when all replicas reach v0.4.0

# 4. POST-UPGRADE VALIDATION
kimberlite-cli cluster status
# Verify: All replicas on v0.4.0, cluster healthy

# Run smoke tests
kimberlite-cli test smoke-tests --critical-only

# 5. MONITOR FOR 1 HOUR
# Watch for: increased latency, errors, view changes
```

**Rollback Procedure:**

```bash
# If issues detected, rollback immediately

for replica_id in 0 1 2; do
    kubectl set image statefulset/kimberlite \
        kimberlite=kimberlite/server:v0.3.0 \
        --namespace production

    kimberlite-cli replica status --replica-id $replica_id --wait-healthy
    sleep 300
done

# Document reason for rollback in incident ticket
```

---

## Escalation Paths

### When to Escalate

| Situation | Escalate To | Timeframe |
|-----------|-------------|-----------|
| Quorum lost | Platform Engineering Lead | Immediate |
| Assertion failure | Kimberlite Engineering | Immediate |
| Multi-region outage | VP Engineering | Immediate |
| Data corruption detected | CTO + Compliance Officer | 15 minutes |
| Performance degradation (P1) | Database SRE Lead | 30 minutes |
| Unresolved issue after 1 hour | On-call manager | 1 hour |

### Escalation Template

```
Subject: [P0] Kimberlite Incident - Quorum Lost

Severity: P0 (Critical)
Status: Active
Started: 2026-02-06 14:30 UTC
Impact: All writes failing, reads degraded

Current State:
- Replicas 0, 1 DOWN
- Replica 2 HEALTHY (no quorum)
- Root cause: Unknown (investigating)

Actions Taken:
1. Acknowledged PagerDuty at 14:31 UTC
2. Checked replica logs (no obvious errors)
3. Verified network connectivity (all reachable)
4. Attempting replica restarts

Requesting:
- Platform Engineering assistance
- Escalated to VP Engineering (CC'd)

Incident Link: https://incidents.kimberlite.io/INC-12345
War Room: #incident-12345
```

---

## Post-Incident Review

### RCA Template

**Incident:** [INC-12345] High Replication Lag

**Date:** 2026-02-06
**Severity:** P1
**Duration:** 45 minutes (14:30 - 15:15 UTC)
**Impact:** Standby replica marked ineligible, DR failover unavailable

**Timeline:**
- 14:30: Alert triggered (lag > 1000ms)
- 14:32: On-call acknowledged, began investigation
- 14:35: Identified root cause (disk I/O saturation)
- 14:40: Applied mitigation (log compaction)
- 15:00: Lag returned to < 100ms
- 15:15: Incident resolved

**Root Cause:**
Log compaction not running automatically due to misconfiguration. Log file grew to 800GB, exhausting disk IOPS.

**Resolution:**
Manually triggered log compaction, freed 200GB, IOPS returned to normal.

**Action Items:**
1. [ ] Fix compaction scheduler config (Priority: P0, Owner: @sre-alice)
2. [ ] Add alert for compaction failures (Priority: P1, Owner: @sre-bob)
3. [ ] Increase disk IOPS provisioned by 50% (Priority: P2, Owner: @sre-charlie)
4. [ ] Document compaction monitoring in runbook (Priority: P3, Owner: @sre-alice)

**Lessons Learned:**
- Compaction is critical for IOPS management
- Need better visibility into storage subsystem health
- Automated remediation for common issues (auto-compact at 85% disk)

**Compliance Impact:**
None. No data loss, no audit log gaps, no compliance violations.

---

## References

- **Production Deployment:** [Production Deployment Guide](production-deployment.md)
- **Monitoring:** [Monitoring and Alerting](monitoring.md)
- **Configuration:** [Configuration Guide](configuration.md)
- **Security:** [Security Best Practices](security.md)
- **Compliance:** [Compliance Certification Package](../compliance/certification-package.md)

---

**Last Updated:** 2026-02-06
**Version:** 0.4.0
**On-Call Rotation:** https://pagerduty.com/kimberlite-production
