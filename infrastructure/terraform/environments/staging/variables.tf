variable "aws_region" {
  type        = string
  description = "AWS region for the single staging cell (maps to Phase 4.1 us-primary)"
  default     = "us-east-1"
}

variable "vpc_cidr" {
  type    = string
  default = "10.40.0.0/16"
}

variable "kubernetes_version" {
  type    = string
  default = "1.31"
}

variable "node_instance_types" {
  type    = list(string)
  default = ["t3.large"]
}

variable "node_desired_size" {
  type    = number
  default = 2
}

variable "node_min_size" {
  type    = number
  default = 2
}

variable "node_max_size" {
  type    = number
  default = 4
}

variable "rds_instance_class" {
  type    = string
  default = "db.t4g.medium"
}

variable "rds_allocated_storage" {
  type    = number
  default = 50
}

variable "rds_master_username" {
  type        = string
  description = "RDS bootstrap user (not the app role). App uses companyos NOSUPERUSER."
  default     = "postgres"
}

variable "rds_master_password" {
  type        = string
  description = "RDS master password — set via TF_VAR_rds_master_password or secrets. Never commit."
  sensitive   = true
  # Placeholder so `validate` works without secrets; plan/apply require a real value.
  default = "CHANGE-ME-via-TF-VAR-before-apply"
}

variable "redis_node_type" {
  type    = string
  default = "cache.t4g.micro"
}

variable "redis_auth_token" {
  type        = string
  description = "Redis AUTH token — set via TF_VAR_redis_auth_token. Never commit."
  sensitive   = true
  default     = "CHANGE-ME-via-TF-VAR-before-apply-16chars"
}
