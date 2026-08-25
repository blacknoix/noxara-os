# Terraform skeletons only — no live cloud account required for Phase 0.
# Do not apply these without reviewing backends and credentials.

terraform {
  required_version = ">= 1.5"
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}

# Backend intentionally unset for local skeletons.
