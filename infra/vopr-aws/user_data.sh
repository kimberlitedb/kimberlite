#!/bin/bash
set -euo pipefail

# Log everything
exec > >(tee /var/log/user-data.log)
exec 2>&1

echo "=== VOPR Simulation Setup Starting ==="

# Environment variables from Terraform
export AWS_DEFAULT_REGION="${aws_region}"
export S3_BUCKET="${s3_bucket}"
export SNS_TOPIC_ARN="${sns_topic_arn}"
export LOG_GROUP="${log_group}"
export GITHUB_REPO="${github_repo}"
export GITHUB_BRANCH="${github_branch}"

# Install dependencies
echo "Installing dependencies..."
dnf update -y
dnf install -y amazon-cloudwatch-agent aws-cli jq git gcc

# Install Rust (ARM build)
echo "Installing Rust..."
export HOME=/root
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.85.0
source "$HOME/.cargo/env"

# Clone and build kimberlite
echo "Building kimberlite..."
cd /opt
git clone "$GITHUB_REPO" kimberlite
cd kimberlite
git checkout "$GITHUB_BRANCH"

# Build VOPR (release mode for performance)
cargo build --release -p kmb-sim --bin vopr

# Create checkpoint directory
mkdir -p /var/lib/vopr
chown -R ec2-user:ec2-user /var/lib/vopr

# Install VOPR runner script
cat > /usr/local/bin/vopr-runner.sh <<'RUNNER_EOF'
#!/bin/bash
set -euo pipefail

# Configuration
CHECKPOINT_FILE="/var/lib/vopr/checkpoint.json"
VOPR_BIN="/opt/kimberlite/target/release/vopr"
BATCH_SIZE=100

echo "VOPR Runner Starting"
echo "S3 Bucket: $S3_BUCKET"
echo "SNS Topic: $SNS_TOPIC_ARN"

# Restore checkpoint from S3 if exists
if aws s3 cp "s3://$S3_BUCKET/checkpoints/latest.json" "$CHECKPOINT_FILE" 2>/dev/null; then
  echo "Restored checkpoint from S3"
  LAST_SEED=$(jq -r '.last_seed // 0' "$CHECKPOINT_FILE")
else
  echo "No checkpoint found, starting from seed 0"
  LAST_SEED=0
fi

# Infinite loop: run VOPR in batches
while true; do
  BATCH_START=$LAST_SEED
  BATCH_END=$((BATCH_START + BATCH_SIZE))

  echo "Starting batch: seeds $BATCH_START to $BATCH_END"

  # Run VOPR with JSON output (limit events per iteration for performance)
  OUTPUT=$($VOPR_BIN --json --max-events 100 --checkpoint-file "$CHECKPOINT_FILE" --seed "$BATCH_START" -n "$BATCH_SIZE" 2>&1 || true)

  # Parse JSON output
  SUCCESSES=$(echo "$OUTPUT" | jq -s '[.[] | select(.type == "iteration" and .data.status == "ok")] | length')
  FAILURES=$(echo "$OUTPUT" | jq -s '[.[] | select(.type == "iteration" and .data.status == "failed")] | length')

  # Extract failed seeds and details
  FAILED_SEEDS=$(echo "$OUTPUT" | jq -r 'select(.type == "iteration" and .data.status == "failed") | .data.seed' | tr '\n' ' ')

  echo "Batch complete: $SUCCESSES successes, $FAILURES failures"

  # Publish metrics to CloudWatch
  aws cloudwatch put-metric-data \
    --namespace VOPR \
    --metric-name IterationsCompleted \
    --value "$BATCH_SIZE" \
    --unit Count \
    --region "$AWS_DEFAULT_REGION"

  aws cloudwatch put-metric-data \
    --namespace VOPR \
    --metric-name FailuresDetected \
    --value "$FAILURES" \
    --unit Count \
    --region "$AWS_DEFAULT_REGION"

  # If failures detected, archive them (always) and maybe send alert (rate limited)
  if [[ "$FAILURES" -gt 0 ]]; then
    echo "Found $FAILURES failure(s) in batch"

    # Archive all failures to S3
    for SEED in $FAILED_SEEDS; do
      TIMESTAMP=$(date +%s)
      echo "$OUTPUT" | jq -s '.' > "/tmp/failure-$SEED-$TIMESTAMP.json"
      aws s3 cp "/tmp/failure-$SEED-$TIMESTAMP.json" \
        "s3://$S3_BUCKET/failures/seed-$SEED-$TIMESTAMP.json" \
        --region "$AWS_DEFAULT_REGION" 2>&1 | grep -v "Completed"
      rm "/tmp/failure-$SEED-$TIMESTAMP.json"
    done

    # Rate-limited alerting: only send SNS alert once per hour OR if >20 failures in single batch
    LAST_ALERT_FILE="/var/lib/vopr/last_alert_time"
    CURRENT_TIME=$(date +%s)
    ALERT_COOLDOWN=3600  # 1 hour in seconds
    CRITICAL_FAILURE_THRESHOLD=20

    SHOULD_ALERT=false
    if [[ "$FAILURES" -ge "$CRITICAL_FAILURE_THRESHOLD" ]]; then
      SHOULD_ALERT=true
      echo "CRITICAL: $FAILURES failures exceeds threshold ($CRITICAL_FAILURE_THRESHOLD)"
    elif [[ ! -f "$LAST_ALERT_FILE" ]]; then
      SHOULD_ALERT=true
      echo "First alert - sending notification"
    else
      LAST_ALERT_TIME=$(cat "$LAST_ALERT_FILE")
      TIME_SINCE_ALERT=$((CURRENT_TIME - LAST_ALERT_TIME))
      if [[ "$TIME_SINCE_ALERT" -ge "$ALERT_COOLDOWN" ]]; then
        SHOULD_ALERT=true
        echo "Cooldown expired ($${TIME_SINCE_ALERT}s > $${ALERT_COOLDOWN}s) - sending alert"
      else
        echo "Alert suppressed - cooldown active ($${TIME_SINCE_ALERT}s / $${ALERT_COOLDOWN}s)"
      fi
    fi

    if [[ "$SHOULD_ALERT" == "true" ]]; then
      # Get total stats from checkpoint
      TOTAL_FAILURES=$(jq -r '.total_failures // 0' "$CHECKPOINT_FILE" 2>/dev/null || echo "0")
      TOTAL_ITERATIONS=$(jq -r '.total_iterations // 0' "$CHECKPOINT_FILE" 2>/dev/null || echo "0")

      # Send summary alert
      aws sns publish \
        --topic-arn "$SNS_TOPIC_ARN" \
        --subject "VOPR Alert: $FAILURES new failures detected (Total: $TOTAL_FAILURES)" \
        --message "$(cat <<SNS_MSG
