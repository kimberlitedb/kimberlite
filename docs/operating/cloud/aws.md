---
title: "Deploying on AWS"
section: "operating/cloud"
slug: "aws"
order: 1
---

# Deploying on AWS

Deploy Kimberlite on Amazon Web Services.

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                     VPC                              │
│  ┌──────────────┐  ┌──────────────┐  ┌────────────┐│
│  │   us-east-1a │  │   us-east-1b │  │  us-east-1c││
│  │              │  │              │  │            ││
│  │  ┌────────┐  │  │  ┌────────┐  │  │ ┌────────┐││
│  │  │ Node 1 │  │  │  │ Node 2 │  │  │ │ Node 3 │││
│  │  │ (Leader)│  │  │  │        │  │  │ │        │││
│  │  └────────┘  │  │  └────────┘  │  │ └────────┘││
│  └──────────────┘  └──────────────┘  └────────────┘│
└─────────────────────────────────────────────────────┘
```

## Prerequisites

- AWS account with EC2 and EBS permissions
- AWS CLI installed and configured
- Terraform or CloudFormation (optional)

## Instance Selection

**Recommended Instance Types:**

| Workload | Instance Type | vCPUs | Memory | Network | EBS |
|----------|---------------|-------|---------|---------|-----|
| Small | `t3.medium` | 2 | 4 GB | Up to 5 Gbps | GP3 100 GB |
| Medium | `c6i.xlarge` | 4 | 8 GB | Up to 12.5 Gbps | GP3 500 GB |
| Large | `c6i.2xlarge` | 8 | 16 GB | Up to 12.5 Gbps | GP3 1 TB |
| Production | `c6i.4xlarge` | 16 | 32 GB | Up to 25 Gbps | GP3 2 TB (3000 IOPS) |

**Instance Selection Tips:**
- Use `c6i` (compute-optimized) for high-throughput workloads
- Use `m6i` (general-purpose) for balanced workloads
- Use `r6i` (memory-optimized) for large projection caches

## Storage Configuration

### EBS Volume Types

| Volume Type | IOPS | Throughput | Use Case | Cost |
|-------------|------|------------|----------|------|
| **GP3** | 3,000-16,000 | 125-1,000 MB/s | Recommended for most | $0.08/GB-month |
| **IO2** | 64,000+ | 4,000 MB/s | Ultra-high performance | $0.125/GB-month |
| **ST1** | 500 (burst) | 500 MB/s | Cold data (NOT for log) | $0.045/GB-month |

**Recommendations:**
- **Log volume:** GP3 with 3000 IOPS minimum
- **Projection volume:** GP3 with 1000 IOPS minimum
- **Separate volumes** for log and projections

### Volume Configuration

```bash
# Create EBS volumes
aws ec2 create-volume \
  --availability-zone us-east-1a \
  --size 500 \
  --volume-type gp3 \
  --iops 3000 \
  --throughput 250 \
  --tag-specifications 'ResourceType=volume,Tags=[{Key=Name,Value=kimberlite-log}]'

aws ec2 create-volume \
  --availability-zone us-east-1a \
  --size 200 \
  --volume-type gp3 \
  --iops 1000 \
  --tag-specifications 'ResourceType=volume,Tags=[{Key=Name,Value=kimberlite-projections}]'

# Attach volumes
aws ec2 attach-volume --volume-id vol-xxx --instance-id i-xxx --device /dev/sdf
aws ec2 attach-volume --volume-id vol-yyy --instance-id i-xxx --device /dev/sdg

# Format and mount
sudo mkfs.ext4 /dev/nvme1n1
sudo mkfs.ext4 /dev/nvme2n1

sudo mkdir -p /var/lib/kimberlite/log
sudo mkdir -p /var/lib/kimberlite/projections

sudo mount /dev/nvme1n1 /var/lib/kimberlite/log
sudo mount /dev/nvme2n1 /var/lib/kimberlite/projections
```

### /etc/fstab Configuration

```bash
# Add to /etc/fstab for auto-mount
UUID=xxx  /var/lib/kimberlite/log          ext4  defaults,nofail  0 2
UUID=yyy  /var/lib/kimberlite/projections  ext4  defaults,nofail  0 2
```

## Networking

### Security Group Configuration

```bash
# Create security group
aws ec2 create-security-group \
  --group-name kimberlite-cluster \
  --description "Kimberlite cluster nodes" \
  --vpc-id vpc-xxx

# Client traffic (from application VPC)
aws ec2 authorize-security-group-ingress \
  --group-id sg-xxx \
  --protocol tcp \
  --port 7000 \
  --source-group sg-app

# Cluster traffic (between nodes)
aws ec2 authorize-security-group-ingress \
  --group-id sg-xxx \
  --protocol tcp \
  --port 7001 \
  --source-group sg-xxx

# Metrics (from monitoring VPC)
aws ec2 authorize-security-group-ingress \
  --group-id sg-xxx \
  --protocol tcp \
  --port 9090 \
  --source-group sg-monitoring

# SSH (from bastion only)
aws ec2 authorize-security-group-ingress \
  --group-id sg-xxx \
  --protocol tcp \
  --port 22 \
  --source-group sg-bastion
```

### Placement Groups

Use cluster placement groups for lowest latency:

```bash
aws ec2 create-placement-group \
  --group-name kimberlite-cluster \
  --strategy cluster
```

## Encryption

### At-Rest Encryption with AWS KMS

```toml
# /etc/kimberlite/config.toml
[encryption]
enabled = true
kms_provider = "aws-kms"
kms_key_id = "arn:aws:kms:us-east-1:123456789:key/abc123"
```

**Create KMS key:**

```bash
aws kms create-key \
  --description "Kimberlite encryption key" \
  --key-policy file://kms-policy.json
