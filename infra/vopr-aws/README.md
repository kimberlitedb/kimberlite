# VOPR AWS Deployment - Implementation Summary

## Overview

This directory contains Terraform infrastructure for deploying VOPR (deterministic simulation testing) to AWS with 24/7 monitoring and alerting.

**Target Cost:** ~$10/month
**Status:** Ready for deployment

## What Was Implemented

### 1. VOPR Binary Enhancements

**Files Modified:**
- `crates/kmb-sim/src/bin/vopr.rs` - Added JSON output and checkpoint support
- `crates/kmb-sim/Cargo.toml` - Added serde, serde_json, chrono dependencies

**New Features:**
- `--json` flag for structured output (newline-delimited JSON)
- `--checkpoint-file <PATH>` for resume support after interruptions
- Automatic progress tracking with seed history
- Failed seed archival for reproduction

**Verification:**
```bash
# Test JSON output
cargo run --release -p kmb-sim --bin vopr -- --json -n 10

# Test checkpoint support
cargo run --release -p kmb-sim --bin vopr -- --checkpoint-file /tmp/checkpoint.json -n 100
```

### 2. Terraform Infrastructure

**Files Created:**
- `main.tf` - EC2 spot instance, IAM roles, CloudWatch, SNS, S3
- `variables.tf` - Configuration variables
- `terraform.tfvars.example` - Example configuration

**Resources Deployed:**
- **EC2**: c7g.medium ARM Graviton spot instance (~$6/month)
- **IAM**: Instance role with least-privilege permissions
- **CloudWatch**: Log group (7-day retention) + 2 metric alarms
- **SNS**: Email topic for failure alerts
- **S3**: Bucket for checkpoints and failure archives (lifecycle policies)

**Key Features:**
- Spot instances with persistent stop behavior (not terminate)
- Automatic checkpoint sync to S3 every batch
- Metric-based alarms (failures detected, no progress)
- Daily checkpoint snapshots (30-day retention)
- Failure archive with Glacier transition (90 days)

### 3. Deployment Scripts

**Files Created:**
- `user_data.sh` - EC2 bootstrap script
  - Installs Rust 1.85 (ARM build)
  - Clones and builds kimberlite
  - Configures CloudWatch agent
  - Creates systemd service

**Features:**
- VOPR runs in infinite loop with 1000-seed batches
- JSON output parsed for metrics
- CloudWatch metrics published every batch
- SNS alerts on failures with reproduction instructions
- Checkpoint restoration on startup

### 4. Documentation

**Files Created:**
- `docs/VOPR_DEPLOYMENT.md` - Comprehensive operations guide
  - Quick start instructions
  - Cost breakdown
  - Monitoring setup
  - Troubleshooting guide
  - Advanced usage examples

### 5. Justfile Commands

**Commands Added:**
```bash
just deploy-vopr          # Deploy infrastructure
just vopr-status          # Check progress
just vopr-logs            # View live logs
just vopr-ssh             # SSH to instance
just vopr-stop            # Stop instance
just vopr-start           # Start instance
just vopr-destroy         # Destroy infrastructure
```

## Quick Deployment

### Prerequisites

1. AWS CLI configured: `aws configure`
2. Terraform installed: `brew install terraform` (macOS)
3. Email for alerts

### Deploy

```bash
# 1. Configure
cd infra/vopr-aws
cp terraform.tfvars.example terraform.tfvars
vim terraform.tfvars  # Add your email

# 2. Deploy
terraform init
terraform apply

# 3. Confirm SNS email subscription (check inbox)

# 4. Monitor
just vopr-logs
```

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│ EC2 Spot Instance (c7g.medium ARM)                      │
│ ┌─────────────────────────────────────────────────────┐ │
│ │ VOPR Binary (--json --checkpoint-file)              │ │
│ │   └─> 1000-seed batches in infinite loop            │ │
│ └────────┬────────────────────────────────────┬────────┘ │
│          │                                     │          │
│          ▼                                     ▼          │
│ ┌─────────────────┐                  ┌─────────────────┐ │
│ │ CloudWatch Logs │                  │ S3 Bucket       │ │
│ │ - VOPR output   │                  │ - checkpoints/  │ │
│ │ - 7d retention  │                  │ - failures/     │ │
│ └────────┬────────┘                  └─────────────────┘ │
└──────────┼─────────────────────────────────────────────────┘
           │
           ▼
  ┌─────────────────┐
  │ Metric Filters  │
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ CloudWatch      │
  │ Alarms          │
  │ - Failures > 0  │
  │ - No progress   │
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ SNS Topic       │
  │ ├─> Email       │
  │ └─> (Optional)  │
  └─────────────────┘
