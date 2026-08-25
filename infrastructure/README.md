# CompanyOS infrastructure

Phase 0 provides:

- `docker/docker-compose.yml` — local dependencies (Postgres, Redis, NATS JetStream, Temporal, MinIO; optional OpenSearch/ClickHouse via profile `full`)
- `terraform/` — skeletons only; **no live apply** in Phase 0

See `scripts/dev-up` and `docs/runbooks/local-dev.md`.
