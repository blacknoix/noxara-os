# Platform service group (Phase 1.8)

Search, files (MinIO), notifications, workflow workers (Temporal), analytics
ingestion, and the outbox → NATS JetStream relay live here.

| Package | Binary / port | Role |
|---------|---------------|------|
| `companyos-outbox-relay` | `:8090` | Poll `outbox_event`, publish to NATS JetStream (`NATS_URL`) or log via `MemoryPublisher`; `/healthz` + `/metrics`; CLI `replay --all\|--id` |
| `companyos-notification` | `:8085` | In-app feed, preferences, event ingest with authz + quiet-hours email deferral |
| `companyos-search` | `:8086` | Event-driven search index (in-memory or OpenSearch); `org_id` required on query; authz re-check per hit |
| `companyos-analytics` | `:8087` | ADR-011 facts from events only → ClickHouse or Postgres mirror `analytics_fact_invoice_issued` |
| `companyos-file` | `:8089` | Presigned upload (MinIO or local stub); allowlist pdf/png/jpeg/webp ≤10MB |
| `companyos-workflow-host` | `:8091` | Temporal catalogue host (`TEMPORAL_NAMESPACE`, default `companyos-local`) |
| `companyos-integration` | `:8095` | Outbound org webhooks + Phase 3.4 marketplace (listings, review, installs, app OAuth) |

## Env

| Variable | Service | Notes |
|----------|---------|-------|
| `DATABASE_URL` | all DB-backed | Shared core Postgres |
| `NATS_URL` | outbox-relay | When unset → MemoryPublisher (dev) |
| `OPENSEARCH_URL` | search | When unset → in-memory `HashMap` |
| `CLICKHOUSE_URL` | analytics | When unset → Postgres mirror (CI) |
| `MINIO_ENDPOINT` | file | When unset → local stub upload URL |
| `AUTH_MAIL_DIR` | notification | Email catcher dir (else stdout + `.tmp/mail`) |
| `TEMPORAL_ADDRESS` | workflow-host | Default `127.0.0.1:7233` |
| `TEMPORAL_NAMESPACE` | workflow-host | Default `companyos-local`; CI uses `companyos-ci` — **never share across env** |
| `INTEGRATION_BIND` | integration | Default `0.0.0.0:8095` |
| `INTEGRATION_SERVICE_URL` | gateway/clients | Default `http://127.0.0.1:8095` |
| `WEBHOOK_ENCRYPTION_KEY` | integration/core | Base64 32-byte AES key (else derived from `AUTH_JWT_SECRET`) |
| `COMPANYOS_LOCAL_AUTH` | HTTP services | Dev/test `X-CompanyOS-Dev-*` headers |

## Workflow catalogue

`ApprovalProcess`, `OrgProvisioning`, `ExpenseApproval`, `QuoteToInvoice`,
`InvoiceDunning`, `DataImport`, `UserOffboarding`, `TenantDeletion` (30-day
timer; tests use `dry_run` and must not destroy data).

Activities call HTTP APIs with `on_behalf_of` headers (stubs OK in Phase 1.8).

## OpenAPI

Each package has `examples/export_openapi.rs`.
