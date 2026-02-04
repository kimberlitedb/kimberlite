# Kimberlite VSR Production Deployment Guide

**Version:** 0.4.0
**Date:** 2026-02-05
**Status:** Production Ready

## Executive Summary

This guide provides comprehensive instructions for deploying Kimberlite VSR in production environments. Topics covered include hardware requirements, cluster topology, configuration best practices, security hardening, and performance tuning.

**Target Audience:** System administrators, DevOps engineers, site reliability engineers

**Prerequisites:**
- Linux kernel 5.10+ (Ubuntu 22.04 LTS or RHEL 9 recommended)
- Rust 1.88+ toolchain
- Network with < 5ms RTT between replicas (same data center recommended)
- NTP or chrony for clock synchronization

---

## Table of Contents

1. [Hardware Requirements](#hardware-requirements)
2. [Cluster Topology](#cluster-topology)
3. [Installation](#installation)
4. [Configuration](#configuration)
5. [Security Hardening](#security-hardening)
6. [Performance Tuning](#performance-tuning)
7. [Monitoring](#monitoring)
8. [Backup & Recovery](#backup--recovery)
9. [Capacity Planning](#capacity-planning)
10. [Troubleshooting](#troubleshooting)

---

## Hardware Requirements

### Minimum Production Configuration (3-Node Cluster)

| Component | Specification | Rationale |
|-----------|--------------|-----------|
| **CPU** | 4 cores (8 vCPUs) | Consensus protocol + kernel processing + scrubbing |
| **Memory** | 16 GB RAM | Log buffer + state machine + OS cache |
| **Storage** | 500 GB NVMe SSD | Low-latency durable writes critical for consensus |
| **Network** | 10 Gbps | High throughput for state transfer |
| **Disk IOPS** | 50,000+ read, 30,000+ write | Fsync latency affects consensus latency |

### Recommended Production Configuration

| Component | Specification | Benefit |
|-----------|--------------|---------|
| **CPU** | 8 cores (16 vCPUs) | Headroom for spikes, multi-tenant isolation |
| **Memory** | 64 GB RAM | Larger log buffer, checkpoint caching |
| **Storage** | 2 TB NVMe SSD (PCIe 4.0) | Future growth, striped RAID for redundancy |
| **Network** | 25 Gbps (redundant) | Fast recovery, dual-path resilience |
| **Disk IOPS** | 100,000+ read, 50,000+ write | Sub-millisecond fsync |

### Storage Requirements

**Log Growth Estimation:**
- Average entry size: ~1 KB (command + metadata)
- Expected throughput: 10,000 ops/sec
- Daily growth: ~86 GB/day (10k ops/sec * 86,400 sec * 1 KB)
- Retention period: 7 days → ~600 GB

**Checkpoint Overhead:**
- Checkpoint interval: 10,000 operations
- Checkpoint size: ~100 MB (depends on state machine size)
- Checkpoint retention: 3 checkpoints → ~300 MB

**Total Storage (7-day retention):** ~650 GB (recommend 2 TB for headroom)

### Disk Configuration

**RAID Recommendations:**
- **RAID 10** (mirrored striping): Best performance + redundancy
- **RAID 1** (mirroring): Simple, good for smaller deployments
- **Avoid RAID 5/6**: Parity overhead impacts write latency

**Filesystem:**
- **ext4** with `noatime,data=ordered` (good default)
- **XFS** with `noatime` (better for large files)
- **Avoid**: NFS, Lustre (network latency kills consensus)

**Partition Layout:**
```
/dev/nvme0n1p1  →  /boot          (1 GB, ext4)
/dev/nvme0n1p2  →  /              (100 GB, ext4)
/dev/nvme0n1p3  →  /var/lib/kimberlite  (1.8 TB, ext4/xfs, noatime)
```

---

## Cluster Topology

### 3-Node Cluster (Recommended Minimum)

```
┌─────────────────┐       ┌─────────────────┐       ┌─────────────────┐
│   Replica 0     │       │   Replica 1     │       │   Replica 2     │
│  (us-east-1a)   │←─────→│  (us-east-1b)   │←─────→│  (us-east-1c)   │
│  Leader         │       │  Backup         │       │  Backup         │
└─────────────────┘       └─────────────────┘       └─────────────────┘
       ↑                          ↑                          ↑
       │                          │                          │
       └──────────────────────────┴──────────────────────────┘
                          Clients
```

**Properties:**
- **Fault Tolerance:** 1 failure (f=1, need 2f+1=3 replicas)
- **Quorum:** 2 of 3 replicas
- **Network RTT:** < 2ms within AZ (< 5ms cross-AZ acceptable)

**Placement:**
- Spread across 3 availability zones (AZs) in same region
- Use anti-affinity rules to prevent co-location
- Private VPC network with security groups

### 5-Node Cluster (High Availability)

```
┌──────┐  ┌──────┐  ┌──────┐  ┌──────┐  ┌──────┐
│Rep 0 │──│Rep 1 │──│Rep 2 │──│Rep 3 │──│Rep 4 │
│Leader│  │Backup│  │Backup│  │Backup│  │Backup│
└──────┘  └──────┘  └──────┘  └──────┘  └──────┘
```

**Properties:**
- **Fault Tolerance:** 2 failures (f=2, need 2f+1=5 replicas)
- **Quorum:** 3 of 5 replicas
- **Use Case:** Critical systems requiring double failure tolerance

**Trade-offs:**
- **Pros:** Survives 2 simultaneous failures, allows rolling upgrades
- **Cons:** Higher write latency (need 3 acks), more network bandwidth

### Multi-Region Setup (Disaster Recovery)

```
Region 1 (us-east)                 Region 2 (us-west)
┌──────────────────────┐           ┌──────────────────────┐
│  Replica 0 (active)  │           │  Replica 3 (standby) │
│  Replica 1 (active)  │←─────────→│  Replica 4 (standby) │
│  Replica 2 (active)  │  (100ms)  │                      │
└──────────────────────┘           └──────────────────────┘
    Quorum: 2/3                       Read-only followers
```

**Properties:**
- 3 active replicas in primary region (low latency)
- 2 standby replicas in DR region (read-only)
- Standby promotion if primary region fails

**Failover Procedure:**
1. Detect primary region failure (monitoring alert)
2. Promote 2 standbys to active (reconfiguration command)
3. Add 3rd replica in DR region (or cloud burst to 3rd region)
4. Update DNS/load balancer to point to DR region

---

## Installation

### From Source

```bash
# 1. Install dependencies
sudo apt-get update
sudo apt-get install -y build-essential libssl-dev pkg-config

# 2. Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# 3. Clone repository
git clone https://github.com/your-org/kimberlite.git
cd kimberlite

# 4. Build release binary
cargo build --release --package kimberlite-vsr

# 5. Install binary
sudo cp target/release/kimberlite-vsr /usr/local/bin/
sudo chmod +x /usr/local/bin/kimberlite-vsr

# 6. Create data directory
sudo mkdir -p /var/lib/kimberlite
sudo chown kimberlite:kimberlite /var/lib/kimberlite
```

### Systemd Service

Create `/etc/systemd/system/kimberlite-vsr.service`:

```ini
[Unit]
Description=Kimberlite VSR Replica
After=network.target

[Service]
Type=simple
User=kimberlite
Group=kimberlite
WorkingDirectory=/var/lib/kimberlite
ExecStart=/usr/local/bin/kimberlite-vsr \
  --config /etc/kimberlite/config.toml \
  --data-dir /var/lib/kimberlite
Restart=always
RestartSec=5s
LimitNOFILE=65536
LimitNPROC=4096

# Security hardening
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/kimberlite
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
```

Enable and start:

```bash
sudo systemctl daemon-reload
sudo systemctl enable kimberlite-vsr
sudo systemctl start kimberlite-vsr
sudo systemctl status kimberlite-vsr
```

---

## Configuration

### Configuration File (`/etc/kimberlite/config.toml`)

```toml
[cluster]
# Replica ID (0, 1, 2 for 3-node cluster)
replica_id = 0

# Cluster members (replica_id => address)
replicas = [
    { id = 0, address = "10.0.1.10:7000" },
    { id = 1, address = "10.0.1.11:7000" },
    { id = 2, address = "10.0.1.12:7000" },
]

[storage]
# Data directory for log and superblock
data_dir = "/var/lib/kimberlite"

# Log file path (append-only, CRC32-protected)
log_path = "/var/lib/kimberlite/log.bin"

# Superblock path (4-copy atomic metadata)
superblock_path = "/var/lib/kimberlite/superblock.bin"

# Fsync after every write (required for durability)
fsync_enabled = true

[timeouts]
# Heartbeat interval (milliseconds)
heartbeat_interval_ms = 100

# Heartbeat timeout (3x interval recommended)
heartbeat_timeout_ms = 300

# View change timeout (5x interval recommended)
view_change_timeout_ms = 500

# Recovery timeout
recovery_timeout_ms = 5000

[clock]
# Clock offset tolerance (milliseconds)
# Replicas with offset > tolerance are rejected
clock_offset_tolerance_ms = 100

# Clock synchronization window (milliseconds)
clock_sync_window_ms = 5000

[checkpoint]
# Checkpoint interval (operations)
checkpoint_interval = 10000

# Maximum checkpoints to retain
checkpoint_retention = 3

[repair]
# Repair budget (requests per tick)
repair_budget = 10

# Repair timeout (milliseconds)
repair_timeout_ms = 500

[scrubbing]
# Scrub budget (entries per tick)
scrub_budget = 100

# Scrub interval (ticks between scrubs)
scrub_interval_ticks = 10

[metrics]
# Enable Prometheus metrics endpoint
prometheus_enabled = true
prometheus_address = "0.0.0.0:9090"

# Enable OpenTelemetry export
otel_enabled = false
otel_endpoint = "http://otel-collector:4317"

[logging]
# Log level (trace, debug, info, warn, error)
level = "info"

# Log format (json, pretty)
format = "json"

# Log output (stdout, file)
output = "file"
log_file = "/var/log/kimberlite/vsr.log"
```

### Environment-Specific Configurations

**Development:**
```toml
[timeouts]
heartbeat_interval_ms = 500  # Slower for debugging
heartbeat_timeout_ms = 1500

[logging]
level = "debug"
format = "pretty"
output = "stdout"

[metrics]
prometheus_enabled = false
```

**Staging:**
```toml
[timeouts]
heartbeat_interval_ms = 100

[logging]
level = "info"
format = "json"

[metrics]
prometheus_enabled = true
otel_enabled = true  # Send to staging Otel collector
```

**Production:**
```toml
[timeouts]
heartbeat_interval_ms = 50  # Aggressive for low latency

[logging]
level = "warn"  # Only warnings and errors
format = "json"

[metrics]
prometheus_enabled = true
otel_enabled = true
```

---

## Security Hardening

### 1. Network Security

**Firewall Rules (iptables):**

```bash
# Allow VSR cluster communication (port 7000)
sudo iptables -A INPUT -p tcp --dport 7000 -s 10.0.1.0/24 -j ACCEPT

# Allow Prometheus metrics (port 9090) from monitoring server
sudo iptables -A INPUT -p tcp --dport 9090 -s 10.0.2.100 -j ACCEPT

# Drop all other traffic
sudo iptables -A INPUT -p tcp --dport 7000 -j DROP
sudo iptables -A INPUT -p tcp --dport 9090 -j DROP
```

**AWS Security Group:**

```
Inbound Rules:
- Port 7000 (TCP): Source = VPC CIDR (10.0.0.0/16)
- Port 9090 (TCP): Source = Monitoring SG
- Port 22 (SSH): Source = Bastion SG

Outbound Rules:
- All traffic: Destination = 0.0.0.0/0 (for NTP, package updates)
```

### 2. TLS/mTLS (Optional, for compliance)

**Generate Certificates:**

```bash
# CA certificate
openssl req -x509 -newkey rsa:4096 -keyout ca-key.pem -out ca-cert.pem -days 3650 -nodes

# Replica certificate
openssl req -newkey rsa:4096 -keyout replica0-key.pem -out replica0-csr.pem -nodes
openssl x509 -req -in replica0-csr.pem -CA ca-cert.pem -CAkey ca-key.pem -out replica0-cert.pem -days 365
```

**Config Update:**

```toml
[network]
tls_enabled = true
tls_cert_path = "/etc/kimberlite/certs/replica0-cert.pem"
tls_key_path = "/etc/kimberlite/certs/replica0-key.pem"
tls_ca_cert_path = "/etc/kimberlite/certs/ca-cert.pem"
```

### 3. User Permissions

```bash
# Create dedicated user
sudo useradd -r -s /bin/false -d /var/lib/kimberlite kimberlite

# Set ownership
sudo chown -R kimberlite:kimberlite /var/lib/kimberlite
sudo chown -R kimberlite:kimberlite /var/log/kimberlite
sudo chmod 700 /var/lib/kimberlite
sudo chmod 600 /var/lib/kimberlite/*.bin

# Restrict config file
sudo chown root:kimberlite /etc/kimberlite/config.toml
sudo chmod 640 /etc/kimberlite/config.toml
```

### 4. Audit Logging

**Enable auditd:**

```bash
sudo apt-get install auditd
sudo auditctl -w /var/lib/kimberlite -p rwa -k kimberlite_data
sudo auditctl -w /etc/kimberlite/config.toml -p wa -k kimberlite_config
```

**Log Review:**

```bash
sudo ausearch -k kimberlite_data
sudo ausearch -k kimberlite_config
```

---

## Performance Tuning

### 1. Kernel Parameters

Edit `/etc/sysctl.conf`:

```bash
# Network buffers (for high throughput)
net.core.rmem_max = 134217728
net.core.wmem_max = 134217728
net.ipv4.tcp_rmem = 4096 87380 67108864
net.ipv4.tcp_wmem = 4096 65536 67108864

# TCP tuning
net.ipv4.tcp_congestion_control = bbr
net.core.default_qdisc = fq

# File descriptors
fs.file-max = 2097152

# Disk I/O scheduler (for NVMe)
# (Set via udev rules, see below)
```

Apply:

```bash
sudo sysctl -p
```

### 2. Disk I/O Scheduler

For NVMe SSDs, use `none` (direct dispatch):

```bash
# Check current scheduler
cat /sys/block/nvme0n1/queue/scheduler

# Set to none
echo none | sudo tee /sys/block/nvme0n1/queue/scheduler
```

Persist via udev (`/etc/udev/rules.d/60-nvme-scheduler.rules`):

```
ACTION=="add|change", KERNEL=="nvme[0-9]n[0-9]", ATTR{queue/scheduler}="none"
```

### 3. CPU Affinity

Pin VSR process to specific cores (reduce context switching):

```bash
# In systemd service file
[Service]
CPUAffinity=0-7  # Cores 0-7
```

### 4. Huge Pages (Optional)

For large memory workloads:

```bash
# Allocate 1024 x 2MB huge pages (2 GB total)
echo 1024 | sudo tee /sys/kernel/mm/hugepages/hugepages-2048kB/nr_hugepages

# Mount hugetlbfs
sudo mkdir /mnt/huge
sudo mount -t hugetlbfs nodev /mnt/huge
```

Update config:

```toml
[memory]
use_huge_pages = true
huge_page_size_kb = 2048
```

---

## Monitoring

See [MONITORING.md](./MONITORING.md) for comprehensive monitoring guide.

**Quick Start:**

1. **Prometheus Scraping:**

```yaml
# prometheus.yml
scrape_configs:
  - job_name: 'kimberlite-vsr'
    static_configs:
      - targets:
        - '10.0.1.10:9090'  # Replica 0
        - '10.0.1.11:9090'  # Replica 1
        - '10.0.1.12:9090'  # Replica 2
```

2. **Key Metrics:**

```promql
# Operations per second
rate(vsr_operations_total[1m])

# P99 latency
histogram_quantile(0.99, rate(vsr_client_latency_ms_bucket[1m]))

# Quorum health
vsr_prepare_ok_votes >= vsr_quorum_size
```

3. **Grafana Dashboards:**

Import dashboard templates from `dashboards/grafana/vsr-overview.json`.

---

## Backup & Recovery

### Backup Strategy

**1. Continuous Replication (Built-in):**
- VSR automatically replicates to 2f+1 replicas
- No additional backup needed for fault tolerance

**2. Point-in-Time Snapshots:**

```bash
# Stop replica (optional, for consistency)
sudo systemctl stop kimberlite-vsr

# Snapshot data directory
sudo tar czf /backup/kimberlite-$(date +%Y%m%d-%H%M%S).tar.gz \
  /var/lib/kimberlite

# Restart replica
sudo systemctl start kimberlite-vsr
```

**3. Checkpoint Export (for compliance):**

```bash
# Export checkpoint with Ed25519 signature
kimberlite-vsr export-checkpoint \
  --checkpoint-id 10000 \
  --output /backup/checkpoint-10000.kmb \
  --verify-signature
```

### Recovery Procedures

**Scenario 1: Single Replica Failure**

```bash
# 1. Provision new host with same replica ID
# 2. Install kimberlite-vsr
# 3. Start with empty data directory
# 4. Replica auto-recovers via recovery protocol

sudo systemctl start kimberlite-vsr
# Monitor logs: replica enters Recovering status, then Normal
```

**Scenario 2: Quorum Loss (2+ replicas down in 3-node cluster)**

```bash
# 1. Restore from backup on one replica
cd /var/lib/kimberlite
sudo tar xzf /backup/kimberlite-20260205-120000.tar.gz

# 2. Start single replica
sudo systemctl start kimberlite-vsr

# 3. Wait for replica to become leader

# 4. Provision and start other replicas (they will sync)
```

**Scenario 3: Complete Cluster Loss**

```bash
# 1. Restore from most recent backup across all replicas
# 2. Verify superblock integrity
kimberlite-vsr verify-superblock --data-dir /var/lib/kimberlite

# 3. Start all replicas simultaneously
# 4. Verify quorum formed via metrics
```

---

## Capacity Planning

### Throughput Targets

| Workload | Expected Throughput | Notes |
|----------|---------------------|-------|
| Small writes (< 1 KB) | 10,000 ops/sec | Limited by network RTT |
| Large writes (10 KB) | 5,000 ops/sec | Limited by network bandwidth |
| Mixed workload | 7,500 ops/sec | Typical production |

### Scaling Guidelines

**Vertical Scaling (Single Cluster):**
- **CPU:** Add cores for parallel client request processing
- **Memory:** Increase for larger log buffer (reduce state transfer frequency)
- **Disk:** Upgrade to faster NVMe for lower fsync latency

**Horizontal Scaling (Multi-Cluster):**
- **Sharding:** Partition tenants across multiple clusters
- **Directory:** Use kimberlite-directory for routing
- **Rebalancing:** Migrate tenants during low-traffic windows

### Growth Projections

**Year 1:**
- **Operations:** 10k ops/sec * 86,400 sec/day * 365 days = 315B operations/year
- **Storage:** 315B ops * 1 KB/op = 315 TB/year (across cluster)
- **Per Replica:** 315 TB / 3 replicas = 105 TB/replica/year

**Recommendation:** Provision 2 TB/replica initially, expand yearly.

---

## Troubleshooting

### Common Issues

**1. High Latency (P99 > 100ms)**

**Symptoms:**
- `vsr_client_latency_ms_bucket{le="100"}` low
- Application timeouts

**Diagnosis:**
```bash
# Check disk latency
iostat -x 1 10

# Check network latency
ping <other-replica-ip>

# Check CPU utilization
top -H -p $(pidof kimberlite-vsr)
```

**Solutions:**
- Upgrade to faster NVMe SSD (target < 1ms fsync)
- Reduce network latency (move replicas closer)
- Tune heartbeat interval (`heartbeat_interval_ms = 50`)

**2. Quorum Lost**

**Symptoms:**
- Writes failing
- `vsr_prepare_ok_votes < vsr_quorum_size`

**Diagnosis:**
```bash
# Check replica status
sudo systemctl status kimberlite-vsr

# Check logs
sudo journalctl -u kimberlite-vsr -n 100

# Check network connectivity
telnet <other-replica-ip> 7000
```

**Solutions:**
- Restart failed replicas
- Check firewall rules
- Promote standby replica if available

**3. Memory Leak**

**Symptoms:**
- `vsr_memory_used_bytes` growing unbounded
- OOM killer terminates process

**Diagnosis:**
```bash
# Check memory usage
ps aux | grep kimberlite-vsr

# Generate heap profile
sudo kill -USR1 $(pidof kimberlite-vsr)  # If profiling enabled
```

**Solutions:**
- Update to latest version (bug fix)
- Reduce checkpoint interval (free memory sooner)
- Increase available memory

---

## Appendix: Quick Reference

### systemctl Commands

```bash
sudo systemctl start kimberlite-vsr
sudo systemctl stop kimberlite-vsr
sudo systemctl restart kimberlite-vsr
sudo systemctl status kimberlite-vsr
sudo systemctl enable kimberlite-vsr
sudo systemctl disable kimberlite-vsr
```

### Log Locations

- **Systemd Logs:** `sudo journalctl -u kimberlite-vsr -f`
- **Application Logs:** `/var/log/kimberlite/vsr.log`
- **Audit Logs:** `/var/log/audit/audit.log`

### Port Reference

| Port | Protocol | Purpose |
|------|----------|---------|
| 7000 | TCP | VSR cluster communication |
| 9090 | TCP | Prometheus metrics |
| 22 | TCP | SSH (management) |

### Useful Commands

```bash
# Check cluster health
curl http://localhost:9090/metrics | grep vsr_replica_status

# Force view change (emergency)
sudo kill -USR2 $(pidof kimberlite-vsr)

# Dump superblock
kimberlite-vsr dump-superblock --data-dir /var/lib/kimberlite

# Verify log integrity
kimberlite-vsr verify-log --data-dir /var/lib/kimberlite
```

---

## Support & Resources

- **Documentation:** https://docs.kimberlite.io
- **GitHub Issues:** https://github.com/your-org/kimberlite/issues
- **Slack Community:** https://kimberlite.slack.com
- **Commercial Support:** support@kimberlite.io

---

## Changelog

- **2026-02-05:** Initial production deployment guide (v0.4.0)
