variable "aws_region" {
  description = "AWS region"
  type        = string
  default     = "us-east-1"
}

variable "instance_type" {
  description = "EC2 instance type (ARM Graviton recommended)"
  type        = string
  default     = "c7g.xlarge"
}

variable "spot_max_price" {
  description = "Maximum spot price per hour (default: on-demand price)"
  type        = string
  default     = "" # Empty = use on-demand price as max
}

variable "alert_email" {
  description = "Email address for daily digest and critical alerts"
  type        = string
}

variable "volume_size" {
  description = "Root EBS volume size in GB (needs space for Docker images: Coq, TLAPS, Ivy)"
  type        = number
  default     = 60
}

variable "run_duration_hours" {
  description = "Total cycle duration in hours before generating digest and restarting"
  type        = number
  default     = 48
}

variable "enable_fuzzing" {
  description = "Enable fuzz testing phase"
  type        = bool
  default     = true
}

variable "enable_formal_verification" {
  description = "Enable formal verification phase (requires Docker)"
  type        = bool
  default     = true
}

variable "enable_benchmarks" {
  description = "Enable benchmark phase"
  type        = bool
  default     = true
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
