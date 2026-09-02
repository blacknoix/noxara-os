# RDS PostgreSQL 16 for one cell — encryption at rest (AWS-managed key, not CMK).
# Master user is rds_superuser-capable; app role `companyos` is created post-apply
# via infrastructure/sql/bootstrap-app-role.sql (NOSUPERUSER NOBYPASSRLS).

terraform {
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = ">= 5.0"
    }
  }
}

variable "name" {
  type = string
}

variable "vpc_id" {
  type = string
}

variable "subnet_ids" {
  type        = list(string)
  description = "Private subnet IDs for the DB subnet group"
}

variable "allowed_security_group_ids" {
  type        = list(string)
  description = "Security groups allowed to reach Postgres (EKS nodes / pods)"
}

variable "instance_class" {
  type    = string
  default = "db.t4g.medium"
}

variable "allocated_storage" {
  type    = number
  default = 50
}

variable "multi_az" {
  type        = bool
  description = "Staging mirrors prod topology: multi-AZ"
  default     = true
}

variable "engine_version" {
  type    = string
  default = "16.4"
}

variable "db_name" {
  type    = string
  default = "companyos"
}

variable "master_username" {
  type        = string
  description = "RDS master (bootstrap) user — not the app role"
  default     = "postgres"
}

variable "master_password" {
  type        = string
  description = "Master password (from Secrets Manager / TF_VAR; never commit)"
  sensitive   = true
}

variable "tags" {
  type    = map(string)
  default = {}
}

resource "aws_db_subnet_group" "this" {
  name       = "${var.name}-pg"
  subnet_ids = var.subnet_ids
  tags = merge(var.tags, {
    Name = "${var.name}-pg-subnets"
  })
}

resource "aws_security_group" "this" {
  name        = "${var.name}-pg"
  description = "CompanyOS Postgres — private only"
  vpc_id      = var.vpc_id

  tags = merge(var.tags, {
    Name = "${var.name}-pg"
  })
}

resource "aws_security_group_rule" "ingress_from_workloads" {
  count = length(var.allowed_security_group_ids)

  type                     = "ingress"
  from_port                = 5432
  to_port                  = 5432
  protocol                 = "tcp"
  security_group_id        = aws_security_group.this.id
  source_security_group_id = var.allowed_security_group_ids[count.index]
  description              = "Postgres from workload SG ${count.index}"
}

resource "aws_security_group_rule" "egress_all" {
  type              = "egress"
  from_port         = 0
  to_port           = 0
  protocol          = "-1"
  cidr_blocks       = ["0.0.0.0/0"]
  security_group_id = aws_security_group.this.id
  description       = "Allow egress for RDS managed networking"
}

resource "aws_db_instance" "this" {
  identifier     = "${var.name}-pg"
  engine         = "postgres"
  engine_version = var.engine_version
  instance_class = var.instance_class

  allocated_storage     = var.allocated_storage
  max_allocated_storage = var.allocated_storage * 2
  storage_type          = "gp3"
  storage_encrypted     = true
  # AWS-managed key only — no customer-managed CMK in this pack.

  db_name  = var.db_name
  username = var.master_username
  password = var.master_password

  db_subnet_group_name   = aws_db_subnet_group.this.name
  vpc_security_group_ids = [aws_security_group.this.id]
  publicly_accessible    = false
  multi_az               = var.multi_az

  backup_retention_period             = 7
  deletion_protection                 = true
  skip_final_snapshot                 = false
  final_snapshot_identifier           = "${var.name}-pg-final"
  copy_tags_to_snapshot               = true
  auto_minor_version_upgrade          = true
  iam_database_authentication_enabled = true

  enabled_cloudwatch_logs_exports = ["postgresql", "upgrade"]

  # Performance Insights optional; keep staging lean.
  performance_insights_enabled = false

  # Force SSL from clients.
  parameter_group_name = aws_db_parameter_group.this.name

  tags = merge(var.tags, {
    Name = "${var.name}-pg"
  })
}

resource "aws_db_parameter_group" "this" {
  name   = "${var.name}-pg16"
  family = "postgres16"

  parameter {
    name  = "rds.force_ssl"
    value = "1"
  }

  tags = var.tags
}

output "endpoint" {
  value = aws_db_instance.this.address
}

output "port" {
  value = aws_db_instance.this.port
}

output "db_name" {
  value = aws_db_instance.this.db_name
}

output "security_group_id" {
  value = aws_security_group.this.id
}

output "master_username" {
  value = aws_db_instance.this.username
}
