# VOPR AWS Deployment Guide

Continuous deterministic simulation testing on AWS with monitoring and edge-case alerting.

## Architecture

```
EC2 Spot Instance (c7g.medium ARM Graviton)
  └─> VOPR Binary (enhanced with JSON + checkpoints)
       ├─> CloudWatch Logs ─> Metric Filters ─> SNS Email Alerts
       └─> S3 Checkpoints + Failure Archive
```

**Target Cost:** ~$10/month
**Deployment Time:** ~30 minutes

## Prerequisites

- AWS Account with CLI configured (`aws configure`)
- Terraform >= 1.0
- Email address for alerts
- Valid IAM permissions for EC2, IAM, CloudWatch, SNS, S3

## Quick Start

### 1. Configure Variables

```bash
cd infra/vopr-aws

# Copy example config
cp terraform.tfvars.example terraform.tfvars

# Edit with your email
vim terraform.tfvars
```

Example `terraform.tfvars`:
```hcl
aws_region    = "us-east-1"
instance_type = "c7g.medium"
alert_email   = "your-email@example.com"
```

### 2. Deploy Infrastructure

```bash
# Initialize Terraform
terraform init

# Review plan
terraform plan

# Deploy
terraform apply
```

### 3. Confirm SNS Email Subscription

Check your email for SNS confirmation and click the link. You won't receive alerts until confirmed.

### 4. Monitor Progress

```bash
# View live logs
aws logs tail /aws/ec2/vopr-simulation --follow

# Check current progress
aws s3 cp s3://vopr-simulation-results-$(aws sts get-caller-identity --query Account --output text)/checkpoints/latest.json - | jq .

# View CloudWatch metrics
aws cloudwatch get-metric-statistics \
  --namespace VOPR \
  --metric-name IterationsCompleted \
  --start-time $(date -u -d '1 hour ago' +%Y-%m-%dT%H:%M:%S) \
  --end-time $(date -u +%Y-%m-%dT%H:%M:%S) \
  --period 3600 \
  --statistics Sum
```

## Features

### JSON Output Mode

VOPR now supports `--json` flag for structured output:

```bash
cargo run --release --bin vopr -- --json -n 10
```

Output format:
```json
{"timestamp":"2026-01-30T12:00:00Z","type":"start","data":{"starting_seed":0,"iterations":10}}
{"timestamp":"2026-01-30T12:00:01Z","type":"iteration","data":{"seed":0,"status":"ok","events":8432}}
{"timestamp":"2026-01-30T12:00:02Z","type":"iteration","data":{"seed":1,"status":"failed","invariant":"linearizability","message":"..."}}
{"timestamp":"2026-01-30T12:01:00Z","type":"batch_complete","data":{"successes":9,"failures":1,"rate":476.2}}
```

### Checkpoint/Resume Support

VOPR automatically saves progress to checkpoint file:

```bash
cargo run --release --bin vopr -- --checkpoint-file /tmp/checkpoint.json -n 1000
```

Checkpoint format:
```json
{
  "last_seed": 1000,
  "total_iterations": 1000,
  "total_failures": 0,
  "failed_seeds": [],
  "last_update": "2026-01-30T12:00:00Z"
}
```

This allows seamless resumption after spot interruptions.

## Operations

### Check Service Status

```bash
# SSH to instance (requires AWS SSM)
aws ssm start-session --target $(cd infra/vopr-aws && terraform output -raw instance_id)

# Check service
sudo systemctl status vopr-sim

# View logs
sudo journalctl -u vopr-sim -f
```

### Manual Seed Reproduction

When you receive an alert, reproduce locally:

```bash
cargo run --release --bin vopr -- --seed {failed-seed} -v
```

Example from alert:
```
Seed: 42
Invariant: linearizability
Message: History is not linearizable
Reproduce: cargo run --release --bin vopr -- --seed 42 -v
```

### View Failure Archives

```bash
# List all failures
aws s3 ls s3://vopr-simulation-results-$(aws sts get-caller-identity --query Account --output text)/failures/

# Download specific failure
aws s3 cp s3://vopr-simulation-results-{account}/failures/seed-42-1738329600.json - | jq .
```

### Stop/Start Instance

```bash
# Stop instance (to save costs)
aws ec2 stop-instances --instance-ids $(cd infra/vopr-aws && terraform output -raw instance_id)

# Start instance (resumes from checkpoint)
aws ec2 start-instances --instance-ids $(cd infra/vopr-aws && terraform output -raw instance_id)
```

### Update VOPR Code

```bash
# SSH to instance
aws ssm start-session --target $(cd infra/vopr-aws && terraform output -raw instance_id)

# Update and rebuild
sudo su - ec2-user
cd /opt/kimberlite
git pull
cargo build --release -p kmb-sim --bin vopr

# Restart service
sudo systemctl restart vopr-sim
```

### Destroy Infrastructure

```bash
cd infra/vopr-aws
terraform destroy
```

**Warning:** This deletes the S3 bucket with all failure archives and checkpoints.

## Cost Breakdown

**Monthly costs (c7g.medium spot in us-east-1):**

