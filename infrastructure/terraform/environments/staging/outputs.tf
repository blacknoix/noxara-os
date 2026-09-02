output "cell_id" {
  value       = local.cell_id
  description = "Phase 4.1 cell id for this staging pack (us-primary)"
}

output "cell_region" {
  value       = local.cell_region
  description = "Tenant region code (us)"
}

output "aws_region" {
  value = var.aws_region
}

output "vpc_id" {
  value = module.network.vpc_id
}

output "private_subnet_ids" {
  value = module.network.private_subnet_ids
}

output "eks_cluster_name" {
  value = module.eks.cluster_name
}

output "eks_cluster_endpoint" {
  value = module.eks.cluster_endpoint
}

output "rds_endpoint" {
  value = module.rds.endpoint
}

output "redis_endpoint" {
  value = module.redis.primary_endpoint
}

output "files_bucket_id" {
  value = module.s3.files_bucket_id
}

output "ecr_repository_urls" {
  value = module.ecr.repository_urls
}

output "file_service_role_arn" {
  value = aws_iam_role.file_service.arn
}

output "helm_values_hints" {
  description = "Non-secret hints for infrastructure/helm/companyos/values-staging.yaml"
  value = {
    cell_id            = local.cell_id
    cell_region        = local.cell_region
    companyos_db_host  = module.rds.endpoint
    redis_host         = module.redis.primary_endpoint
    files_bucket       = module.s3.files_bucket_id
    file_irsa_role_arn = aws_iam_role.file_service.arn
  }
}
