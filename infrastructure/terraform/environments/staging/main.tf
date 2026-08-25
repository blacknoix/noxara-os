# Staging skeleton — placeholders only.

terraform {
  required_version = ">= 1.5"
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}

variable "region" {
  type        = string
  description = "AWS region (tenant region attribute is separate — see ADR 015)"
  default     = "us-east-1"
}

# No resources defined in Phase 0.
