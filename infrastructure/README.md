# CompanyOS infrastructure

| Path | Purpose |
|------|---------|
| `docker/docker-compose.yml` | Local deps (Postgres, Redis, NATS, Temporal, MinIO; optional OpenSearch/ClickHouse via profile `full`) |
| `docker/Dockerfile.rust` / `Dockerfile.web` | Container images for staging (not used by `dev-up`) |
| `terraform/` | Cloud resources for the **staging** cell (`us-east-1` / `us-primary`) |
| `helm/companyos/` | Kubernetes workloads + self-hosted deps for that cell |
| `sql/bootstrap-app-role.sql` | Post-RDS app role (`companyos` NOSUPERUSER NOBYPASSRLS) |

**Do not apply Terraform from CI.** Plan / validate / scan only — see [`docs/ops/staging.md`](../docs/ops/staging.md) and `make staging-plan`.

Local bootstrap remains `scripts/dev-up` + Compose.