VOPR Simulation Update

Batch: seeds $BATCH_START to $BATCH_END
Failures in this batch: $FAILURES
Latest failed seeds: $FAILED_SEEDS

Overall Progress:
- Total iterations: $TOTAL_ITERATIONS
- Total failures: $TOTAL_FAILURES
- Failure rate: $(awk "BEGIN {printf \"%.2f%%\", ($TOTAL_FAILURES/$TOTAL_ITERATIONS)*100}")

Instance: $(ec2-metadata --instance-id | cut -d' ' -f2)
Timestamp: $(date -u +%Y-%m-%dT%H:%M:%SZ)

View failures: s3://$S3_BUCKET/failures/
View checkpoint: s3://$S3_BUCKET/checkpoints/latest.json

To reproduce a failure:
  cargo run --release --bin vopr -- --seed <SEED> -v
SNS_MSG
)" \
        --region "$AWS_DEFAULT_REGION" 2>&1 | grep MessageId

      # Update last alert time
      echo "$CURRENT_TIME" > "$LAST_ALERT_FILE"
    fi
  fi

  # Sync checkpoint to S3 every batch
  if [[ -f "$CHECKPOINT_FILE" ]]; then
    aws s3 cp "$CHECKPOINT_FILE" "s3://$S3_BUCKET/checkpoints/latest.json" --region "$AWS_DEFAULT_REGION"

    # Daily snapshot
    DAILY_CHECKPOINT="checkpoints/daily/$(date +%Y-%m-%d).json"
    aws s3 cp "$CHECKPOINT_FILE" "s3://$S3_BUCKET/$DAILY_CHECKPOINT" --region "$AWS_DEFAULT_REGION"
  fi

  # Update last seed for next batch
  LAST_SEED=$BATCH_END

  # Small delay to avoid hammering
  sleep 1
done
RUNNER_EOF

chmod +x /usr/local/bin/vopr-runner.sh

# Configure CloudWatch Agent
echo "Configuring CloudWatch Agent..."
cat > /opt/aws/amazon-cloudwatch-agent/etc/config.json <<CW_EOF
{
  "logs": {
    "logs_collected": {
      "files": {
        "collect_list": [
          {
            "file_path": "/var/log/vopr-runner.log",
            "log_group_name": "${log_group}",
            "log_stream_name": "{instance_id}",
            "timezone": "UTC"
          }
        ]
      }
    }
  },
  "metrics": {
    "namespace": "VOPR/System",
    "metrics_collected": {
      "cpu": {
        "measurement": [
          {"name": "cpu_usage_idle", "rename": "CPU_IDLE", "unit": "Percent"}
        ],
        "metrics_collection_interval": 60
      },
      "mem": {
        "measurement": [
          {"name": "mem_used_percent", "rename": "MEM_USED", "unit": "Percent"}
        ],
        "metrics_collection_interval": 60
      }
    }
  }
}
CW_EOF

# Start CloudWatch Agent
/opt/aws/amazon-cloudwatch-agent/bin/amazon-cloudwatch-agent-ctl \
  -a fetch-config \
  -m ec2 \
  -s \
  -c file:/opt/aws/amazon-cloudwatch-agent/etc/config.json

# Create systemd service
echo "Creating systemd service..."
cat > /etc/systemd/system/vopr-sim.service <<SERVICE_EOF
[Unit]
Description=VOPR Deterministic Simulation Runner
After=network.target amazon-cloudwatch-agent.service

[Service]
Type=simple
User=ec2-user
WorkingDirectory=/opt/kimberlite
Environment="AWS_DEFAULT_REGION=${aws_region}"
Environment="S3_BUCKET=${s3_bucket}"
Environment="SNS_TOPIC_ARN=${sns_topic_arn}"
ExecStart=/usr/local/bin/vopr-runner.sh
Restart=always
RestartSec=10
StandardOutput=append:/var/log/vopr-runner.log
StandardError=append:/var/log/vopr-runner.log

[Install]
WantedBy=multi-user.target
SERVICE_EOF

# Start service
systemctl daemon-reload
systemctl enable vopr-sim.service
systemctl start vopr-sim.service

echo "=== VOPR Simulation Setup Complete ==="
echo "Service status:"
systemctl status vopr-sim.service --no-pager
