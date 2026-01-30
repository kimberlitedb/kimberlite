variable "aws_region" {
  description = "AWS region"
  type        = string
  default     = "us-east-1"
}

variable "instance_type" {
  description = "EC2 instance type (ARM Graviton recommended)"
  type        = string
  default     = "c7g.medium"
}

variable "spot_max_price" {
  description = "Maximum spot price per hour (default: on-demand price)"
  type        = string
  default     = ""  # Empty = use on-demand price as max
}

variable "alert_email" {
  description = "Email address for VOPR failure alerts"
  type        = string
}

variable "github_repo" {
  description = "GitHub repository URL"
  type        = string
  default     = "https://github.com/kimberlitedb/kimberlite.git"
}

variable "github_branch" {
  description = "GitHub branch to deploy"
  type        = string
  default     = "main"
}