| Item | Cost |
|------|------|
| c7g.medium spot (730h @ $0.008/h) | $5.84 |
| EBS 20GB GP3 | $1.60 |
| CloudWatch Logs (3GB, 7d retention) | $1.51 |
| CloudWatch Metrics (4 custom) | $1.20 |
| S3 (10GB + requests) | $0.30 |
| SNS (100 emails) | $0.00 |
| **Total** | **~$10.45** |

**Cost optimization tips:**
- Use t4g.small ($3/month) for lower throughput
- Reduce CloudWatch log retention to 3 days ($0.75)
- Use on-demand instead of spot for higher availability (+$7/month)

## Monitoring

### CloudWatch Dashboards

Access CloudWatch console:
```
https://console.aws.amazon.com/cloudwatch/home?region=us-east-1#metricsV2:graph=~();namespace=VOPR
```

Key metrics:
- `VOPR/IterationsCompleted` - Simulations per period
- `VOPR/FailuresDetected` - Invariant violations

### Alarms

Two CloudWatch alarms are configured:

1. **Failure Detected** - Triggers on any `FailuresDetected > 0`
2. **No Progress** - Triggers if `IterationsCompleted < 1` for 15 minutes

Both send email via SNS.

### Email Alerts

Example alert email:
```
Subject: VOPR: 1 Invariant Violation(s) Detected

VOPR detected 1 invariant violation(s) in batch starting at seed 42000.

Seed: 42042
Invariant: linearizability
Message: Final history is not linearizable
Reproduce: cargo run --release --bin vopr -- --seed 42042 -v
---

Instance: i-0123456789abcdef0
Timestamp: 2026-01-30T12:00:00Z

S3 Archive: s3://vopr-simulation-results-123456789012/failures/
```

## Troubleshooting

### Instance keeps stopping

**Cause:** Spot instance interrupted by AWS.

**Solutions:**
1. Check interruption logs:
   ```bash
   aws ec2 describe-spot-instance-requests --filters "Name=instance-id,Values=$(terraform output -raw instance_id)"
   ```
2. Switch to on-demand: In `main.tf`, remove `instance_market_options` block
3. Increase max spot price: Set `spot_max_price = "0.05"` in `terraform.tfvars`

### No logs in CloudWatch

**Diagnostic steps:**
1. Check IAM permissions:
   ```bash
   aws iam get-role-policy --role-name vopr-simulation-instance-role --policy-name vopr-permissions | jq .
   ```
2. Verify CloudWatch agent running:
   ```bash
   aws ssm start-session --target $(terraform output -raw instance_id)
   sudo systemctl status amazon-cloudwatch-agent
   ```
3. Check agent logs:
   ```bash
   sudo tail -f /opt/aws/amazon-cloudwatch-agent/logs/amazon-cloudwatch-agent.log
   ```

### No email alerts

**Diagnostic steps:**
1. Confirm SNS subscription:
   ```bash
   aws sns list-subscriptions-by-topic --topic-arn $(terraform output -raw sns_topic)
   ```
   Status should be "Confirmed", not "PendingConfirmation"

2. Check spam folder for confirmation email

3. Test SNS manually:
   ```bash
   aws sns publish \
     --topic-arn $(terraform output -raw sns_topic) \
     --subject "Test Alert" \
     --message "This is a test"
   ```

### VOPR binary crashes

**Check logs:**
```bash
aws logs tail /aws/ec2/vopr-simulation --since 10m
```

**Common issues:**
- Out of memory: Reduce `max_events` in VOPR runner script
- Segfault: Check Rust version matches MSRV (1.85)
- Checkpoint corruption: Delete checkpoint file and restart

## Advanced Usage

### Run Multiple Instances

Deploy multiple instances testing different seed ranges:

```bash
# Create workspace for second instance
terraform workspace new vopr-instance-2

# Deploy with different seed offset
terraform apply
```

Then modify `user_data.sh` to set different `BATCH_START` offsets.

### Custom Fault Scenarios

Edit `/usr/local/bin/vopr-runner.sh` on instance:

```bash
# Add custom VOPR flags
$VOPR_BIN --json --checkpoint-file "$CHECKPOINT_FILE" \
  --seed "$BATCH_START" -n "$BATCH_SIZE" \
  --faults network \
  --max-events 20000
```

Restart service:
```bash
sudo systemctl restart vopr-sim
```

### Export Metrics to External Systems

CloudWatch metrics can be exported to:
- Prometheus via CloudWatch Exporter
- Grafana via CloudWatch data source
- Datadog via AWS integration

Example Prometheus scrape config:
```yaml
scrape_configs:
  - job_name: 'vopr'
    static_configs:
      - targets: ['cloudwatch-exporter:9106']
    params:
      region: ['us-east-1']
      namespace: ['VOPR']
```

## Security Considerations

- EC2 instance has no SSH ingress (use AWS SSM)
- IAM role follows least privilege (scoped to specific resources)
- S3 bucket is private (no public access)
- SNS topic restricted to account
- Spot instance stops (not terminates) on interruption, preserving EBS

## References

- [VOPR Testing Methodology](https://tigerbeetle.com/blog/2023-07-11-we-put-a-distributed-system-in-the-microwave/)
- [FoundationDB Testing](https://www.foundationdb.org/blog/deterministic-simulation-testing/)
- [AWS Spot Best Practices](https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/spot-best-practices.html)
- [Terraform AWS Provider](https://registry.terraform.io/providers/hashicorp/aws/latest/docs)