```

**kms-policy.json:**

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {
        "AWS": "arn:aws:iam::123456789:role/kimberlite-node"
      },
      "Action": [
        "kms:Decrypt",
        "kms:Encrypt",
        "kms:GenerateDataKey"
      ],
      "Resource": "*"
    }
  ]
}
```

### In-Transit Encryption with TLS

Generate certificates using AWS Certificate Manager or self-signed:

```bash
# Using ACM Private CA
aws acm-pca issue-certificate \
  --certificate-authority-arn arn:aws:acm-pca:us-east-1:xxx:certificate-authority/yyy \
  --csr file://server.csr \
  --signing-algorithm SHA256WITHRSA \
  --validity Value=365,Type=DAYS

# Or use Let's Encrypt with DNS challenge
certbot certonly --dns-route53 -d kimberlite.example.com
```

## IAM Configuration

**EC2 Instance Role:**

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "kms:Decrypt",
        "kms:Encrypt",
        "kms:GenerateDataKey"
      ],
      "Resource": "arn:aws:kms:us-east-1:123456789:key/abc123"
    },
    {
      "Effect": "Allow",
      "Action": [
        "ec2:DescribeInstances",
        "ec2:DescribeVolumes"
      ],
      "Resource": "*"
    },
    {
      "Effect": "Allow",
      "Action": [
        "cloudwatch:PutMetricData"
      ],
      "Resource": "*"
    }
  ]
}
```

## Deployment with Terraform

```hcl
# main.tf
resource "aws_instance" "kimberlite_node" {
  count         = 3
  ami           = "ami-xxx"  # Ubuntu 22.04
  instance_type = "c6i.xlarge"

  vpc_security_group_ids = [aws_security_group.kimberlite.id]
  subnet_id              = element(var.subnet_ids, count.index)
  placement_group        = aws_placement_group.kimberlite.id

  iam_instance_profile = aws_iam_instance_profile.kimberlite.name

  user_data = templatefile("user-data.sh", {
    node_id = count.index + 1
    cluster_peers = join(",", [for i in range(3) : "node${i+1}:7001" if i != count.index])
  })

  tags = {
    Name = "kimberlite-node-${count.index + 1}"
  }
}

resource "aws_ebs_volume" "log" {
  count             = 3
  availability_zone = element(var.availability_zones, count.index)
  size              = 500
  type              = "gp3"
  iops              = 3000
  throughput        = 250

  tags = {
    Name = "kimberlite-log-${count.index + 1}"
  }
}

resource "aws_volume_attachment" "log" {
  count       = 3
  device_name = "/dev/sdf"
  volume_id   = aws_ebs_volume.log[count.index].id
  instance_id = aws_instance.kimberlite_node[count.index].id
}
```

## Monitoring Integration

### CloudWatch Metrics

Export Kimberlite metrics to CloudWatch:

```bash
# Install CloudWatch agent
wget https://s3.amazonaws.com/amazoncloudwatch-agent/ubuntu/amd64/latest/amazon-cloudwatch-agent.deb
sudo dpkg -i amazon-cloudwatch-agent.deb

# Configure agent
cat > /opt/aws/amazon-cloudwatch-agent/etc/config.json <<'EOF'
{
  "metrics": {
    "namespace": "Kimberlite",
    "metrics_collected": {
      "prometheus": {
        "prometheus_config_path": "/opt/aws/amazon-cloudwatch-agent/etc/prometheus.yml",
        "emf_processor": {
          "metric_declaration": [
            {
              "source_labels": ["job"],
              "label_matcher": "kimberlite",
              "dimensions": [["node_id"]],
              "metric_selectors": [
                "kmb_log_entries_total",
                "kmb_write_duration_seconds"
              ]
            }
          ]
        }
      }
    }
  }
}
EOF
```

## Backup Strategy

### EBS Snapshots

```bash
# Create snapshot schedule
aws dlm create-lifecycle-policy \
  --description "Kimberlite daily snapshots" \
  --state ENABLED \
  --execution-role-arn arn:aws:iam::xxx:role/AWSDataLifecycleManagerRole \
  --policy-details file://snapshot-policy.json
```

**snapshot-policy.json:**

```json
{
  "ResourceTypes": ["VOLUME"],
  "TargetTags": [{
    "Key": "Backup",
    "Value": "kimberlite"
  }],
  "Schedules": [{
    "Name": "Daily snapshots",
    "CreateRule": {
      "Interval": 24,
      "IntervalUnit": "HOURS",
      "Times": ["03:00"]
    },
    "RetainRule": {
      "Count": 30
    }
  }]
}
```

## Cost Optimization

**Estimated Monthly Costs (3-node cluster):**

| Component | Configuration | Cost |
|-----------|---------------|------|
| 3x c6i.xlarge | 24/7 | $370 |
| 3x GP3 500 GB (log) | 3000 IOPS | $120 |
| 3x GP3 200 GB (proj) | 1000 IOPS | $48 |
| Data transfer | 1 TB/month | $90 |
| **Total** | | **~$630/month** |

**Cost Reduction Tips:**
- Use Savings Plans for 40% discount on compute
- Use Reserved Instances for steady-state workloads
- Lifecycle policy to delete old snapshots
- Use S3 Glacier for long-term archival

## Related Documentation

- **[Deployment Guide](../deployment.md)** - General deployment patterns
- **[Configuration Guide](../configuration.md)** - Configuration options
- **[Security Guide](../security.md)** - TLS setup
- **[Monitoring Guide](../monitoring.md)** - Observability

---

**Key Takeaway:** Use GP3 volumes with 3000 IOPS for log, spread nodes across AZs, enable KMS encryption, and use placement groups for low latency.
