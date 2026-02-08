terraform {
  required_version = ">= 1.0"
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}

provider "aws" {
  region = var.aws_region
}

data "aws_caller_identity" "current" {}

# ============================================================================
# IAM Role and Instance Profile
# ============================================================================

resource "aws_iam_role" "vopr_instance" {
  name = "kimberlite-testing-instance-role"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Action = "sts:AssumeRole"
      Effect = "Allow"
      Principal = {
        Service = "ec2.amazonaws.com"
      }
    }]
  })

  tags = {
    Project = "kimberlite-testing"
  }
}

resource "aws_iam_role_policy" "vopr_permissions" {
  name = "kimberlite-testing-permissions"
  role = aws_iam_role.vopr_instance.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect = "Allow"
        Action = [
          "logs:CreateLogGroup",
          "logs:CreateLogStream",
          "logs:PutLogEvents",
          "logs:DescribeLogStreams"
        ]
        Resource = "arn:aws:logs:*:*:log-group:/aws/ec2/kimberlite-testing:*"
      },
      {
        Effect   = "Allow"
        Action   = "cloudwatch:PutMetricData"
        Resource = "*"
        Condition = {
          StringEquals = {
            "cloudwatch:namespace" = "Kimberlite/Testing"
          }
        }
      },
      {
        Effect = "Allow"
        Action = [
          "s3:PutObject",
          "s3:GetObject",
          "s3:ListBucket"
        ]
        Resource = [
          aws_s3_bucket.vopr_results.arn,
          "${aws_s3_bucket.vopr_results.arn}/*"
        ]
      },
      {
        Effect   = "Allow"
        Action   = "sns:Publish"
        Resource = aws_sns_topic.vopr_alerts.arn
      },
      {
        Effect = "Allow"
        Action = [
          "ssmmessages:CreateControlChannel",
          "ssmmessages:CreateDataChannel",
          "ssmmessages:OpenControlChannel",
          "ssmmessages:OpenDataChannel",
          "ssm:UpdateInstanceInformation"
        ]
        Resource = "*"
      }
    ]
  })
}

resource "aws_iam_instance_profile" "vopr" {
  name = "kimberlite-testing-profile"
  role = aws_iam_role.vopr_instance.name
}

# ============================================================================
# Security Group
# ============================================================================

resource "aws_security_group" "vopr" {
  name        = "kimberlite-testing"
  description = "Kimberlite long-running testing instance"

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = {
    Name    = "kimberlite-testing"
    Project = "kimberlite-testing"
  }
}

# ============================================================================
# EC2 Spot Instance
# ============================================================================

# Latest Amazon Linux 2023 ARM AMI
data "aws_ami" "al2023_arm" {
  most_recent = true
  owners      = ["amazon"]

  filter {
    name   = "name"
    values = ["al2023-ami-*-arm64"]
  }

  filter {
    name   = "virtualization-type"
    values = ["hvm"]
  }
}

resource "aws_instance" "vopr" {
  ami                    = data.aws_ami.al2023_arm.id
  instance_type          = var.instance_type
  iam_instance_profile   = aws_iam_instance_profile.vopr.name
  vpc_security_group_ids = [aws_security_group.vopr.id]
  user_data_base64 = base64gzip(templatefile("${path.module}/user_data.sh", {
    s3_bucket                  = aws_s3_bucket.vopr_results.id
    sns_topic_arn              = aws_sns_topic.vopr_alerts.arn
    log_group                  = aws_cloudwatch_log_group.vopr.name
    aws_region                 = var.aws_region
    github_repo                = var.github_repo
    github_branch              = var.github_branch
    run_duration_hours         = var.run_duration_hours
    enable_fuzzing             = var.enable_fuzzing
    enable_formal_verification = var.enable_formal_verification
    enable_benchmarks          = var.enable_benchmarks
  }))

  instance_market_options {
    market_type = "spot"
    spot_options {
      max_price                      = var.spot_max_price
      spot_instance_type             = "persistent"
      instance_interruption_behavior = "stop"
    }
  }

  root_block_device {
    volume_size = var.volume_size
    volume_type = "gp3"
  }

  tags = {
    Name    = "kimberlite-testing"
    Project = "kimberlite-testing"
  }
}

