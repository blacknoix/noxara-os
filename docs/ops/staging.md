# Staging cell — single-region IaC (plan/validate only)

Status: **IaC shipped; nothing applied to AWS.** Synthetic data only when applied later.

## Topology (matches Phase 4.1 `us-primary`)

One staging region = one cell:

| Attribute | Value |
|-----------|--------|
| AWS region | `us-east-1` |
| Cell id | `us-primary` |
| Tenant region code | `us` |
| AZ layout | 2 AZs (multi-AZ at reduced scale) |
| Workloads | EKS (not App Runner) |
| App data | RDS PostgreSQL 16 + ElastiCache Redis 7 + S3 |
| Messaging / workflow / search / analytics backends | Self-hosted on EKS (see substitutions) |

This pack is **not** live multi-region failover. Phase 4.1 already simulates cells (`us-primary`, `us-dr`, `eu-primary`, `ap-primary`) and HTTP 451 residency in-process / CI. Staging IaC materializes **only** the US primary cell at reduced scale. EU/AP/DR cells remain code+drill until separate packs.

```
                    Internet
                       │
                   ALB (HTTPS)
                       │
         ┌─────────────┴──────────────┐
         │  EKS (private nodes)       │
         │  gateway, core, business,  │
         │  platform, ai, web         │
         │  + NATS / Temporal /       │
         │    OpenSearch / ClickHouse │
         └─────┬──────────┬───────────┘
               │          │
        RDS PG16     ElastiCache Redis
        (multi-AZ)   (multi-AZ)
               │
              S3 (files; replaces MinIO)
```

## Substitutions vs `scripts/dev-up` / Compose

| Local (`dev-up`) | Staging choice | Why |
|------------------|----------------|-----|
| Postgres 16 (compose) | **RDS PostgreSQL 16** multi-AZ, encrypted (AWS-managed key) | Managed backups; app role ≠ master |
| Redis 7 | **ElastiCache Redis 7** multi-AZ, TLS + AUTH | Matches rate-limit/SSE usage |
| MinIO | **S3** (SSE-S3, private, TLS-only policy) | AWS-native object store |
| NATS JetStream | **Helm on EKS** (`nats:2.10`) | No AWS managed NATS |
| Temporal (+ temporal-postgres) | **Helm on EKS**; Temporal DB on **same RDS** (`temporal` database) | Fewer RDS instances |
| OpenSearch (profile `full`) | **Helm on EKS** single-node | Avoid OpenSearch Service cost/complexity for staging |
| ClickHouse (profile `full`) | **Helm on EKS** single replica | No AWS managed ClickHouse; analytics still Postgres-first (see `docs/ops/gaps.md`) |
| Processes on host | **EKS Deployments** + ECR images | Kubernetes per environment |
| — | **ECR** per service image | Build later; not in this PR’s apply path |

## What is intentionally not live

- No `terraform apply` in CI (not on `main` either in this PR)
- No customer-managed KMS CMKs, PrivateLink, second region, Anycast
- No real Okta/SCIM, App Store, or live `AI_API_KEY`
- No production tenant data — staging is synthetic only
- Helm deploy / image build pipelines are **not** auto-deploy yet (CI stops at plan/validate/scan)

## PostgreSQL RLS

RDS master user is bootstrap-only. After apply, run [`infrastructure/sql/bootstrap-app-role.sql`](../../infrastructure/sql/bootstrap-app-role.sql) so apps use `companyos` with `NOSUPERUSER NOBYPASSRLS`. Superusers bypass RLS even under `FORCE ROW LEVEL SECURITY`.

## CI

Workflow job `staging-infra`:

1. `terraform fmt -check`
2. `terraform init -backend=false` + `terraform validate`
3. Checkov static scan on `infrastructure/terraform`
4. Optional `terraform plan` **only if** repo secrets `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` (or OIDC role) exist — otherwise logs a skip and **does not fail** the PR
5. **No apply job**

## Operator runbook (`make staging-plan`)

Prerequisites (human account — not this agent):

1. AWS account + IAM principal that can plan VPC/EKS/RDS/ElastiCache/S3/ECR/IAM
2. Optional remote state bucket (see `backend.tf.example`)
3. Secrets (never commit):
   - `TF_VAR_rds_master_password`
   - `TF_VAR_redis_auth_token` (≥16 chars)
   - Later for Helm: Kubernetes secret `companyos-staging-secrets` with `DATABASE_URL`, `REDIS_URL`, `AUTH_JWT_SECRET`, Temporal DB creds — **no** live `AI_API_KEY` unless intentionally testing a provider
4. From repo root:

```bash
make staging-plan
# or:
cd infrastructure/terraform/environments/staging
terraform init -backend=false   # or with backend.tf
terraform plan -var-file=terraform.tfvars.example
```

### Apply later (manual, out of band)

```bash
# ONLY from a trusted workstation / break-glass role — not CI in this PR
terraform apply
# then: bootstrap SQL, build/push images to ECR, helm upgrade -f values-staging.yaml
```

Rollback:

1. Helm: `helm rollback companyos <revision> -n companyos`
2. Terraform: `terraform plan` → targeted destroy of mistaken resources; prefer forward-fix; keep RDS `deletion_protection = true`
3. Cell traffic: keep `COMPANYOS_CELL_ID=us-primary`; do not “fail over” staging to another AWS region in this pack — use the Phase 4.1 control-plane drill for residency behaviour, not infra cutover

## Cost band

Precise AWS prices were **not** queried in this environment (pricing MCP unavailable). Treat any dollar figure as unknown until estimated in a real account with AWS Pricing Calculator / Cost Explorer against this plan.

Rough component list for a later estimate: 1× NAT Gateway, 2× `t3.large` EKS nodes, EKS control plane, RDS `db.t4g.medium` Multi-AZ, ElastiCache `cache.t4g.micro`×2, S3, ALB, ECR storage, plus pod CPU/memory for NATS/Temporal/OpenSearch/ClickHouse. **Estimate later with a real account.**

## Related docs

- [`docs/compliance/data-residency.md`](../compliance/data-residency.md) — cell catalogue
- [`docs/compliance/multi-region-evidence.md`](../compliance/multi-region-evidence.md) — what CI already proves vs live infra
- [`docs/runbooks/regional-failover.md`](../runbooks/regional-failover.md) — drill (not this staging apply)
- [`docs/runbooks/local-dev.md`](../runbooks/local-dev.md) — Compose path
