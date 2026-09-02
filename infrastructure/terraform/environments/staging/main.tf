# CompanyOS staging cell — us-east-1 / us-primary
#
# DO NOT APPLY from CI. This pack is plan/validate/scan only.
# See docs/ops/staging.md and `make staging-plan`.

terraform {
  required_version = ">= 1.5"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
    tls = {
      source  = "hashicorp/tls"
      version = "~> 4.0"
    }
  }

  # Backend is intentionally unset in-repo. Copy backend.tf.example when an
  # account exists; CI uses `terraform init -backend=false`.
}

provider "aws" {
  region = var.aws_region

  default_tags {
    tags = local.common_tags
  }
}

locals {
  name = "companyos-staging"

  # Phase 4.1 cell identity — one staging region only (not live multi-region).
  cell_id     = "us-primary"
  cell_region = "us"

  common_tags = {
    Project     = "companyos"
    Environment = "staging"
    CellId      = local.cell_id
    CellRegion  = local.cell_region
    ManagedBy   = "terraform"
    # Synthetic data only — never production tenant data.
    DataClass = "synthetic"
  }

  ecr_repositories = [
    "gateway",
    "core",
    "crm",
    "finance",
    "project",
    "project-worker",
    "hr",
    "inventory",
    "notification",
    "search",
    "analytics",
    "file",
    "outbox-relay",
    "workflow-host",
    "workflow",
    "ai",
    "integration",
    "custom",
    "web",
  ]
}

data "aws_availability_zones" "available" {
  state = "available"
}

locals {
  azs = slice(data.aws_availability_zones.available.names, 0, 2)
}

# ---------------------------------------------------------------------------
# Network
# ---------------------------------------------------------------------------

module "network" {
  source = "../../modules/network"

  name = local.name
  cidr = var.vpc_cidr
  azs  = local.azs
  tags = local.common_tags
}

# ---------------------------------------------------------------------------
# IAM (cluster/node) → EKS → IRSA
# ---------------------------------------------------------------------------

module "iam" {
  source = "../../modules/iam"

  name = local.name
  tags = local.common_tags
}

module "eks" {
  source = "../../modules/eks"

  name                = local.name
  vpc_id              = module.network.vpc_id
  subnet_ids          = module.network.private_subnet_ids
  cluster_role_arn    = module.iam.eks_cluster_role_arn
  node_role_arn       = module.iam.eks_node_role_arn
  kubernetes_version  = var.kubernetes_version
  node_instance_types = var.node_instance_types
  node_desired_size   = var.node_desired_size
  node_min_size       = var.node_min_size
  node_max_size       = var.node_max_size
  tags                = local.common_tags

  # Ensure IAM policy attachments exist before control plane / nodes create.
  depends_on = [module.iam]
}

# ---------------------------------------------------------------------------
# Data plane (managed)
# ---------------------------------------------------------------------------

module "rds" {
  source = "../../modules/rds"

  name                       = local.name
  vpc_id                     = module.network.vpc_id
  subnet_ids                 = module.network.private_subnet_ids
  allowed_security_group_ids = [module.eks.node_security_group_id]
  instance_class             = var.rds_instance_class
  allocated_storage          = var.rds_allocated_storage
  multi_az                   = true
  master_username            = var.rds_master_username
  master_password            = var.rds_master_password
  tags                       = local.common_tags
}

module "redis" {
  source = "../../modules/redis"

  name                       = local.name
  vpc_id                     = module.network.vpc_id
  subnet_ids                 = module.network.private_subnet_ids
  allowed_security_group_ids = [module.eks.node_security_group_id]
  node_type                  = var.redis_node_type
  num_cache_clusters         = 2
  auth_token                 = var.redis_auth_token
  tags                       = local.common_tags
}

module "s3" {
  source = "../../modules/s3"

  name          = local.name
  force_destroy = false
  tags          = local.common_tags
}

module "ecr" {
  source = "../../modules/ecr"

  name         = local.name
  repositories = local.ecr_repositories
  tags         = local.common_tags
}

# ---------------------------------------------------------------------------
# IRSA — file-service → S3 (least privilege)
# ---------------------------------------------------------------------------

data "aws_iam_policy_document" "file_irsa_assume" {
  statement {
    actions = ["sts:AssumeRoleWithWebIdentity"]
    principals {
      type        = "Federated"
      identifiers = [module.eks.oidc_provider_arn]
    }
    condition {
      test     = "StringEquals"
      variable = "${module.eks.oidc_provider_url}:sub"
      values   = ["system:serviceaccount:companyos:file-service"]
    }
    condition {
      test     = "StringEquals"
      variable = "${module.eks.oidc_provider_url}:aud"
      values   = ["sts.amazonaws.com"]
    }
  }
}

resource "aws_iam_role" "file_service" {
  name               = "${local.name}-file-service"
  assume_role_policy = data.aws_iam_policy_document.file_irsa_assume.json
  tags               = local.common_tags
}

data "aws_iam_policy_document" "file_s3" {
  statement {
    sid       = "ListBucket"
    actions   = ["s3:ListBucket", "s3:GetBucketLocation"]
    resources = [module.s3.files_bucket_arn]
  }
  statement {
    sid = "ObjectRW"
    actions = [
      "s3:GetObject",
      "s3:PutObject",
      "s3:DeleteObject",
      "s3:AbortMultipartUpload",
      "s3:ListMultipartUploadParts",
    ]
    resources = ["${module.s3.files_bucket_arn}/*"]
  }
}

resource "aws_iam_role_policy" "file_s3" {
  name   = "${local.name}-file-s3"
  role   = aws_iam_role.file_service.id
  policy = data.aws_iam_policy_document.file_s3.json
}
