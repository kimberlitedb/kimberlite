# Kimberlite Long-Running Testing Infrastructure

Continuous 48-hour testing cycles on AWS covering VOPR simulation, fuzz testing, formal verification, and benchmarks.

## Architecture

Single `c7g.xlarge` spot instance running all four workloads on a repeating cycle:

```
Hour 0-1:   git pull, cargo build, install deps
Hour 1-2:   Formal Verification (TLA+, Coq, Alloy, Ivy, TLAPS via Docker)
Hour 2-3:   Benchmarks (5 Criterion suites, quiet system)
Hour 3-7:   Fuzz Testing (4h, 3 targets in parallel)
Hour 7-47:  VOPR Marathon (40h continuous, all scenarios)
Hour 47-48: Generate digest, upload to S3, send 1 email
```

```
┌─────────────────────────────────────────────────────────┐
│ EC2 Spot Instance (c7g.xlarge ARM, 4 vCPU, 8GB)        │
│ ┌─────────────────────────────────────────────────────┐ │
│ │ kimberlite-test-runner.sh (systemd service)         │ │
│ │   ├─> Formal Verification (Docker: Coq, TLAPS, Ivy)│ │
│ │   ├─> Benchmarks (Criterion, 5 suites)             │ │
│ │   ├─> Fuzz Testing (cargo-fuzz, 3 targets)         │ │
│ │   └─> VOPR Marathon (batch loop, all scenarios)     │ │
│ └────────┬───────────────────────────────────┬────────┘ │
│          │                                    │          │
│          ▼                                    ▼          │
│ ┌─────────────────┐                 ┌─────────────────┐ │
│ │ CloudWatch Logs │                 │ S3 Bucket       │ │
│ │ - Runner output │                 │ - digests/      │ │
│ │ - 7d retention  │                 │ - failures/     │ │
│ └────────┬────────┘                 │ - benchmarks/   │ │
│          │                          │ - fuzz-corpus/  │ │
│          │                          │ - checkpoints/  │ │
│          │                          └─────────────────┘ │
└──────────┼──────────────────────────────────────────────┘
           │
           ▼
  ┌─────────────────┐     ┌─────────────────┐
  │ CloudWatch      │     │ SNS Topic       │
  │ Alarms          │────>│ - Daily digest  │
  │ - No progress   │     │ - Critical only │
  │ - No digest     │     └─────────────────┘
  └─────────────────┘
```

## Notification Strategy

The old `failure_detected` alarm has been removed — it fired on every batch with any failure, causing thousands of emails.

| Alert Type | Frequency | Trigger |
|------------|-----------|---------|
| Daily digest | 1/cycle (every 48h) | End of each testing cycle |
| No progress | Operational | Instance crashed/stuck (15min) |
| No digest | Operational | No digest uploaded in 26 hours |
| Critical | Max 1/hour | >100 failures in single VOPR batch |

Failure deduplication tracks signatures (`invariant:scenario`) in S3 — only **new** bug types are highlighted in digests.

## Quick Start

### Prerequisites

1. AWS CLI configured: `aws configure`
2. Terraform installed: `brew install terraform` (macOS)
3. Email address for digest notifications

### Deploy

```bash
# 1. Configure
cd infra/vopr-aws
cp terraform.tfvars.example terraform.tfvars
vim terraform.tfvars  # Set alert_email

# 2. Deploy
just deploy-infra

# 3. Confirm SNS email subscription (check inbox)

# 4. Monitor
just infra-logs
```

## Developer Workflow

```bash
# Daily check
just infra-status              # Quick overview (instance state + latest digest)
just infra-digest              # Full digest JSON with all details

# Found a bug? Reproduce locally
just list-failures             # See available VOPR + fuzz artifacts
just fetch-failure vopr/seed-1234567-1700000000.json
just vopr-repro .artifacts/aws-failures/seed-1234567.kmb

# Management
just deploy-infra              # Deploy or update infrastructure
just infra-logs                # Live CloudWatch logs
just infra-ssh                 # SSM session to instance
just infra-stop                # Pause instance (save money)
just infra-start               # Resume instance
just infra-destroy             # Tear down everything
just infra-bench               # View latest benchmark results
```

## S3 Bucket Structure

```
s3://vopr-simulation-results-{account}/
  checkpoints/latest.json            # VOPR checkpoint (resume after spot interrupt)
  checkpoints/daily/YYYY-MM-DD.json  # Daily checkpoint snapshots (30d retention)
  failures/vopr/seed-SEED-TS.json    # VOPR failure details (90d → Glacier)
  failures/fuzz/TARGET-TS.tar.gz     # Fuzz crash artifacts (90d → Glacier)
  digests/latest.json                # Most recent daily digest (30d retention)
  digests/YYYY-MM-DD.json            # Historical digests
  signatures/known-failures.json     # Deduplication registry
  benchmarks/YYYY-MM-DD.json         # Benchmark results (90d retention)
  benchmarks/baseline.json           # Stable baseline for regression detection
  formal-verification/YYYY-MM-DD.json # FV results (90d retention)
  fuzz-corpus/TARGET/                # Persistent fuzz corpus (survives spot interrupts)
```

## Configuration

### Variables (`terraform.tfvars`)

| Variable | Default | Description |
|----------|---------|-------------|
| `alert_email` | (required) | Email for digest notifications |
| `instance_type` | `c7g.xlarge` | EC2 instance type (ARM Graviton) |
| `volume_size` | `60` | Root EBS volume in GB |
| `run_duration_hours` | `48` | Cycle duration before digest + restart |
| `enable_fuzzing` | `true` | Include fuzz testing phase |
| `enable_formal_verification` | `true` | Include formal verification (needs Docker) |
| `enable_benchmarks` | `true` | Include benchmark phase |
| `spot_max_price` | `""` | Max spot price (empty = on-demand max) |
| `aws_region` | `us-east-1` | AWS region |
| `github_repo` | `https://github.com/kimberlitedb/kimberlite.git` | Repository URL |
| `github_branch` | `main` | Branch to test |

## Cost Breakdown

| Item | Monthly Cost |
|------|--------------|
| c7g.xlarge spot (730h @ $0.032/h) | $23.36 |
| EBS 60GB GP3 | $4.80 |
| CloudWatch Logs (5GB, 7d) | $2.52 |
| CloudWatch Metrics (4 custom) | $2.40 |
| S3 (20GB + requests) | $0.60 |
| SNS (30 emails) | $0.00 |
| **Total** | **~$34/month** |

## Local Smoke Test

Before deploying, verify all test harnesses work locally:

```bash
just test-infra-smoke
```

This checks:
1. VOPR produces valid JSON output
2. All 3 fuzz targets run successfully
3. Docker is available (for formal verification)
4. Benchmarks execute correctly

## Troubleshooting

### No logs appearing
- Check instance state: `just infra-status`
- Verify IAM permissions include CloudWatch Logs access
- SSH in to check: `just infra-ssh`, then `systemctl status kimberlite-testing`

### Instance stopped unexpectedly
- Spot interruption — instance has `persistent` spot type and will auto-restart
- VOPR resumes from S3 checkpoint, fuzz corpus persists across restarts

### No digest email received
- Confirm SNS subscription (check spam folder)
- The `no_digest` alarm fires if no digest in 26 hours
- Check logs: `just infra-logs`

### Build failures on instance
- Check Rust version: should be 1.88.0 (workspace MSRV)
- Check disk space: 60GB should be sufficient for Docker images + build artifacts