# ============================================================================
# CloudWatch Logs
# ============================================================================

resource "aws_cloudwatch_log_group" "vopr" {
  name              = "/aws/ec2/kimberlite-testing"
  retention_in_days = 7

  tags = {
    Project = "kimberlite-testing"
  }
}

# ============================================================================
# CloudWatch Alarms
# ============================================================================

# NOTE: The old "failure_detected" alarm has been intentionally removed.
# It fired on every batch with FailuresDetected > 0, causing email spam.
# Failure notification is now handled by the daily digest in user_data.sh.

resource "aws_cloudwatch_metric_alarm" "no_progress" {
  alarm_name          = "kimberlite-no-progress"
  comparison_operator = "LessThanThreshold"
  evaluation_periods  = 3
  metric_name         = "IterationsCompleted"
  namespace           = "Kimberlite/Testing"
  period              = 300
  statistic           = "Sum"
  threshold           = 1
  alarm_description   = "Alert when testing stops making progress (possible crash/deadlock)"
  alarm_actions       = [aws_sns_topic.vopr_alerts.arn]
  treat_missing_data  = "breaching"

  tags = {
    Project = "kimberlite-testing"
  }
}

resource "aws_cloudwatch_metric_alarm" "no_digest" {
  alarm_name          = "kimberlite-no-digest"
  comparison_operator = "LessThanThreshold"
  evaluation_periods  = 1
  metric_name         = "DigestUploaded"
  namespace           = "Kimberlite/Testing"
  period              = 93600 # 26 hours
  statistic           = "Sum"
  threshold           = 1
  alarm_description   = "Alert when no daily digest has been uploaded in 26 hours"
  alarm_actions       = [aws_sns_topic.vopr_alerts.arn]
  treat_missing_data  = "breaching"

  tags = {
    Project = "kimberlite-testing"
  }
}

# ============================================================================
# SNS Topic for Alerts
# ============================================================================

resource "aws_sns_topic" "vopr_alerts" {
  name = "kimberlite-testing-alerts"

  tags = {
    Project = "kimberlite-testing"
  }
}

resource "aws_sns_topic_subscription" "email" {
  topic_arn = aws_sns_topic.vopr_alerts.arn
  protocol  = "email"
  endpoint  = var.alert_email
}

# ============================================================================
# S3 Bucket for Results
# ============================================================================

resource "aws_s3_bucket" "vopr_results" {
  bucket = "vopr-simulation-results-${data.aws_caller_identity.current.account_id}"

  tags = {
    Project = "kimberlite-testing"
  }
}

resource "aws_s3_bucket_lifecycle_configuration" "vopr_results" {
  bucket = aws_s3_bucket.vopr_results.id

  rule {
    id     = "archive-failures"
    status = "Enabled"

    filter {
      prefix = "failures/"
    }

    transition {
      days          = 90
      storage_class = "GLACIER"
    }
  }

  rule {
    id     = "cleanup-old-checkpoints"
    status = "Enabled"

    filter {
      prefix = "checkpoints/daily/"
    }

    expiration {
      days = 30
    }
  }

  rule {
    id     = "expire-digests"
    status = "Enabled"

    filter {
      prefix = "digests/"
    }

    expiration {
      days = 30
    }
  }

  rule {
    id     = "expire-old-benchmarks"
    status = "Enabled"

    filter {
      prefix = "benchmarks/"
    }

    expiration {
      days = 90
    }
  }

  rule {
    id     = "expire-formal-verification"
    status = "Enabled"

    filter {
      prefix = "formal-verification/"
    }

    expiration {
      days = 90
    }
  }
}

# ============================================================================
# Outputs
# ============================================================================

output "instance_id" {
  description = "EC2 instance ID"
  value       = aws_instance.vopr.id
}

output "s3_bucket" {
  description = "S3 bucket name for results"
  value       = aws_s3_bucket.vopr_results.id
}

output "sns_topic" {
  description = "SNS topic ARN for alerts"
  value       = aws_sns_topic.vopr_alerts.arn
}

output "log_group" {
  description = "CloudWatch log group name"
  value       = aws_cloudwatch_log_group.vopr.name
}