```

## Cost Breakdown

| Item | Monthly Cost |
|------|--------------|
| c7g.medium spot (730h @ $0.008/h) | $5.84 |
| EBS 20GB GP3 | $1.60 |
| CloudWatch Logs (3GB, 7d) | $1.51 |
| CloudWatch Metrics (4 custom) | $1.20 |
| S3 (10GB + requests) | $0.30 |
| SNS (100 emails) | $0.00 |
| **Total** | **~$10.45** |

## Monitoring

### CloudWatch Metrics

Two custom metrics published every batch:
- `VOPR/IterationsCompleted` - Number of simulations completed
- `VOPR/FailuresDetected` - Number of invariant violations

### Alarms

1. **Failure Detected** - Triggers when `FailuresDetected > 0`
2. **No Progress** - Triggers when `IterationsCompleted < 1` for 15 minutes

**Alert Rate Limiting:**
- Emails are rate-limited to **once per hour** to prevent flooding
- Critical alerts (>20 failures in single batch) bypass rate limit
- All failures are always archived to S3 regardless of email alerts
- Emails include summary stats, not individual failure details

### Email Alert Example

```
Subject: VOPR Alert: 8 new failures detected (Total: 360)

VOPR Simulation Update

Batch: seeds 3800 to 3900
Failures in this batch: 8
Latest failed seeds: 3825 3837 3843 3859 3871 3875 3886 3899

Overall Progress:
- Total iterations: 4400
- Total failures: 360
- Failure rate: 8.18%

Instance: i-0123456789abcdef0
Timestamp: 2026-01-30T12:00:00Z

View failures: s3://vopr-simulation-results-123456789012/failures/
View checkpoint: s3://vopr-simulation-results-123456789012/checkpoints/latest.json

To reproduce a failure:
  cargo run --release --bin vopr -- --seed <SEED> -v
```

**Note:** Alerts are rate-limited to once per hour (except critical failures >20)

## Verification Steps

After deployment:

1. **Instance Running**
   ```bash
   terraform output instance_id
   aws ec2 describe-instances --instance-ids <id> --query 'Reservations[0].Instances[0].State.Name'
   # Should show: "running"
   ```

2. **Logs Streaming**
   ```bash
   just vopr-logs
   # Should show JSON output from VOPR
   ```

3. **Metrics Publishing**
   ```bash
   just vopr-status
   # Should show iterations > 0
   ```

4. **Checkpoint Syncing**
   ```bash
   aws s3 ls s3://vopr-simulation-results-$(aws sts get-caller-identity --query Account --output text)/checkpoints/
   # Should show latest.json
   ```

## Troubleshooting

See [docs/VOPR_DEPLOYMENT.md](../../docs/VOPR_DEPLOYMENT.md) for detailed troubleshooting guide.

Common issues:
- **No logs**: Check IAM permissions and CloudWatch agent status
- **No alerts**: Confirm SNS subscription (check spam folder)
- **Instance stopped**: Spot interruption - will auto-restart and resume from checkpoint

## Next Steps

1. Deploy infrastructure: `just deploy-vopr`
2. Confirm SNS email subscription
3. Monitor for 24-48 hours
4. Review failure archives in S3 if alerts received
5. Reproduce failed seeds locally for debugging

## References

- [VOPR Deployment Guide](../../docs/VOPR_DEPLOYMENT.md)
- [Terraform AWS Provider Docs](https://registry.terraform.io/providers/hashicorp/aws/latest/docs)
- [VOPR Testing Methodology](https://tigerbeetle.com/blog/2023-07-11-we-put-a-distributed-system-in-the-microwave/)
