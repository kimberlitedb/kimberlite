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
  name = "vopr-simulation-instance-role"

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
    Project = "kimberlite-vopr"
  }
}

resource "aws_iam_role_policy" "vopr_permissions" {
  name = "vopr-permissions"
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
        Resource = "arn:aws:logs:*:*:log-group:/aws/ec2/vopr-simulation:*"
      },
      {
        Effect = "Allow"
        Action = "cloudwatch:PutMetricData"
        Resource = "*"
        Condition = {
          StringEquals = {
            "cloudwatch:namespace" = "VOPR"
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
        Effect = "Allow"
        Action = "sns:Publish"
        Resource = aws_sns_topic.vopr_alerts.arn
      }
    ]
  })
}

resource "aws_iam_instance_profile" "vopr" {
  name = "vopr-simulation-profile"
  role = aws_iam_role.vopr_instance.name
}

# ============================================================================
# Security Group
# ============================================================================

resource "aws_security_group" "vopr" {
  name        = "vopr-simulation"
  description = "VOPR simulation instance"

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = {
    Name    = "vopr-simulation"
    Project = "kimberlite-vopr"
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
  user_data              = templatefile("${path.module}/user_data.sh", {
    s3_bucket     = aws_s3_bucket.vopr_results.id
    sns_topic_arn = aws_sns_topic.vopr_alerts.arn
    log_group     = aws_cloudwatch_log_group.vopr.name
    aws_region    = var.aws_region
    github_repo   = var.github_repo
    github_branch = var.github_branch
  })

  instance_market_options {
    market_type = "spot"
    spot_options {
      max_price                      = var.spot_max_price
      spot_instance_type             = "persistent"
      instance_interruption_behavior = "stop"
    }
  }

  root_block_device {
    volume_size = 20
    volume_type = "gp3"
  }

  tags = {
    Name    = "vopr-simulation"
    Project = "kimberlite-vopr"
  }
}

# ============================================================================
# CloudWatch Logs
# ============================================================================

resource "aws_cloudwatch_log_group" "vopr" {
  name              = "/aws/ec2/vopr-simulation"
  retention_in_days = 7

  tags = {
    Project = "kimberlite-vopr"
  }
}

# ============================================================================
# CloudWatch Alarms
# ============================================================================

resource "aws_cloudwatch_metric_alarm" "failure_detected" {
  alarm_name          = "vopr-failure-detected"
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = 1
  metric_name         = "FailuresDetected"
  namespace           = "VOPR"
  period              = 60
  statistic           = "Sum"
  threshold           = 0
  alarm_description   = "Alert when VOPR detects an invariant violation"
  alarm_actions       = [aws_sns_topic.vopr_alerts.arn]
  treat_missing_data  = "notBreaching"

  tags = {
    Project = "kimberlite-vopr"
  }
}

resource "aws_cloudwatch_metric_alarm" "no_progress" {
  alarm_name          = "vopr-no-progress"
  comparison_operator = "LessThanThreshold"
  evaluation_periods  = 3
  metric_name         = "IterationsCompleted"
  namespace           = "VOPR"
  period              = 300
  statistic           = "Sum"
  threshold           = 1
  alarm_description   = "Alert when VOPR stops making progress (possible crash/deadlock)"
  alarm_actions       = [aws_sns_topic.vopr_alerts.arn]
  treat_missing_data  = "breaching"

  tags = {
    Project = "kimberlite-vopr"
  }
}

# ============================================================================
# SNS Topic for Alerts
# ============================================================================

resource "aws_sns_topic" "vopr_alerts" {
  name = "vopr-simulation-alerts"

  tags = {
    Project = "kimberlite-vopr"
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
    Project = "kimberlite-vopr"
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
